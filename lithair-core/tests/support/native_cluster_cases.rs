use lithair_core::{
    app::LithairServer,
    cluster::{
        native::{Model, NativeCluster},
        operator::{run, OperatorCommand},
    },
    DeclarativeModel,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};
#[path = "operator_files.rs"]
#[allow(dead_code)]
mod operator_files;

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
#[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
pub struct Record {
    #[db(primary_key)]
    #[serde(default = "new_id")]
    pub id: String,
    #[db(unique)]
    pub email: String,
    #[http(validate = "non_empty")]
    pub name: String,
    #[serde(default)]
    pub left: u64,
    #[serde(default)]
    pub right: u64,
}
#[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
struct PrivateRecord {
    #[db(primary_key)]
    id: String,
    #[permission(read = "ReadPrivate")]
    name: String,
}
fn models() -> Vec<Model> {
    vec![
        Model::of::<Record>("test.records", "/api/records").unwrap(),
        Model::of::<PrivateRecord>("test.private", "/api/private").unwrap(),
    ]
}
pub struct Node {
    pub cluster: NativeCluster,
    pub url: String,
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<anyhow::Result<()>>,
}
impl Node {
    async fn close(mut self) {
        let _ = self.stop.take().unwrap().send(());
        (&mut self.task).await.unwrap().unwrap();
        self.cluster.shutdown().await.unwrap();
    }
}
impl Drop for Node {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        self.task.abort();
    }
}
pub struct Group {
    files: operator_files::OperatorFiles,
    pub nodes: Vec<Option<Node>>,
    pub client: reqwest::Client,
}
impl Group {
    pub async fn new() -> Self {
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
        let mut group = Self {
            files,
            nodes: vec![None, None, None],
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(6))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
        };
        for (n, listener) in listeners.into_iter().enumerate() {
            group.start(n, Some(listener)).await;
        }
        for node in group.nodes.iter().flatten() {
            assert!(!node.cluster.ready().await);
        }
        assert!(group.nodes[1].as_ref().unwrap().cluster.bootstrap().await.is_err());
        group.nodes[0].as_ref().unwrap().cluster.bootstrap().await.unwrap();
        group.leader().await;
        group
    }
    async fn start(&mut self, n: usize, listener: Option<TcpListener>) {
        self.start_models(n, listener, models()).await;
    }
    async fn start_models(&mut self, n: usize, listener: Option<TcpListener>, models: Vec<Model>) {
        let config = &self.files.configs[&(n as u64 + 1)];
        let cluster = match listener {
            Some(listener) => {
                NativeCluster::open_with_listener(config, "native-test-v1", models, listener)
                    .await
                    .unwrap()
            }
            None => NativeCluster::open(config, "native-test-v1", models).await.unwrap(),
        };
        let server = LithairServer::new()
            .with_admin_panel(false)
            .with_native_cluster(cluster.clone())
            .with_data_dir(self.files.root.path().join(format!("app-{n}")).to_string_lossy())
            .build()
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (stop, done) = oneshot::channel();
        let task = tokio::spawn(server.serve_with_listener(listener, async {
            let _ = done.await;
        }));
        self.nodes[n] = Some(Node { cluster, url, stop: Some(stop), task });
        let url = self.nodes[n].as_ref().unwrap().url.clone();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if self
                    .client
                    .get(format!("{url}/health"))
                    .send()
                    .await
                    .is_ok_and(|r| r.status().is_success())
                {
                    break;
                }
                assert!(
                    !self.nodes[n].as_ref().unwrap().task.is_finished(),
                    "server exited before readiness"
                );
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
    }
    pub async fn leader(&self) -> usize {
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                for (n, node) in self.nodes.iter().enumerate() {
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
        .expect("ready majority leader")
    }
    pub fn url(&self, n: usize, path: &str) -> String {
        format!("{}{path}", self.nodes[n].as_ref().unwrap().url)
    }
    pub async fn mutate(
        &self,
        n: usize,
        method: reqwest::Method,
        path: &str,
        key: &str,
        value: Value,
    ) -> reqwest::Response {
        self.client
            .request(method, self.url(n, path))
            .header("Idempotency-Key", key)
            .json(&value)
            .send()
            .await
            .unwrap()
    }
    pub async fn stop(&mut self, n: usize) {
        self.nodes[n].take().unwrap().close().await;
    }
    pub async fn close(mut self) {
        for n in 0..3 {
            if self.nodes[n].is_some() {
                self.stop(n).await;
            }
        }
    }
}
pub async fn crud_and_retry() {
    let group = Group::new().await;
    let n = group.leader().await;
    let cluster = &group.nodes[n].as_ref().unwrap().cluster;
    assert!(LithairServer::new().with_native_cluster(cluster.clone()).build().is_err());
    assert!(LithairServer::new()
        .with_admin_panel(false)
        .with_native_cluster(cluster.clone())
        .with_data_admin()
        .build()
        .is_err());
    assert!(LithairServer::new()
        .with_admin_panel(false)
        .with_native_cluster(cluster.clone())
        .with_models_require_session(true)
        .build()
        .is_err());
    assert!(LithairServer::new()
        .with_admin_panel(false)
        .with_native_cluster(cluster.clone())
        .with_sse(true)
        .build()
        .is_err());
    assert!(LithairServer::new()
        .with_admin_panel(false)
        .with_native_cluster(cluster.clone())
        .with_model::<Record>("unused-local-directory", "/api/local")
        .build()
        .is_err());
    assert!(LithairServer::new()
        .with_admin_panel(false)
        .with_native_cluster(cluster.clone())
        .with_sessions(lithair_core::session::SessionManager::new(
            lithair_core::session::MemorySessionStore::new()
        ))
        .build()
        .is_err());
    let body = json!({"email":"one@example.test","name":"one"});
    assert_eq!(
        group
            .mutate(
                n,
                reqwest::Method::POST,
                "/api/private",
                "private",
                json!({"id":"p","name":"hidden"})
            )
            .await
            .status(),
        201
    );
    assert_eq!(
        group
            .client
            .get(group.url(n, "/api/private/p"))
            .header("x-lithair-internal-auth", "admin")
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let private: Value = group
        .client
        .get(group.url(n, "/api/private"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(private["total"], 0);
    let first = group
        .mutate(n, reqwest::Method::POST, "/api/records", "create-one", body.clone())
        .await;
    assert_eq!(first.status(), 201);
    let item: Value = first.json().await.unwrap();
    let path = format!("/api/records/{}", item["id"].as_str().unwrap());
    let again = group.mutate(n, reqwest::Method::POST, "/api/records", "create-one", body).await;
    assert_eq!(again.status(), 201);
    assert_eq!(again.json::<Value>().await.unwrap(), item);
    assert_eq!(
        group
            .mutate(
                n,
                reqwest::Method::POST,
                "/api/records",
                "create-one",
                json!({"name":"different"})
            )
            .await
            .status(),
        409
    );
    let (left, right) = tokio::join!(
        group.mutate(n, reqwest::Method::PATCH, &path, "left", json!({"left":7})),
        group.mutate(n, reqwest::Method::PATCH, &path, "right", json!({"right":9}))
    );
    assert_eq!(left.status(), 200);
    assert_eq!(right.status(), 200);
    let item: Value = group
        .client
        .get(group.url(n, &path))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(item["left"], 7);
    assert_eq!(item["right"], 9);
    let page: Value = group
        .client
        .get(group.url(n, "/api/records?name=one&take=1&sort=-name"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(page["total"], 1);
    assert_eq!(page["data"][0], item);
    assert_eq!(page["has_more"], false);
    let mut replacement = item.clone();
    replacement["name"] = json!("replaced");
    assert_eq!(
        group.mutate(n, reqwest::Method::PUT, &path, "put", replacement).await.status(),
        200
    );
    assert_eq!(
        group
            .mutate(n, reqwest::Method::PATCH, &path, "bad-key", json!({"id":"changed"}))
            .await
            .status(),
        400
    );
    assert_eq!(
        group
            .mutate(n, reqwest::Method::PATCH, &path, "bad-validation", json!({"name":""}))
            .await
            .status(),
        400
    );
    let (a, b) = tokio::join!(
        group.mutate(
            n,
            reqwest::Method::POST,
            "/api/records",
            "unique-a",
            json!({"id":"a","email":"race","name":"a"})
        ),
        group.mutate(
            n,
            reqwest::Method::POST,
            "/api/records",
            "unique-b",
            json!({"id":"b","email":"race","name":"b"})
        )
    );
    let mut statuses = vec![a.status().as_u16(), b.status().as_u16()];
    statuses.sort();
    assert_eq!(statuses, vec![201, 409]);
    let follower = (n + 1) % 3;
    let options = group
        .client
        .request(reqwest::Method::OPTIONS, group.url(follower, &path))
        .send()
        .await
        .unwrap();
    assert_eq!(options.status(), 204);
    assert!(options.headers()["allow"].to_str().unwrap().contains("PATCH"));
    assert!(options.bytes().await.unwrap().is_empty());
    for reserved in ["count", "random-id", "_schema", "stream", "_bulk"] {
        assert_eq!(
            group
                .client
                .get(group.url(n, &format!("/api/records/{reserved}")))
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
        assert_eq!(
            group
                .mutate(
                    n,
                    reqwest::Method::POST,
                    "/api/records",
                    reserved,
                    json!({"id":reserved,"email":reserved,"name":"reserved"})
                )
                .await
                .status(),
            400
        );
    }
    let rejected = group
        .mutate(follower, reqwest::Method::PATCH, &path, "follower", json!({"left":99}))
        .await;
    assert_eq!(rejected.status(), 503);
    assert!(rejected.headers().get("location").is_none());
    assert_eq!(group.client.get(group.url(follower, &path)).send().await.unwrap().status(), 503);
    let deleted = group.mutate(n, reqwest::Method::DELETE, &path, "delete", Value::Null).await;
    assert_eq!(deleted.status(), 204);
    assert!(deleted.bytes().await.unwrap().is_empty());
    assert_eq!(
        group
            .mutate(n, reqwest::Method::DELETE, &path, "delete", Value::Null)
            .await
            .status(),
        204
    );
    assert_eq!(group.client.get(group.url(n, &path)).send().await.unwrap().status(), 404);
    assert_eq!(
        group
            .client
            .get(group.url(n, "/_admin/data/models"))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(
        group
            .client
            .post(group.url(n, "/_raft/append"))
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    group.close().await;
}
pub async fn failover_and_cold_recovery() {
    let mut group = Group::new().await;
    let leader = group.leader().await;
    let body = json!({"id":"durable","email":"durable","name":"before"});
    assert_eq!(
        group
            .mutate(leader, reqwest::Method::POST, "/api/records", "survive", body.clone())
            .await
            .status(),
        201
    );
    group.stop(leader).await;
    let next = group.leader().await;
    let retry = group
        .mutate(next, reqwest::Method::POST, "/api/records", "survive", body.clone())
        .await;
    assert_eq!(retry.status(), 201);
    assert_eq!(retry.json::<Value>().await.unwrap()["id"], "durable");
    assert_eq!(
        group
            .mutate(
                next,
                reqwest::Method::PATCH,
                "/api/records/durable",
                "update",
                json!({"name":"after"})
            )
            .await
            .status(),
        200
    );
    group.start(leader, None).await;
    for n in 0..3 {
        group.stop(n).await;
    }
    // Refuse both application-version and actual declaration drift before replay.
    let error = NativeCluster::open(&group.files.configs[&1], "different-app", models())
        .await
        .err()
        .expect("application drift rejected");
    assert!(error.to_string().contains("application contract mismatch"));
    let mut changed = models();
    changed[0] = Model::of::<PrivateRecord>("test.records", "/api/records").unwrap();
    let error = NativeCluster::open(&group.files.configs[&1], "native-test-v1", changed)
        .await
        .err()
        .expect("schema drift rejected");
    assert!(error.to_string().contains("application contract mismatch"));
    for n in 0..3 {
        let mut renamed = models();
        renamed[0] = Model::of::<Record>("test.records", "/api/renamed").unwrap();
        group.start_models(n, None, renamed).await;
    }
    let n = group.leader().await;
    assert!(group.nodes[0].as_ref().unwrap().cluster.bootstrap().await.is_err());
    let item: Value = group
        .client
        .get(group.url(n, "/api/renamed/durable"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(item["name"], "after");
    assert_eq!(
        group
            .mutate(n, reqwest::Method::POST, "/api/renamed", "survive", body)
            .await
            .status(),
        201
    );
    group.close().await;
}
