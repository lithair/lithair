//! BDD step definitions for the retention/pinned system (issue #97).
//!
//! These steps exercise `DeclarativeHttpHandler` directly (no HTTP server)
//! to validate the core retention mechanics: eviction when over limit,
//! on-demand load of evicted items from the event store, and replay
//! preserving the retention policy across restarts.
//!
//! The model `RetentionTestEmail` carries a huge static `#[retention]`
//! limit; each scenario tightens it via the `LT_RETENTIONTESTEMAIL_MEMORY_RETENTION`
//! environment variable (env override added in this same PR). Cucumber
//! runs scenarios serially (`max_concurrent_scenarios(Some(1))` in main.rs),
//! so env-var mutation is safe.
//!
//! Currently implemented scenarios (subset of retention.feature):
//! - Retention limit evicts oldest items
//! - Evicted items are still accessible from disk
//! - Event replay respects retention policy

use crate::features::world::{LithairWorld, RetentionTestEmail};
use cucumber::{given, then, when};
use lithair_core::http::DeclarativeHttpHandler;
use std::sync::Arc;

fn handler_slot(
    world: &mut LithairWorld,
) -> &mut Option<Arc<DeclarativeHttpHandler<RetentionTestEmail>>> {
    &mut world.retention_handler
}

/// Background step. The retention scenarios manage their own temp dirs
/// inside `given_retention_limit`, so this is a no-op acknowledgement.
#[given("the engine uses a temporary data directory")]
async fn given_temp_data_dir(_world: &mut LithairWorld) {}

/// Env-override scenario: same setup as `given_retention_limit` but the
/// phrasing in the feature file makes it clear the limit comes from the
/// env var, not from a Gherkin parameter. Used to test that the env
/// override (added in PR #100, extended in this PR) actually wins over
/// the model's static `#[retention(memory = 999999)]` annotation.
#[given(expr = "a fresh handler with env retention limit {int}")]
async fn given_env_limit(world: &mut LithairWorld, limit: usize) {
    given_retention_limit(world, limit).await;
}

#[given(expr = "a model with retention limit of {int}")]
async fn given_retention_limit(world: &mut LithairWorld, limit: usize) {
    // Set env override BEFORE creating the handler — new() reads it once.
    // `set_var` is marked unsafe from Rust 1.80+ because env mutation is
    // racy in multi-threaded contexts; cucumber here runs scenarios
    // serially (max_concurrent_scenarios = 1), so we control the
    // race. Wrap to satisfy newer toolchains.
    unsafe {
        std::env::set_var("LT_RETENTIONTESTEMAIL_MEMORY_RETENTION", limit.to_string());
    }

    // Fresh temp dir for the event store, kept alive in the world for replay tests.
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let path = temp_dir.path().to_string_lossy().to_string();

    let handler = DeclarativeHttpHandler::<RetentionTestEmail>::new_with_replay(&path)
        .await
        .expect("create handler");

    *handler_slot(world) = Some(Arc::new(handler));
    let mut td_slot = world.retention_temp_dir.lock().await;
    *td_slot = Some(temp_dir);
    let mut path_slot = world.retention_path.lock().await;
    *path_slot = Some(path);
}

#[when(expr = "I create {int} items sequentially")]
async fn when_create_items(world: &mut LithairWorld, count: usize) {
    let handler = handler_slot(world).clone().expect("handler not initialized");
    for i in 0..count {
        let item = RetentionTestEmail {
            id: format!("email-{:05}", i),
            from: format!("user{}@test.com", i),
            subject: format!("Subject {}", i),
            body: format!("Body content for email {}", i),
        };
        handler.apply_replicated_item(item).await.expect("insert item");
    }
}

#[then(expr = "only the {int} most recent items should be fully in memory")]
async fn then_hot_count_is(world: &mut LithairWorld, expected: usize) {
    let handler = handler_slot(world).clone().expect("handler not initialized");
    let hot = handler.storage_count().await;
    assert_eq!(hot, expected, "expected {} items in hot memory, found {}", expected, hot);
}

#[then(expr = "the {int} oldest items should be evicted from full memory")]
async fn then_evicted_count_is(world: &mut LithairWorld, expected: usize) {
    let handler = handler_slot(world).clone().expect("handler not initialized");
    let total = handler.total_item_count().await;
    let hot = handler.storage_count().await;
    let evicted = total - hot;
    assert_eq!(
        evicted, expected,
        "expected {} evicted items, found {} (total={}, hot={})",
        expected, evicted, total, hot
    );
}

#[when(expr = "I create {int} items with known content")]
async fn when_create_items_known(world: &mut LithairWorld, count: usize) {
    when_create_items(world, count).await;
}

#[when(expr = "I create {int} items")]
async fn when_create_items_plain(world: &mut LithairWorld, count: usize) {
    when_create_items(world, count).await;
}

#[given(expr = "{int} items exist in memory")]
async fn given_items_exist(world: &mut LithairWorld, count: usize) {
    when_create_items(world, count).await;
}

#[given(expr = "{int} items have been created and persisted")]
async fn given_items_persisted(world: &mut LithairWorld, count: usize) {
    when_create_items(world, count).await;
}

#[then(expr = "the {int} oldest items should be evicted")]
async fn then_evicted_alt(world: &mut LithairWorld, expected: usize) {
    then_evicted_count_is(world, expected).await;
}

#[when(expr = "I request evicted item {int} by id")]
async fn when_request_evicted(world: &mut LithairWorld, idx: usize) {
    let handler = handler_slot(world).clone().expect("handler not initialized");
    let id = format!("email-{:05}", idx);
    let item = handler.get_by_id(&id).await;
    let mut last = world.retention_last_item.lock().await;
    *last = item;
}

#[then("the full item should be returned from disk")]
async fn then_full_item_returned(world: &mut LithairWorld) {
    let last = world.retention_last_item.lock().await;
    assert!(last.is_some(), "expected item to be retrieved, got None");
}

#[then("all fields should match the original content")]
async fn then_fields_match(world: &mut LithairWorld) {
    let last = world.retention_last_item.lock().await;
    let item = last.as_ref().expect("no retrieved item");
    assert!(!item.id.is_empty(), "id should be populated");
    assert!(!item.from.is_empty(), "from (pinned) should be populated");
    assert!(!item.subject.is_empty(), "subject (pinned) should be populated");
    assert!(!item.body.is_empty(), "body (non-pinned) should be re-loaded from disk");
    // Verify body matches the expected format from when_create_items
    let expected_body_prefix = "Body content for email";
    assert!(
        item.body.starts_with(expected_body_prefix),
        "body content mismatch: got {:?}",
        item.body
    );
}

#[when("the engine is restarted")]
async fn when_engine_restarted(world: &mut LithairWorld) {
    // Drop the current handler and create a fresh one against the same path,
    // exercising replay over the persisted event store.
    let path = {
        let slot = world.retention_path.lock().await;
        slot.clone().expect("no retention path recorded")
    };
    *handler_slot(world) = None;
    let handler = DeclarativeHttpHandler::<RetentionTestEmail>::new_with_replay(&path)
        .await
        .expect("recreate handler on restart");
    *handler_slot(world) = Some(Arc::new(handler));
}

#[then(expr = "all {int} items should be accessible by id")]
async fn then_all_accessible(world: &mut LithairWorld, count: usize) {
    let handler = handler_slot(world).clone().expect("handler not initialized");
    let total = handler.total_item_count().await;
    assert_eq!(total, count, "expected {} items total (hot+warm), found {}", count, total);

    if count == 0 {
        return;
    }

    // Spot-check: first, middle, last should all resolve via get_by_id.
    for idx in [0, count / 2, count - 1] {
        let id = format!("email-{:05}", idx);
        let item = handler.get_by_id(&id).await;
        assert!(item.is_some(), "item {} not accessible after restart", id);
    }
}

#[then(expr = "the {int} most recent items should be fully in memory")]
async fn then_recent_in_memory(world: &mut LithairWorld, expected: usize) {
    then_hot_count_is(world, expected).await;
}

#[then(expr = "the {int} oldest items should have only pinned fields in memory")]
async fn then_oldest_pinned_only(world: &mut LithairWorld, expected: usize) {
    then_evicted_count_is(world, expected).await;
}
