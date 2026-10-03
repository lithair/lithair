//! RFC 308 step 1: any `SessionStore` reaches the gate, guards and handlers,
//! and only shared-authority stores are accepted next to a native cluster.
use lithair_core::app::{LithairServer, LithairServerBuilder};
use lithair_core::session::{MemorySessionStore, Session, SessionManager, SessionStore};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::{net::TcpListener, sync::oneshot};

/// A store Lithair has no built-in knowledge of. `shared` declares it a
/// shared authority; `sets` counts writes.
#[derive(Default)]
struct CustomStore {
    inner: MemorySessionStore,
    shared: bool,
    sets: AtomicUsize,
}
#[async_trait::async_trait]
impl SessionStore for CustomStore {
    async fn get(&self, id: &str) -> anyhow::Result<Option<Session>> {
        self.inner.get(id).await
    }
    async fn set(&self, session: Session) -> anyhow::Result<()> {
        self.sets.fetch_add(1, Ordering::SeqCst);
        self.inner.set(session).await
    }
    async fn delete(&self, id: &str) -> anyhow::Result<()> {
        self.inner.delete(id).await
    }
    async fn cleanup_expired(&self) -> anyhow::Result<usize> {
        self.inner.cleanup_expired().await
    }
    async fn count(&self) -> anyhow::Result<usize> {
        self.inner.count().await
    }
    fn shared_authority(&self) -> bool {
        self.shared
    }
}

async fn session(store: &CustomStore, id: &str) {
    let expires = chrono::Utc::now() + chrono::Duration::hours(1);
    let mut session = Session::new(id.into(), expires);
    session.set("role", "Admin").unwrap();
    store.set(session).await.unwrap();
}

struct Server {
    url: String,
    stop: Option<oneshot::Sender<()>>,
}
impl Drop for Server {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}
async fn serve(builder: LithairServerBuilder) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = builder.with_admin_panel(false).build().unwrap();
    let (stop, done) = oneshot::channel();
    tokio::spawn(server.serve_with_listener(listener, async {
        let _ = done.await;
    }));
    let client = reqwest::Client::new();
    tokio::time::timeout(Duration::from_secs(10), async {
        while client.get(format!("{url}/health")).send().await.is_err() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    Server { url, stop: Some(stop) }
}

#[derive(Clone, serde::Serialize, serde::Deserialize, lithair_core::DeclarativeModel)]
struct Item {
    #[db(primary_key)]
    id: String,
}

#[tokio::test]
async fn any_session_store_reaches_the_model_gate() {
    let store = Arc::new(CustomStore::default());
    session(&store, "valid").await;
    let dir = tempfile::tempdir().unwrap();
    // Before RFC 308 this refused to start: "unrecognized shape".
    let server = serve(
        LithairServer::new()
            .with_sessions(SessionManager::from_arc(Arc::clone(&store)))
            .with_model::<Item>(dir.path().join("items").to_string_lossy(), "/api/items")
            .with_models_require_session(true),
    )
    .await;
    let client = reqwest::Client::new();
    let anonymous = client.get(format!("{}/api/items", server.url)).send().await.unwrap();
    assert_eq!(anonymous.status(), 401);
    let authed = client
        .get(format!("{}/api/items", server.url))
        .bearer_auth("valid")
        .send()
        .await
        .unwrap();
    assert_eq!(authed.status(), 200);
}

#[tokio::test]
async fn shared_stores_are_never_rewritten_per_request() {
    use lithair_core::session::{SessionConfig, SessionMiddleware};
    for shared in [false, true] {
        let store = Arc::new(CustomStore { shared, ..Default::default() });
        session(&store, "s").await;
        let before = store.sets.load(Ordering::SeqCst);
        let config = SessionConfig::default().with_bearer_auth(true);
        let middleware = SessionMiddleware::new(Arc::clone(&store), config);
        let req = http::Request::builder().header("authorization", "Bearer s").body(()).unwrap();
        assert!(middleware.extract_session(&req).await.unwrap().is_some());
        let writes = store.sets.load(Ordering::SeqCst) - before;
        assert_eq!(
            writes,
            usize::from(!shared),
            "shared={shared}: absolute expiry, no touch write"
        );
    }
}

#[cfg(all(feature = "cluster", feature = "tls"))]
#[path = "support/operator_files.rs"]
#[allow(dead_code)]
mod operator_files;

#[cfg(all(feature = "cluster", feature = "tls"))]
mod cluster {
    use super::operator_files;
    use super::*;
    use lithair_core::app::{response, RouteRequest};
    use lithair_core::cluster::{
        native::{Model, NativeCluster},
        operator::{run, OperatorCommand},
    };

    #[derive(Clone, serde::Serialize, serde::Deserialize, lithair_core::DeclarativeModel)]
    struct Ledger {
        #[db(primary_key)]
        id: String,
    }

    /// One provisioned, unbootstrapped node: enough to exercise `build()` and
    /// the gate (which answers before the cluster; the cluster answers 503).
    async fn node() -> (operator_files::OperatorFiles, NativeCluster) {
        let files = operator_files::OperatorFiles::new();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let path = &files.configs[&1];
        let config = std::fs::read_to_string(path)
            .unwrap()
            .replace("127.0.0.1:20001", &listener.local_addr().unwrap().to_string());
        std::fs::write(path, config).unwrap();
        run(OperatorCommand::Provision, path).await.unwrap();
        let models = vec![Model::of::<Ledger>("s.ledger", "/api/ledger").unwrap()];
        let cluster = NativeCluster::open_with_listener(path, "sessions-v1", models, listener)
            .await
            .unwrap();
        (files, cluster)
    }

    #[tokio::test]
    async fn only_shared_session_stores_are_accepted_next_to_a_native_cluster() {
        let (_files, cluster) = node().await;
        let base = || {
            LithairServer::new()
                .with_admin_panel(false)
                .with_native_cluster(cluster.clone())
        };
        let local = Arc::new(CustomStore::default());
        let refused = base().with_sessions(SessionManager::from_arc(local)).build();
        let message = refused.err().expect("a local store is refused").to_string();
        assert!(message.contains("shared session store"), "{message}");
        let rbac = || {
            lithair_core::rbac::ServerRbacConfig::new()
                .with_roles(vec![("Admin".into(), vec!["*".into()])])
        };
        assert!(base().with_rbac_config(rbac()).build().is_err(), "RBAC with its local store");

        // A shared store unlocks sessions, the gate, custom routes and RBAC,
        // in either builder order.
        let shared = Arc::new(CustomStore { shared: true, ..Default::default() });
        for rbac_first in [false, true] {
            let mut builder = base();
            if rbac_first {
                builder = builder.with_rbac_config(rbac());
            }
            builder = builder.with_sessions(SessionManager::from_arc(Arc::clone(&shared)));
            if !rbac_first {
                builder = builder.with_rbac_config(rbac());
            }
            builder
                .with_models_require_session(true)
                .with_route_async(http::Method::GET, "/whoami", |_: RouteRequest| async {
                    Ok(response::json(http::StatusCode::OK, "{}"))
                })
                .build()
                .unwrap_or_else(|e| panic!("rbac_first={rbac_first}: {e}"));
        }
        cluster.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn the_session_gate_covers_native_consensus_routes() {
        let (_files, cluster) = node().await;
        let shared = Arc::new(CustomStore { shared: true, ..Default::default() });
        session(&shared, "valid").await;
        let server = serve(
            LithairServer::new()
                .with_native_cluster(cluster.clone())
                .with_sessions(SessionManager::from_arc(Arc::clone(&shared)))
                .with_models_require_session(true),
        )
        .await;
        let client = reqwest::Client::new();
        let url = format!("{}/api/ledger", server.url);
        assert_eq!(client.get(&url).send().await.unwrap().status(), 401);
        assert_eq!(client.get(&url).bearer_auth("forged").send().await.unwrap().status(), 401);
        // Past the gate: this node is not a ready leader, so the cluster says 503.
        let authed = client.get(&url).bearer_auth("valid").send().await.unwrap();
        assert_eq!(authed.status(), 503);
        drop(server);
        cluster.shutdown().await.unwrap();
    }
}
