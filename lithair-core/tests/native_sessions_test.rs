//! RFC 308 store A: sessions replicated in the native consensus group.
#![cfg(all(feature = "cluster", feature = "tls"))]
#[path = "support/operator_files.rs"]
#[allow(dead_code)]
mod operator_files;
use lithair_core::{
    app::LithairServer,
    cluster::{
        native::{Model, NativeCluster},
        operator::{run, OperatorCommand},
    },
    rbac::{RbacUser, ServerRbacConfig},
    session::{Session, SessionManager, SessionStore},
};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};

#[derive(Clone, serde::Serialize, serde::Deserialize, lithair_core::DeclarativeModel)]
struct Ledger {
    #[db(primary_key)]
    id: String,
}
const APP: &str = "native-sessions-v1";
fn models() -> Vec<Model> {
    vec![Model::of::<Ledger>("s.ledger", "/api/ledger").unwrap(), Model::sessions()]
}

struct Node {
    cluster: NativeCluster,
    url: String,
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<anyhow::Result<()>>,
}

async fn serve(cluster: NativeCluster) -> Node {
    let rbac = ServerRbacConfig::new()
        .with_roles(vec![("Admin".into(), vec!["*".into()])])
        .with_user(RbacUser::new("alice", "secret-pass", "Admin"));
    let sessions = cluster.session_store().expect("Model::sessions() declared");
    let server = LithairServer::new()
        .with_admin_panel(false)
        .with_native_cluster(cluster.clone())
        .with_sessions(SessionManager::from_arc(sessions))
        .with_rbac_config(rbac)
        .with_models_require_session(true)
        .build()
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (stop, done) = oneshot::channel();
    let task = tokio::spawn(server.serve_with_listener(listener, async {
        let _ = done.await;
    }));
    let client = reqwest::Client::new();
    tokio::time::timeout(Duration::from_secs(10), async {
        while client.get(format!("{url}/health")).send().await.is_err() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    Node { cluster, url, stop: Some(stop), task }
}

async fn leader(nodes: &[Option<Node>]) -> usize {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            for (n, node) in nodes.iter().enumerate() {
                if let Some(node) = node {
                    if node.cluster.ready().await {
                        return n;
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("a ready leader")
}

async fn stop(node: &mut Option<Node>) {
    let mut node = node.take().unwrap();
    let _ = node.stop.take().unwrap().send(());
    (&mut node.task).await.unwrap().unwrap();
    node.cluster.shutdown().await.unwrap();
}

async fn login(client: &reqwest::Client, url: &str) -> reqwest::Response {
    client
        .post(format!("{url}/auth/login"))
        .json(&json!({"username": "alice", "password": "secret-pass"}))
        .send()
        .await
        .unwrap()
}
async fn token(resp: reqwest::Response) -> String {
    assert_eq!(resp.status(), 200);
    resp.json::<Value>().await.unwrap()["session_token"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn replicated_sessions_follow_consensus_failover_and_restart() {
    let files = operator_files::OperatorFiles::new();
    let mut listeners = Vec::new();
    for _ in 0..3 {
        listeners.push(TcpListener::bind("127.0.0.1:0").await.unwrap());
    }
    for path in files.configs.values() {
        let mut config = std::fs::read_to_string(path).unwrap();
        for (n, listener) in listeners.iter().enumerate() {
            config = config.replace(
                &format!("127.0.0.1:{}", 20001 + n),
                &listener.local_addr().unwrap().to_string(),
            );
        }
        std::fs::write(path, config).unwrap();
        run(OperatorCommand::Provision, path).await.unwrap();
    }
    let mut nodes = Vec::new();
    for (n, listener) in listeners.into_iter().enumerate() {
        let path = &files.configs[&(n as u64 + 1)];
        let cluster =
            NativeCluster::open_with_listener(path, APP, models(), listener).await.unwrap();
        nodes.push(Some(serve(cluster).await));
    }
    nodes[0].as_ref().unwrap().cluster.bootstrap().await.unwrap();
    let lead = leader(&nodes).await;
    let follower = (lead + 1) % 3;
    let url = |nodes: &[Option<Node>], n: usize| nodes[n].as_ref().unwrap().url.clone();
    let client = reqwest::Client::new();
    let get = |base: String, path: &'static str, token: Option<String>| {
        let mut req = client.get(format!("{base}{path}"));
        if let Some(token) = token {
            req = req.bearer_auth(token);
        }
        req.send()
    };

    // Only the ready leader logs in and authorizes; followers say "retry".
    let refused = login(&client, &url(&nodes, follower)).await;
    assert_eq!(refused.status(), 503);
    assert_eq!(refused.headers()["retry-after"], "1");
    let first = token(login(&client, &url(&nodes, lead)).await).await;
    let leader_url = url(&nodes, lead);
    assert_eq!(get(leader_url.clone(), "/api/ledger", None).await.unwrap().status(), 401);
    assert_eq!(
        get(leader_url.clone(), "/api/ledger", Some("forged".into()))
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        get(leader_url.clone(), "/api/ledger", Some(first.clone()))
            .await
            .unwrap()
            .status(),
        200
    );
    // The internal collection is not an HTTP route.
    assert_eq!(get(leader_url.clone(), "/elsewhere", None).await.unwrap().status(), 404);
    let follower_url = url(&nodes, follower);
    assert_eq!(
        get(follower_url.clone(), "/api/ledger", Some(first.clone()))
            .await
            .unwrap()
            .status(),
        503
    );
    assert_eq!(
        get(follower_url, "/auth/validate", Some(first.clone())).await.unwrap().status(),
        503
    );
    let valid: Value = get(leader_url.clone(), "/auth/validate", Some(first.clone()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(valid["valid"], true);

    // Logout is a consensus write: refused on a follower, then the token is
    // refused once the leader commits it.
    let logout = client
        .post(format!("{}/auth/logout", url(&nodes, follower)))
        .bearer_auth(&first)
        .send()
        .await
        .unwrap();
    assert_eq!(logout.status(), 503);
    let logout = client
        .post(format!("{leader_url}/auth/logout"))
        .bearer_auth(&first)
        .send()
        .await
        .unwrap();
    assert_eq!(logout.status(), 200);
    assert_eq!(get(leader_url.clone(), "/api/ledger", Some(first)).await.unwrap().status(), 401);

    // Expired sessions are refused, and only the leader cleans them up.
    let store = nodes[lead].as_ref().unwrap().cluster.session_store().unwrap();
    store
        .set(Session::new(
            "expired".into(),
            chrono::Utc::now() - chrono::Duration::seconds(1),
        ))
        .await
        .unwrap();
    assert_eq!(
        get(leader_url.clone(), "/api/ledger", Some("expired".into()))
            .await
            .unwrap()
            .status(),
        401
    );
    assert!(store.get("expired").await.unwrap().is_none());
    let follower_store = nodes[follower].as_ref().unwrap().cluster.session_store().unwrap();
    assert_eq!(follower_store.cleanup_expired().await.unwrap(), 0);
    assert!(store.cleanup_expired().await.unwrap() >= 1);

    // A session survives the loss of the leader that created it.
    let kept = token(login(&client, &leader_url).await).await;
    stop(&mut nodes[lead]).await;
    let elected = leader(&nodes).await;
    assert_eq!(
        get(url(&nodes, elected), "/api/ledger", Some(kept.clone()))
            .await
            .unwrap()
            .status(),
        200
    );

    // ...and a cold restart of the whole group: sessions are checkpointed.
    for node in nodes.iter_mut().filter(|n| n.is_some()) {
        stop(node).await;
    }
    // The application contract includes the sessions collection.
    let without = NativeCluster::open(
        &files.configs[&1],
        APP,
        vec![Model::of::<Ledger>("s.ledger", "/api/ledger").unwrap()],
    )
    .await;
    assert!(without.is_err(), "removing Model::sessions() changes the contract");
    for (n, node) in nodes.iter_mut().enumerate() {
        let cluster = NativeCluster::open(&files.configs[&(n as u64 + 1)], APP, models())
            .await
            .unwrap();
        *node = Some(serve(cluster).await);
    }
    let elected = leader(&nodes).await;
    assert_eq!(
        get(url(&nodes, elected), "/api/ledger", Some(kept)).await.unwrap().status(),
        200
    );
    for node in &mut nodes {
        stop(node).await;
    }
}
