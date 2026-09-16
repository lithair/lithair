//! Native model replay must interpret operations, including historical crate-qualified events.

use lithair_core::app::{LithairServer, LithairServerBuilder};
use lithair_core::engine::events::{EventEnvelope, EventStore};
use lithair_core::http::DeclarativeHttpHandler;
use lithair_core::DeclarativeModel;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

// All tests in this binary acquire this lock, including readers of LT_ENABLE_BINARY.
static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct BinaryMode(Option<std::ffi::OsString>);
impl BinaryMode {
    fn set(enabled: bool) -> Self {
        let previous = std::env::var_os("LT_ENABLE_BINARY");
        std::env::set_var("LT_ENABLE_BINARY", if enabled { "1" } else { "0" });
        Self(previous)
    }
}
impl Drop for BinaryMode {
    fn drop(&mut self) {
        if let Some(value) = &self.0 {
            std::env::set_var("LT_ENABLE_BINARY", value);
        } else {
            std::env::remove_var("LT_ENABLE_BINARY");
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeclarativeModel)]
struct Attempt {
    #[db(primary_key)]
    id: String,
    title: String,
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
        let base = format!("http://{}/api/attempts", listener.local_addr().expect("address"));
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let task =
            tokio::spawn(builder.build().expect("server").serve_with_listener(listener, async {
                let _ = stopped.await;
            }));
        Self { base, stop: Some(stop), task: Some(task) }
    }

    async fn stop(mut self) {
        self.stop.take().expect("stop").send(()).expect("send");
        tokio::time::timeout(Duration::from_secs(15), self.task.take().expect("task"))
            .await
            .expect("shutdown deadline")
            .expect("join")
            .expect("serve");
    }
}

#[tokio::test]
async fn http_delete_stays_deleted_after_restart_and_recreation_keeps_append_order() {
    let _guard = ENV_LOCK.lock().await;
    for (binary, declarative) in [(false, false), (false, true), (true, false), (true, true)] {
        let _mode = BinaryMode::set(binary);
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("attempts");
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("client");
        for boot in 0..3 {
            let builder = LithairServer::new().with_data_dir(tmp.path().to_string_lossy());
            let builder = if declarative {
                builder.with_declarative_model::<Attempt>(path.to_string_lossy(), "/api/attempts")
            } else {
                builder.with_model::<Attempt>(path.to_string_lossy(), "/api/attempts")
            };
            let server = Running::start(builder).await;
            if boot == 0 {
                for id in ["deleted", "kept", "recreated"] {
                    assert_eq!(
                        client
                            .post(&server.base)
                            .json(&json!({"id":id,"title":"original"}))
                            .send()
                            .await
                            .expect("create")
                            .status(),
                        201
                    );
                }
                assert_eq!(
                    client
                        .patch(format!("{}/kept", server.base))
                        .json(&json!({"title":"updated"}))
                        .send()
                        .await
                        .expect("update")
                        .status(),
                    200
                );
                for id in ["deleted", "recreated"] {
                    assert_eq!(
                        client
                            .delete(format!("{}/{id}", server.base))
                            .send()
                            .await
                            .expect("delete")
                            .status(),
                        204
                    );
                }
                assert_eq!(
                    client
                        .post(&server.base)
                        .json(&json!({"id":"recreated","title":"new"}))
                        .send()
                        .await
                        .expect("recreate")
                        .status(),
                    201
                );
            }
            assert_eq!(
                client
                    .get(format!("{}/deleted", server.base))
                    .send()
                    .await
                    .expect("deleted lookup")
                    .status(),
                404,
                "boot {boot}, declarative={declarative}"
            );
            for (id, title) in [("kept", "updated"), ("recreated", "new")] {
                let response =
                    client.get(format!("{}/{id}", server.base)).send().await.expect("get");
                assert_eq!(response.status(), 200);
                assert_eq!(response.json::<Value>().await.expect("json")["title"], title);
            }
            server.stop().await;
        }
        let store = EventStore::new(path.to_str().expect("path")).expect("store");
        let events: Vec<EventEnvelope> = store
            .get_all_events()
            .expect("events")
            .into_iter()
            .map(|line| serde_json::from_str(&line).expect("envelope"))
            .collect();
        assert!(events.iter().any(|e| e.event_type == "AttemptDeleted"));
        assert!(
            events.iter().all(|e| matches!(
                e.event_type.as_str(),
                "AttemptCreated" | "AttemptUpdated" | "AttemptAdminEdit" | "AttemptDeleted"
            )),
            "unexpected event types: {:?}",
            events.iter().map(|e| &e.event_type).collect::<Vec<_>>()
        );
        assert!(events.iter().all(|e| e.event_id.starts_with("Attempt:")));
        assert!(store.verify_chain().expect("verify chain").is_fully_verified());
    }
}

fn append(store: &mut EventStore, kind: &str, id: &str, title: &str, aggregate: bool) {
    let envelope = EventEnvelope {
        event_type: kind.into(),
        event_id: format!("fixture:{}", store.event_count()),
        // Deliberately identical timestamps: append order, not time or event type, wins.
        timestamp: 42,
        payload: json!({"id":id,"title":title}).to_string(),
        aggregate_id: aggregate.then(|| id.into()),
        event_hash: None,
        previous_hash: None,
    };
    store.append_envelope(&envelope).expect("append");
}

#[tokio::test]
async fn historical_crate_paths_and_legacy_deletes_replay_without_rewriting_the_log() {
    let _guard = ENV_LOCK.lock().await;
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().to_str().expect("path");
    {
        let mut store = EventStore::new(path).expect("store");
        for id in ["deleted", "legacy", "kept", "recreated"] {
            append(&mut store, "curio_backend::models::AttemptCreated", id, "original", true);
        }
        append(&mut store, "kompri_backend::models::AttemptUpdated", "kept", "updated", true);
        append(
            &mut store,
            "kompri_backend::models::AttemptDeleted",
            "deleted",
            "original",
            true,
        );
        append(
            &mut store,
            "curio_backend::old_module::AttemptDeleted",
            "legacy",
            "original",
            false,
        );
        append(
            &mut store,
            "kompri_backend::models::AttemptDeleted",
            "recreated",
            "original",
            true,
        );
        append(&mut store, "AttemptCreated", "recreated", "new", true);
        append(&mut store, "AttemptAdminEdit", "kept", "edited", true);
        store.flush().expect("flush");
    }
    let log = tmp.path().join("events.raftlog");
    let before = std::fs::read(&log).expect("log");
    let handler = DeclarativeHttpHandler::<Attempt>::new_with_replay(path).await.expect("replay");
    assert!(handler.get_by_id("deleted").await.is_none());
    assert!(handler.get_by_id("legacy").await.is_none());
    assert_eq!(handler.total_item_count().await, 2);
    assert_eq!(handler.get_by_id("kept").await.expect("kept").title, "edited");
    assert_eq!(handler.get_by_id("recreated").await.expect("recreated").title, "new");
    assert_eq!(
        std::fs::read(&log).expect("log"),
        before,
        "replay must not rewrite historical hashes"
    );
    handler
        .submit_admin_edit("kept", json!({"title":"edited again"}))
        .await
        .expect("admin edit");
    let store = handler.get_event_store();
    store.write().await.flush().expect("flush");
    let store = store.read().await;
    let last: EventEnvelope =
        serde_json::from_str(store.get_all_events().expect("events").last().expect("last"))
            .expect("envelope");
    assert_eq!(last.event_type, "AttemptAdminEdit");
    assert!(last.event_id.starts_with("Attempt:AdminEdit:"));
    assert!(store.verify_chain().expect("chain continuation").is_fully_verified());
}

#[tokio::test]
async fn deletion_after_snapshot_removes_snapshot_state() {
    let _guard = ENV_LOCK.lock().await;
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().to_str().expect("path");
    let handler = DeclarativeHttpHandler::<Attempt>::new_with_replay(path).await.expect("handler");
    for id in ["deleted", "kept"] {
        handler
            .apply_replicated_item(Attempt { id: id.into(), title: "snapshot".into() })
            .await
            .expect("create");
    }
    handler.get_event_store().write().await.flush().expect("flush");
    handler.compact().await.expect("compact");
    assert_eq!(handler.get_event_store().read().await.event_count(), 0);
    handler.apply_replicated_delete("deleted").await.expect("delete");
    handler.get_event_store().write().await.flush().expect("flush");
    drop(handler);
    let handler = DeclarativeHttpHandler::<Attempt>::new_with_replay(path).await.expect("restart");
    assert!(handler.get_by_id("deleted").await.is_none());
    assert_eq!(handler.get_by_id("kept").await.expect("snapshot survivor").title, "snapshot");
    assert_eq!(handler.total_item_count().await, 1);
}

#[derive(Clone, Debug, Serialize, Deserialize, DeclarativeModel)]
#[retention(memory = 1)]
struct RetainedAttempt {
    #[db(primary_key)]
    id: String,
    title: String,
}

#[tokio::test]
async fn replicated_deletion_of_a_warm_record_is_persisted_and_idempotent() {
    let _guard = ENV_LOCK.lock().await;
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().to_str().expect("path");
    let handler = DeclarativeHttpHandler::<RetainedAttempt>::new_with_replay(path)
        .await
        .expect("handler");
    for id in ["warm", "hot"] {
        handler
            .apply_replicated_item(RetainedAttempt { id: id.into(), title: "original".into() })
            .await
            .expect("create");
    }
    assert_eq!(handler.storage_count().await, 1);
    assert!(handler.apply_replicated_delete("warm").await.expect("delete warm record"));
    assert!(!handler.apply_replicated_delete("warm").await.expect("repeat delete"));
    handler.get_event_store().write().await.flush().expect("flush");
    assert_eq!(
        handler.get_event_store().read().await.event_count(),
        3,
        "exactly one deletion is persisted"
    );
    drop(handler);
    let handler = DeclarativeHttpHandler::<RetainedAttempt>::new_with_replay(path)
        .await
        .expect("restart");
    assert!(handler.get_by_id("warm").await.is_none());
    assert!(handler.get_by_id("hot").await.is_some());
    assert_eq!(handler.total_item_count().await, 1);
}

#[tokio::test]
async fn replay_removes_hot_and_warm_records_even_when_delete_payload_has_an_old_schema() {
    let _guard = ENV_LOCK.lock().await;
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().to_str().expect("path");
    let handler = DeclarativeHttpHandler::<RetainedAttempt>::new_with_replay(path)
        .await
        .expect("handler");
    for id in ["deleted-warm", "kept", "deleted-hot"] {
        handler
            .apply_replicated_item(RetainedAttempt { id: id.into(), title: "original".into() })
            .await
            .expect("create");
    }
    // Append old-schema tombstones directly: identity must suffice even when
    // the historic full-object payload cannot deserialize as today's model.
    {
        let mut store = handler.get_event_store().write().await;
        for id in ["deleted-warm", "deleted-hot"] {
            let event = EventEnvelope {
                event_type: "old_crate::models::RetainedAttemptDeleted".into(),
                event_id: format!("delete:{id}"),
                timestamp: 42,
                payload: json!({"old_field":"legacy data"}).to_string(),
                aggregate_id: Some(id.into()),
                event_hash: None,
                previous_hash: None,
            };
            store.append_envelope(&event).expect("append deletion");
        }
        store.flush().expect("flush");
    }
    // The on-demand loader must stop at a tombstone, never return an older value.
    assert!(handler.get_by_id("deleted-warm").await.is_none());
    assert_eq!(handler.get_by_id("kept").await.expect("warm survivor").title, "original");
    drop(handler);
    let handler = DeclarativeHttpHandler::<RetainedAttempt>::new_with_replay(path)
        .await
        .expect("restart");
    assert!(handler.get_by_id("deleted-warm").await.is_none());
    assert!(handler.get_by_id("deleted-hot").await.is_none());
    assert_eq!(
        handler.total_item_count().await,
        1,
        "deleted IDs must leave the retention index too"
    );
    assert_eq!(handler.get_by_id("kept").await.expect("kept").title, "original");
}

#[tokio::test]
async fn model_events_do_not_apply_another_models_compatible_payload() {
    let _guard = ENV_LOCK.lock().await;
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().to_str().expect("path");
    {
        let mut store = EventStore::new(path).expect("store");
        append(&mut store, "AttemptCreated", "kept", "original", true);
        append(&mut store, "OtherAttemptDeleted", "kept", "foreign", true);
        append(&mut store, "old_crate::OtherAttemptCreated", "foreign", "foreign", true);
        append(&mut store, "AttemptDeletedExtra", "unknown", "unknown", true);
        store.flush().expect("flush");
    }
    let handler = DeclarativeHttpHandler::<Attempt>::new_with_replay(path).await.expect("replay");
    assert_eq!(handler.total_item_count().await, 1);
    assert_eq!(handler.get_by_id("kept").await.expect("kept").title, "original");
}

#[tokio::test]
async fn segmented_store_replay_preserves_each_aggregates_operation_order() {
    let _guard = ENV_LOCK.lock().await;
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().to_str().expect("path");
    let mut store = EventStore::new_with_config(path, true).expect("segmented store");
    for id in ["deleted", "kept", "recreated"] {
        append(&mut store, "old_crate::RetainedAttemptCreated", id, "original", true);
    }
    append(&mut store, "RetainedAttemptDeleted", "deleted", "original", true);
    append(&mut store, "RetainedAttemptDeleted", "recreated", "original", true);
    append(&mut store, "RetainedAttemptReplicated", "recreated", "new", true);
    store.flush().expect("flush");
    assert!(tmp.path().join("deleted/events.raftlog").is_file());
    // Use the actual segmented backend, not LT_MULTI_FILE (which only configures
    // the generic engine, not the native HTTP handler's constructor).
    let handler = DeclarativeHttpHandler::<RetainedAttempt>::new(path).expect("handler");
    *handler.get_event_store().write().await = store;
    handler.replay_events().await.expect("replay");
    assert!(handler.get_by_id("deleted").await.is_none());
    assert_eq!(handler.get_by_id("recreated").await.expect("recreated").title, "new");
    assert_eq!(handler.get_by_id("kept").await.expect("kept").title, "original");
    assert_eq!(handler.total_item_count().await, 2);
}
