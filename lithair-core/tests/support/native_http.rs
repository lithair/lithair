use lithair_core::app::LithairServer;
use lithair_core::http::{create_broadcaster, DeclarativeHttpHandler, HttpExposable};
use lithair_core::DeclarativeModel;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeMap, sync::Arc, time::Duration};

#[derive(Clone, Debug, Serialize, Deserialize, DeclarativeModel)]
pub struct Document {
    #[db(primary_key)]
    #[permission(write = "Write")]
    pub id: String,
    #[db(unique)]
    pub name: String,
    #[serde(flatten)]
    pub fields: BTreeMap<String, Value>,
}

pub fn document(id: &str) -> Document {
    Document { id: id.into(), name: id.into(), fields: BTreeMap::new() }
}

pub struct Fixture {
    pub handler: Arc<DeclarativeHttpHandler<Document>>,
    pub client: reqwest::Client,
    pub base: String,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<anyhow::Result<()>>>,
    pub dir: tempfile::TempDir,
}
impl Fixture {
    pub async fn start() -> Self {
        Self::start_with_permissions(None).await
    }
    pub async fn start_with_permissions(permissions: Option<Vec<String>>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut handler =
            DeclarativeHttpHandler::new(dir.path().join("documents").to_str().unwrap()).unwrap();
        if let Some(permissions) = permissions {
            handler = handler.with_permission_extractor(move |_| permissions.clone());
        }
        let handler = Arc::new(handler);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/api/documents", listener.local_addr().unwrap());
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let server = LithairServer::new()
            .with_data_dir(dir.path().to_string_lossy())
            .with_handler(handler.clone(), "/api/documents")
            .build()
            .unwrap();
        let task = tokio::spawn(server.serve_with_listener(listener, async {
            let _ = stopped.await;
        }));
        Self {
            dir,
            handler,
            base,
            client: reqwest::Client::builder().timeout(Duration::from_secs(15)).build().unwrap(),
            stop: Some(stop),
            task: Some(task),
        }
    }
    pub async fn reopen(&self) -> DeclarativeHttpHandler<Document> {
        DeclarativeHttpHandler::new_with_replay(self.dir.path().join("documents").to_str().unwrap())
            .await
            .unwrap()
    }
    pub async fn stop(mut self) {
        self.stop.take().unwrap().send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(15), self.task.take().unwrap())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

pub async fn failed_http_mutations() {
    for method in [
        reqwest::Method::POST,
        reqwest::Method::PUT,
        reqwest::Method::PATCH,
        reqwest::Method::DELETE,
    ] {
        let fixture = Fixture::start().await;
        fixture.handler.reconcile_replace_all(vec![document("kept")]).await;
        let broken = fixture.dir.path().join("documents/events.raftlog");
        std::fs::create_dir(&broken).unwrap();
        let url = if method == reqwest::Method::POST {
            fixture.base.clone()
        } else {
            format!("{}/kept", fixture.base)
        };
        let response = fixture.client.request(method.clone(), url).json(&json!({"id":if method == reqwest::Method::POST {"new"} else {"kept"}, "name":"changed"})).send().await.unwrap();
        assert_eq!(response.status(), 500, "{method}: storage failures are server errors");
        assert_eq!(
            fixture.handler.get_by_id("kept").await.unwrap().name,
            "kept",
            "{method}: failed mutation changed memory"
        );
        assert!(fixture.handler.get_by_id("new").await.is_none());
        // Repairing the path cannot clear an ambiguous commit failure in a running handler.
        std::fs::remove_dir(&broken).unwrap();
        assert!(fixture.handler.apply_replicated_item(document("later")).await.is_err());
        assert!(fixture.handler.get_by_id("later").await.is_none());
        fixture.stop().await;
    }
}

pub async fn acknowledged_mutations_reopen() {
    let fixture = Fixture::start().await;
    // Even callers changing the raw EventStore batch size must not weaken HTTP acknowledgements.
    fixture.handler.get_event_store().write().await.set_flush_every(100_000);
    assert_eq!(
        fixture
            .client
            .post(&fixture.base)
            .json(&document("one"))
            .send()
            .await
            .unwrap()
            .status(),
        201
    );
    assert_eq!(fixture.reopen().await.get_by_id("one").await.unwrap().name, "one");
    assert_eq!(
        fixture
            .client
            .patch(format!("{}/one", fixture.base))
            .json(&json!({"name":"edited"}))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(fixture.reopen().await.get_by_id("one").await.unwrap().name, "edited");
    assert_eq!(
        fixture
            .client
            .delete(format!("{}/one", fixture.base))
            .send()
            .await
            .unwrap()
            .status(),
        204
    );
    assert!(fixture.reopen().await.get_by_id("one").await.is_none());
    fixture.stop().await;
}

pub async fn concurrent_partial_edits() {
    let fixture = Fixture::start().await;
    fixture.handler.apply_replicated_item(document("one")).await.unwrap();
    let mut tasks = tokio::task::JoinSet::new();
    let barrier = Arc::new(tokio::sync::Barrier::new(32));
    for index in 0..32 {
        let client = fixture.client.clone();
        let url = format!("{}/one", fixture.base);
        let barrier = barrier.clone();
        tasks.spawn(async move {
            barrier.wait().await;
            let response = client
                .patch(url)
                .json(&json!({format!("field{index}"):index}))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
        });
    }
    while let Some(result) = tasks.join_next().await {
        result.unwrap();
    }
    let live = fixture.handler.get_by_id("one").await.unwrap();
    assert_eq!(live.fields.len(), 32, "successful partial edits must not overwrite each other");
    assert_eq!(fixture.reopen().await.get_by_id("one").await.unwrap().fields, live.fields);
    fixture.stop().await;
}

pub async fn failed_replicated_mutations() {
    for operation in ["create", "update", "delete", "admin"] {
        let dir = tempfile::tempdir().unwrap();
        let broadcaster = create_broadcaster();
        let handler = DeclarativeHttpHandler::<Document>::new(dir.path().to_str().unwrap())
            .unwrap()
            .with_sse_broadcaster(broadcaster.clone());
        handler.reconcile_replace_all(vec![document("kept")]).await;
        let mut events = broadcaster.subscribe(Document::http_base_path()).await;
        std::fs::create_dir(dir.path().join("events.raftlog")).unwrap();
        let result = match operation {
            "create" => handler.apply_replicated_item(document("new")).await,
            "update" => {
                handler
                    .apply_replicated_update(
                        "kept",
                        Document { name: "changed".into(), ..document("kept") },
                    )
                    .await
            }
            "delete" => handler.apply_replicated_delete("kept").await.map(|_| ()),
            _ => handler.submit_admin_edit("kept", json!({"name":"changed"})).await.map(|_| ()),
        };
        assert!(result.is_err(), "{operation} acknowledged a failed write");
        assert_eq!(handler.get_by_id("kept").await.unwrap().name, "kept");
        assert!(handler.get_by_id("new").await.is_none());
        assert!(matches!(
            events.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    }
}
