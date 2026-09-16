use lithair_core::app::{LithairServer, LithairServerBuilder};
use lithair_core::http::{DeclarativeHttpHandler, HttpExposable};
use lithair_core::session::{
    CookieConfig, CrossSiteCheck, MemorySessionStore, Session, SessionManager, SessionStore,
};
use lithair_macros::DeclarativeModel;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{path::Path, sync::Arc, time::Duration};

#[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
#[storage(turso, collection = "notes_v1", namespace = "tenant-a", filters("category"))]
struct Note {
    #[db(primary_key)]
    #[permission(read = "NoteRead", write = "NoteWrite")]
    id: String,
    #[http(validate = "non_empty")]
    title: String,
    category: String,
}
#[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
#[storage(turso)]
struct PublicNote {
    id: String,
    title: String,
}
#[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
#[storage(native)]
struct NativeNote {
    id: String,
    title: String,
}

fn note(id: &str) -> Value {
    json!({"id":id,"title":"kept","category":"work"})
}
fn client() -> Client {
    Client::builder().timeout(Duration::from_secs(10)).build().expect("client")
}
async fn sessions() -> MemorySessionStore {
    let store = MemorySessionStore::new();
    for (id, permissions, expired) in [
        ("writer", vec!["NoteRead", "NoteWrite"], false),
        ("reader", vec!["NoteRead"], false),
        ("empty", vec![], false),
        ("expired", vec!["NoteRead", "NoteWrite"], true),
    ] {
        let expiry = chrono::Utc::now() + chrono::Duration::hours(if expired { -1 } else { 1 });
        let mut session = Session::new(id.into(), expiry);
        session.set("permissions", permissions).expect("permissions");
        session.set("role", id).expect("role");
        store.set(session).await.expect("session");
    }
    store
}
struct Running {
    base: String,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<anyhow::Result<()>>>,
}
impl Drop for Running {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}
impl Running {
    async fn start(builder: LithairServerBuilder) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let base = format!("http://{}", listener.local_addr().expect("address"));
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let task =
            tokio::spawn(builder.build().expect("build").serve_with_listener(listener, async {
                let _ = stopped.await;
            }));
        Self { base, stop: Some(stop), task: Some(task) }
    }
    async fn stop(mut self) {
        self.stop.take().expect("stop").send(()).expect("send");
        tokio::time::timeout(Duration::from_secs(15), self.task.take().expect("task"))
            .await
            .expect("deadline")
            .expect("join")
            .expect("serve");
    }
}
async fn builder(path: &Path, schema: bool) -> LithairServerBuilder {
    let builder = LithairServer::new().with_data_dir(path.to_string_lossy().to_string());
    let builder = if schema {
        builder
            .with_declarative_model::<Note>(path.join("notes").to_string_lossy(), "/custom/notes")
    } else {
        builder.with_model::<Note>(path.join("notes").to_string_lossy(), "/custom/notes")
    };
    // Deliberately configure authentication AFTER models to pin order independence.
    builder
        .with_sessions(SessionManager::new(sessions().await))
        .with_models_require_session(true)
}

#[tokio::test]
async fn declared_storage_generates_crud_and_survives_restart_with_both_builders() {
    for schema in [false, true] {
        let tmp = tempfile::tempdir().expect("tempdir");
        let http = client();
        for boot in 0..3 {
            let running = Running::start(builder(tmp.path(), schema).await).await;
            let url = format!("{}/custom/notes", running.base);
            if boot == 0 {
                assert_eq!(
                    http.post(&url)
                        .bearer_auth("writer")
                        .json(&note("a/b"))
                        .send()
                        .await
                        .expect("create")
                        .status(),
                    201
                );
                assert_eq!(
                    http.post(&url)
                        .bearer_auth("writer")
                        .json(&note("a/b"))
                        .send()
                        .await
                        .expect("conflict")
                        .status(),
                    409
                );
                assert_eq!(
                    http.patch(format!("{url}/a%2Fb"))
                        .bearer_auth("writer")
                        .json(&json!({"title":"updated"}))
                        .send()
                        .await
                        .expect("patch")
                        .status(),
                    200
                );
            }
            let result = http
                .get(format!("{url}/a%2Fb"))
                .bearer_auth("reader")
                .send()
                .await
                .expect("get");
            if boot < 2 {
                assert_eq!(result.status(), 200);
                assert_eq!(result.json::<Value>().await.expect("json")["title"], "updated");
                let page: Value = http
                    .get(format!("{url}?category=work&limit=1"))
                    .bearer_auth("reader")
                    .send()
                    .await
                    .expect("list")
                    .json()
                    .await
                    .expect("page");
                assert_eq!(page["data"].as_array().expect("items").len(), 1);
            } else {
                assert_eq!(result.status(), 404);
            }
            if boot == 1 {
                assert_eq!(
                    http.delete(format!("{url}/a%2Fb"))
                        .bearer_auth("writer")
                        .send()
                        .await
                        .expect("delete")
                        .status(),
                    200
                );
            }
            running.stop().await;
            assert!(tmp.path().join("notes/model.db").is_file());
            assert!(!tmp.path().join("notes/events.raftlog").exists());
            assert!(!tmp.path().join("notes/schema.json").exists());
        }
    }
}

#[tokio::test]
async fn sessions_permissions_cookie_policy_and_input_bounds_are_enforced() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let running =
        Running::start(builder(tmp.path(), false).await.with_session_cookie(CookieConfig {
            name: "archive_session".into(),
            cross_site_check: CrossSiteCheck::Enforce,
            ..Default::default()
        }))
        .await;
    let http = client();
    let url = format!("{}/custom/notes", running.base);
    assert_eq!(http.get(&url).send().await.expect("anonymous").status(), 401);
    assert_eq!(
        http.get(&url).bearer_auth("expired").send().await.expect("expired").status(),
        401
    );
    assert_eq!(http.request(Method::OPTIONS, &url).send().await.expect("options").status(), 204);
    assert_eq!(
        http.post(&url)
            .bearer_auth("reader")
            .json(&note("one"))
            .send()
            .await
            .expect("read only")
            .status(),
        403
    );
    assert_eq!(
        http.post(&url)
            .bearer_auth("empty")
            .header("x-permissions", "NoteWrite")
            .json(&note("one"))
            .send()
            .await
            .expect("forged")
            .status(),
        403
    );
    assert_eq!(
        http.post(&url)
            .header("cookie", "archive_session=writer")
            .header("sec-fetch-site", "cross-site")
            .json(&note("one"))
            .send()
            .await
            .expect("csrf")
            .status(),
        403
    );
    assert_eq!(
        http.post(&url)
            .bearer_auth("writer")
            .header("sec-fetch-site", "cross-site")
            .json(&note("one"))
            .send()
            .await
            .expect("bearer")
            .status(),
        201
    );
    assert_eq!(
        http.get(format!("{url}/one"))
            .header("cookie", "archive_session=reader")
            .send()
            .await
            .expect("cookie")
            .status(),
        200
    );
    assert_eq!(
        http.get(format!("{url}/one"))
            .bearer_auth("empty")
            .send()
            .await
            .expect("private read")
            .status(),
        403
    );
    for query in ["limit=0", "limit=101", "offset=no", "unknown=x", "limit=1&limit=2"] {
        assert_eq!(
            http.get(format!("{url}?{query}"))
                .bearer_auth("reader")
                .send()
                .await
                .expect("query")
                .status(),
            400,
            "{query}"
        );
    }
    for patch in [json!({"id":"other"}), json!({"title":""}), json!({"missing":true}), json!([])] {
        assert_eq!(
            http.patch(format!("{url}/one"))
                .bearer_auth("writer")
                .json(&patch)
                .send()
                .await
                .expect("invalid patch")
                .status(),
            400
        );
    }
    let unchanged: Value = http
        .get(format!("{url}/one"))
        .bearer_auth("reader")
        .send()
        .await
        .expect("get")
        .json()
        .await
        .expect("json");
    assert_eq!(unchanged, note("one"));
    let head = http
        .head(format!("{url}/one"))
        .bearer_auth("reader")
        .send()
        .await
        .expect("head");
    assert_eq!(head.status(), 200);
    assert!(head.bytes().await.expect("body").is_empty());
    assert_eq!(
        http.post(&url)
            .bearer_auth("writer")
            .body("{}")
            .send()
            .await
            .expect("media")
            .status(),
        415
    );
    assert_eq!(
        http.post(&url)
            .bearer_auth("writer")
            .header("content-type", "application/json")
            .body("x".repeat(lithair_turso::MAX_DOCUMENT_BYTES + 1))
            .send()
            .await
            .expect("oversized")
            .status(),
        413
    );
    assert_eq!(
        http.delete(format!("{url}/one"))
            .bearer_auth("reader")
            .send()
            .await
            .expect("unauthorized delete")
            .status(),
        403
    );
    running.stop().await;
}

struct Checker;
impl lithair_core::rbac::PermissionChecker for Checker {
    fn has_permission(&self, role: &str, permission: &str) -> bool {
        role == "writer" && ["NoteRead", "NoteWrite"].contains(&permission)
    }
}
#[tokio::test]
async fn full_registration_honors_the_selected_backend_and_explicit_checker() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let running = Running::start(
        LithairServer::new()
            .with_data_dir(tmp.path().to_string_lossy().to_string())
            .with_model_full::<Note>(
                tmp.path().join("notes").to_string_lossy(),
                "/notes",
                Some(Arc::new(Checker)),
                Some(Arc::new(sessions().await)),
            ),
    )
    .await;
    let http = client();
    let url = format!("{}/notes", running.base);
    assert_eq!(
        http.post(&url)
            .bearer_auth("reader")
            .json(&note("one"))
            .send()
            .await
            .expect("denied")
            .status(),
        403
    );
    assert_eq!(
        http.post(&url)
            .bearer_auth("writer")
            .json(&note("one"))
            .send()
            .await
            .expect("allowed")
            .status(),
        201
    );
    running.stop().await;
    assert!(tmp.path().join("notes/model.db").is_file());
}

#[tokio::test]
async fn native_is_the_default_and_sql_cannot_silently_use_the_native_handler() {
    let tmp = tempfile::tempdir().expect("tempdir");
    assert!(NativeNote::storage_factory().is_none());
    assert!(PublicNote::storage_factory().is_some());
    assert!(DeclarativeHttpHandler::<PublicNote>::new(tmp.path().to_str().expect("path")).is_err());
    let running = Running::start(
        LithairServer::new()
            .with_data_dir(tmp.path().to_string_lossy().to_string())
            .with_model::<PublicNote>(tmp.path().join("sql").to_string_lossy(), "/sql")
            .with_model::<NativeNote>(tmp.path().join("native").to_string_lossy(), "/native"),
    )
    .await;
    for path in ["sql", "native"] {
        assert_eq!(
            client()
                .post(format!("{}/{path}", running.base))
                .json(&json!({"id":"one","title":"hello"}))
                .send()
                .await
                .expect("create")
                .status(),
            201
        );
    }
    running.stop().await;
    assert!(tmp.path().join("sql/model.db").is_file());
    assert!(tmp.path().join("native/events.raftlog").is_file());
}

#[tokio::test]
async fn native_admin_and_missing_session_configuration_fail_before_serving() {
    for admin in [true, false] {
        let tmp = tempfile::tempdir().expect("tempdir");
        let builder = LithairServer::new()
            .with_data_dir(tmp.path().to_string_lossy().to_string())
            .with_model::<PublicNote>(tmp.path().join("sql").to_string_lossy(), "/sql");
        let builder = if admin {
            builder.with_data_admin_public()
        } else {
            builder.with_models_require_session(true)
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let result = tokio::time::timeout(
            Duration::from_secs(10),
            builder
                .build()
                .expect("build")
                .serve_with_listener(listener, std::future::pending::<()>()),
        )
        .await
        .expect("deadline");
        let message = format!("{:#}", result.expect_err("must refuse"));
        assert!(
            message.contains(if admin { "data-admin" } else { "no session store" }),
            "{message}"
        );
    }
}

#[tokio::test]
async fn sql_rejects_native_replication_configuration() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut config = lithair_core::config::LithairConfig::default();
    config.replication.enabled = true;
    let builder = LithairServerBuilder::with_config(config)
        .with_data_dir(tmp.path().to_string_lossy().to_string())
        .with_model::<PublicNote>(tmp.path().join("sql").to_string_lossy(), "/sql");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        builder
            .build()
            .expect("build")
            .serve_with_listener(listener, std::future::pending::<()>()),
    )
    .await
    .expect("deadline");
    assert!(format!("{:#}", result.expect_err("must refuse")).contains("native clustering"));
}

#[tokio::test]
async fn backend_changes_require_an_explicit_migration() {
    let native = tempfile::tempdir().expect("native directory");
    std::fs::write(native.path().join("state.raftsnap"), "existing snapshot").expect("snapshot");
    let factory = PublicNote::storage_factory().expect("factory");
    let result = factory(native.path().to_string_lossy().into_owned()).await;
    assert!(result.is_err());
    assert!(!native.path().join("model.db").exists());
    assert_eq!(
        std::fs::read_to_string(native.path().join("state.raftsnap")).expect("snapshot"),
        "existing snapshot"
    );
    let sql = tempfile::tempdir().expect("SQL directory");
    let handler = factory(sql.path().to_string_lossy().into_owned()).await.expect("SQL handler");
    assert!(DeclarativeHttpHandler::<NativeNote>::new(sql.path().to_str().expect("path")).is_err());
    assert!(!sql.path().join("events.raftlog").exists());
    drop(handler);
}
