#[path = "support/native_http.rs"]
mod support;

#[tokio::test]
async fn failed_http_mutations_leave_memory_unchanged() {
    support::failed_http_mutations().await;
}

#[tokio::test]
async fn acknowledgements_include_journal_flush() {
    support::acknowledged_mutations_reopen().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_patches_preserve_every_acknowledged_field() {
    support::concurrent_partial_edits().await;
}

#[tokio::test]
async fn replicated_and_admin_failures_are_not_published() {
    support::failed_replicated_mutations().await;
}

#[tokio::test]
async fn cancelled_request_finishes_an_admitted_commit_without_blocking_reads() {
    let fixture = support::Fixture::start().await;
    let store = fixture.handler.get_event_store().write().await;
    let mut pending = Box::pin(fixture.handler.apply_replicated_item(support::document("one")));
    assert!(futures::poll!(&mut pending).is_pending());
    // The commit is admitted, but its journal lock is deliberately unavailable.
    assert!(fixture.handler.get_by_id("one").await.is_none());
    drop(pending);
    drop(store);
    // A later mutation must wait for the detached commit, not overtake it.
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        fixture.handler.apply_replicated_item(support::document("two")),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(fixture.handler.total_item_count().await, 2);
    assert_eq!(fixture.reopen().await.total_item_count().await, 2);
    fixture.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_unique_creates_have_one_winner() {
    let fixture = support::Fixture::start().await;
    let mut requests = tokio::task::JoinSet::new();
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(24));
    for index in 0..24 {
        let client = fixture.client.clone();
        let url = fixture.base.clone();
        let barrier = barrier.clone();
        requests.spawn(async move {
            barrier.wait().await;
            client
                .post(url)
                .json(&serde_json::json!({"id":format!("{index}"), "name":"unique"}))
                .send()
                .await
                .unwrap()
                .status()
        });
    }
    let mut winners = 0;
    while let Some(result) = requests.join_next().await {
        match result.unwrap().as_u16() {
            201 => winners += 1,
            409 => {}
            status => panic!("unexpected status {status}"),
        }
    }
    assert_eq!(winners, 1);
    assert_eq!(fixture.reopen().await.total_item_count().await, 1);
    fixture.stop().await;
}

#[tokio::test]
async fn primary_key_changes_are_rejected_before_persistence() {
    let fixture = support::Fixture::start().await;
    fixture.handler.apply_replicated_item(support::document("one")).await.unwrap();
    for method in [reqwest::Method::PUT, reqwest::Method::PATCH] {
        let response = fixture
            .client
            .request(method, format!("{}/one", fixture.base))
            .json(&support::document("other"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400);
    }
    assert!(fixture
        .handler
        .apply_replicated_update("one", support::document("other"))
        .await
        .is_err());
    assert_eq!(fixture.handler.get_event_store().read().await.event_count(), 1);
    assert_eq!(fixture.reopen().await.get_by_id("one").await.unwrap().name, "one");
    fixture.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn compaction_and_mutations_preserve_all_acknowledged_records() {
    let fixture = support::Fixture::start().await;
    let mut tasks = tokio::task::JoinSet::new();
    for index in 0..40 {
        let handler = fixture.handler.clone();
        tasks.spawn(async move {
            handler
                .apply_replicated_item(support::document(&format!("{index}")))
                .await
                .unwrap();
        });
        if index % 5 == 0 {
            let handler = fixture.handler.clone();
            tasks.spawn(async move {
                handler.compact().await.unwrap();
            });
        }
    }
    while let Some(result) = tasks.join_next().await {
        result.unwrap();
    }
    assert_eq!(fixture.handler.total_item_count().await, 40);
    assert_eq!(fixture.reopen().await.total_item_count().await, 40);
    fixture.stop().await;
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, lithair_core::DeclarativeModel)]
#[retention(memory = 1)]
struct Retained {
    #[db(primary_key)]
    id: String,
    #[pinned]
    name: String,
}

#[tokio::test]
async fn evicted_snapshot_records_remain_readable_and_cannot_be_truncated() {
    use lithair_core::http::DeclarativeHttpHandler;
    let dir = tempfile::tempdir().unwrap();
    let handler = DeclarativeHttpHandler::<Retained>::new(dir.path().to_str().unwrap()).unwrap();
    handler
        .apply_replicated_item(Retained { id: "one".into(), name: "snapshot".into() })
        .await
        .unwrap();
    handler.compact().await.unwrap();
    handler
        .apply_replicated_item(Retained { id: "two".into(), name: "journal".into() })
        .await
        .unwrap();
    assert_eq!(handler.storage_count().await, 1);
    assert_eq!(handler.get_by_id("one").await.unwrap().name, "snapshot");
    let journal = std::fs::read(dir.path().join("events.raftlog")).unwrap();
    assert!(handler.compact().await.unwrap_err().contains("warm records"));
    assert_eq!(std::fs::read(dir.path().join("events.raftlog")).unwrap(), journal);
    handler
        .submit_admin_edit("one", serde_json::json!({"name":"edited"}))
        .await
        .unwrap();
    handler.apply_replicated_delete("two").await.unwrap();
    drop(handler);
    let reopened =
        DeclarativeHttpHandler::<Retained>::new_with_replay(dir.path().to_str().unwrap())
            .await
            .unwrap();
    assert_eq!(reopened.get_by_id("one").await.unwrap().name, "edited");
    assert!(reopened.get_by_id("two").await.is_none());
}

#[tokio::test]
async fn patch_on_an_external_handler_enforces_write_permissions() {
    let fixture = support::Fixture::start_with_permissions(Some(Vec::new())).await;
    fixture.handler.apply_replicated_item(support::document("one")).await.unwrap();
    let response = fixture
        .client
        .patch(format!("{}/one", fixture.base))
        .json(&serde_json::json!({"name":"forbidden"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 403);
    assert_eq!(fixture.handler.get_by_id("one").await.unwrap().name, "one");
    assert_eq!(fixture.handler.get_event_store().read().await.event_count(), 1);
    fixture.stop().await;
}

#[tokio::test]
async fn cold_reads_exclude_a_journal_suffix_not_yet_published() {
    use lithair_core::{engine::events::EventEnvelope, http::DeclarativeHttpHandler};
    let dir = tempfile::tempdir().unwrap();
    let handler = DeclarativeHttpHandler::<Retained>::new(dir.path().to_str().unwrap()).unwrap();
    for id in ["one", "two"] {
        handler
            .apply_replicated_item(Retained { id: id.into(), name: "published".into() })
            .await
            .unwrap();
    }
    assert_eq!(handler.storage_count().await, 1);
    // Model the ambiguous tail of a write that reaches the journal but has not
    // passed the handler's publication boundary. No wall-clock race is needed.
    let envelope = EventEnvelope::new(
        "RetainedUpdated".into(),
        "unpublished".into(),
        0,
        serde_json::to_string(&Retained { id: "one".into(), name: "unpublished".into() }).unwrap(),
        Some("one".into()),
        None,
    );
    handler.get_event_store().write().await.append_envelope(&envelope).unwrap();
    assert_eq!(handler.get_by_id("one").await.unwrap().name, "published");
}

#[tokio::test]
async fn bulk_create_keeps_only_the_committed_prefix_on_conflict() {
    use lithair_core::{app::LithairServer, http::DeclarativeHttpHandler};
    struct StopOnDrop(Option<tokio::sync::oneshot::Sender<()>>);
    impl Drop for StopOnDrop {
        fn drop(&mut self) {
            if let Some(stop) = self.0.take() {
                let _ = stop.send(());
            }
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("documents");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/api/documents", listener.local_addr().unwrap());
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let mut stop = StopOnDrop(Some(stop));
    let server = LithairServer::new()
        .with_data_dir(dir.path().to_string_lossy())
        .with_model::<support::Document>(path.to_string_lossy(), "/api/documents")
        .build()
        .unwrap();
    let task = tokio::spawn(server.serve_with_listener(listener, async {
        let _ = stopped.await;
    }));
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap();
    let response = client.post(format!("{base}/_bulk")).json(&serde_json::json!([
        {"id":"first", "name":"duplicate"}, {"id":"rejected", "name":"duplicate"}, {"id":"unreached", "name":"other"}
    ])).send().await.unwrap();
    assert_eq!(response.status(), 409);
    assert_eq!(client.get(format!("{base}/first")).send().await.unwrap().status(), 200);
    assert_eq!(client.get(format!("{base}/rejected")).send().await.unwrap().status(), 404);
    stop.0.take().unwrap().send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let reopened =
        DeclarativeHttpHandler::<support::Document>::new_with_replay(path.to_str().unwrap())
            .await
            .unwrap();
    assert_eq!(reopened.total_item_count().await, 1);
    assert!(reopened.get_by_id("first").await.is_some());
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn fsync_failures_are_not_acknowledged_in_json_or_binary_mode() {
    use lithair_core::{engine::events::EventStore, http::DeclarativeHttpHandler};
    for binary in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        let handler = DeclarativeHttpHandler::<support::Document>::new(path).unwrap();
        let mut store = EventStore::new_with_options(path, false, binary).unwrap();
        store.configure_batching(16_384, true);
        *handler.get_event_store().write().await = store;
        // /dev/null accepts writes and flushes, but rejects fsync. This isolates
        // sync failure from open/write failure without permissions or disk-fill races.
        std::os::unix::fs::symlink("/dev/null", dir.path().join("events.raftlog")).unwrap();
        let error = handler.apply_replicated_item(support::document("one")).await.unwrap_err();
        assert!(error.contains("sync"), "{error}");
        assert!(handler.get_by_id("one").await.is_none());
        assert!(handler.apply_replicated_item(support::document("two")).await.is_err());
    }
}
