//! Retention count-eviction during startup replay, on the multi-file
//! (segmented) event-store backend.
//!
//! History: investigating a BDD flake (#132 chain) surfaced that the old
//! `LT_MAX_LOG_FILE_SIZE` single-file "rotation" DELETED prior segments —
//! replay lost 49 of 50 items. That mechanism is removed (this PR); the
//! supported way to bound file sizes is the multi-file store, which keeps
//! one append-only log per aggregate. This test pins the promise on that
//! real backend: all items survive replay AND the retention limit is
//! re-applied. Own integration file = own process (env vars can't leak).

use lithair_core::http::DeclarativeHttpHandler;
use lithair_core::DeclarativeModel;
use serde::{Deserialize, Serialize};

#[derive(DeclarativeModel, Serialize, Deserialize, Clone, Debug)]
struct ReplayMfEmail {
    #[db(primary_key)]
    #[http(expose)]
    id: String,
    #[http(expose)]
    body: String,
}

#[tokio::test]
async fn replay_applies_retention_on_multifile_store() {
    std::env::set_var("LT_REPLAYMFEMAIL_MEMORY_RETENTION", "10");
    std::env::set_var("LT_MULTI_FILE", "1"); // the segmented backend

    let tmp = tempfile::tempdir().expect("tmpdir");
    let path = tmp.path().to_string_lossy().to_string();

    // Handler A: create 50 items (each event ≫ 512 bytes cap → rotation).
    {
        let handler = DeclarativeHttpHandler::<ReplayMfEmail>::new_with_replay(&path)
            .await
            .expect("handler A");
        for i in 0..50 {
            let item = ReplayMfEmail {
                id: format!("email-{:05}", i),
                body: format!("Body content for email {} {}", i, "x".repeat(64)),
            };
            handler.apply_replicated_item(item).await.expect("insert");
        }
        let hot = handler.storage_count().await;
        let total = handler.total_item_count().await;
        println!("LIVE:   hot={hot} total={total}");
        assert_eq!(hot, 10, "live eviction should keep 10 hot (got {hot})");
        assert_eq!(total, 50, "all 50 live items accessible (got {total})");
    }

    // Handler B: replay from disk. Retention must be re-applied.
    let handler = DeclarativeHttpHandler::<ReplayMfEmail>::new_with_replay(&path)
        .await
        .expect("handler B");
    let hot = handler.storage_count().await;
    let total = handler.total_item_count().await;
    println!("REPLAY: hot={hot} total={total}");
    assert_eq!(total, 50, "all 50 items must survive replay (got {total})");
    assert_eq!(hot, 10, "replay must re-apply the retention limit (got {hot})");
}
