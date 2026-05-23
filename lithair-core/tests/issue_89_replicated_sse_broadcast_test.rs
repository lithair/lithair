//! Regression test for issue #89 — `apply_replicated_item`,
//! `apply_replicated_update`, and `apply_replicated_delete` must broadcast
//! the same SSE events that the HTTP CRUD path (`handle_create`,
//! `handle_put`, `handle_delete`) does.
//!
//! ## The bug (LensMail v2 IMAP-sync repro)
//!
//! v0.9.0 added `with_model_ref` so a consumer (background worker, OAuth
//! callback, scheduled job) could hold an `Arc<DeclarativeHttpHandler<T>>`
//! and write items programmatically via `apply_replicated_item(...)`. But
//! the three `apply_replicated_*` methods only updated storage and the
//! event store — they did NOT call `self.broadcast_sse(...)`. Result: an
//! SSE subscriber to `/api/{model}/stream` saw `connected` + heartbeats
//! but never the actual change events when the writes came from the
//! programmatic / replicated path. This blocked LensMail Phase 4 (live
//! new-mail notifications).
//!
//! ## What this test pins
//!
//! For each of the three replicated apply methods, after the call, a
//! subscriber on the broadcaster's per-model channel receives an event
//! with the right operation name and payload:
//!
//! - `apply_replicated_item(item)`            → `operation == "create"`
//! - `apply_replicated_update(id, item)`      → `operation == "update"`
//! - `apply_replicated_delete(id)` (existed)  → `operation == "delete"`
//! - `apply_replicated_delete(id)` (missing)  → no event (idempotent no-op)
//!
//! The operation names match the HTTP path:
//! `handle_create` emits `"create"`, `handle_put` emits `"update"`,
//! `handle_delete` emits `"delete"` (see `declarative.rs:1254`,
//! `:1576`, `:1731`). The replicated UPDATE path maps to PUT-semantics
//! (full-record overwrite), not PATCH-semantics (`"patched"`), so we
//! emit `"update"`.
//!
//! ## Why this isn't routed through `with_sse(true) + with_model_ref`
//!
//! The issue body suggests the test "set up a server with `with_sse(true)`,
//! register a model via `with_model_ref`, subscribe to `/api/{model}/stream`".
//! That would be the most natural end-to-end test, but it currently doesn't
//! work for an unrelated reason: `with_model_ref` delegates to
//! `with_handler`, and `with_handler` does NOT (a) wire the server's SSE
//! broadcaster onto the handler, nor (b) register a `/stream` route. That's
//! a separate gap (tracked in the PR body for follow-up), and fixing it
//! here would balloon the diff well beyond the scope of #89.
//!
//! Instead we wire the broadcaster directly onto the handler — the same
//! way `serve()` does for `with_model`-registered handlers (see
//! `app/mod.rs:776`, `set_sse_broadcaster`). That exercises the production
//! broadcast path (`broadcast_sse` → `broadcaster.broadcast(...)`) without
//! depending on the `with_model_ref`/`with_handler` SSE-route gap.

use lithair_core::http::{create_broadcaster, DeclarativeHttpHandler, HttpExposable};
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
struct Mail {
    #[http(expose)]
    id: String,
    #[http(expose)]
    subject: String,
}

/// Build a handler with an SSE broadcaster attached, mirroring what
/// `LithairServer::serve()` does for `with_model`-registered handlers
/// (`app/mod.rs:776`). Returns the handler (so the test can call
/// `apply_replicated_*` on it) and the broadcaster (so the test can
/// subscribe to the per-model channel directly).
async fn build_handler_with_broadcaster() -> (
    Arc<DeclarativeHttpHandler<Mail>>,
    Arc<lithair_core::http::SseEventBroadcaster>,
    tempfile::TempDir,
) {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let data_path = tmp.path().join("mails").to_string_lossy().to_string();
    std::fs::create_dir_all(&data_path).expect("create data dir");

    let broadcaster = create_broadcaster();
    let handler = DeclarativeHttpHandler::<Mail>::new_with_replay(&data_path)
        .await
        .expect("handler")
        .with_sse_broadcaster(Arc::clone(&broadcaster));

    // Return the tmpdir so each test binds it in scope — automatic cleanup
    // on test exit. Previously we leaked via `std::mem::forget` which
    // prevented cleanup pressure but suppressed normal Drop.
    (Arc::new(handler), broadcaster, tmp)
}

/// `apply_replicated_item` broadcasts a `"create"` SSE event whose `data`
/// payload is the inserted item, on the model's per-base-path channel.
#[tokio::test]
async fn issue_89_replicated_item_broadcasts_create() {
    let (handler, broadcaster, _tmp) = build_handler_with_broadcaster().await;

    // Subscribe BEFORE the write — `broadcast::Sender` only delivers events
    // emitted after subscription.
    let mut rx = broadcaster.subscribe(Mail::http_base_path()).await;

    let item = Mail { id: "m1".to_string(), subject: "hello from imap".to_string() };
    handler.apply_replicated_item(item.clone()).await.expect("programmatic insert");

    let event = timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("SSE event received within 2s — issue #89 regression: apply_replicated_item silently dropped the broadcast")
        .expect("channel open");

    assert_eq!(event.operation, "create", "operation name must match HTTP POST path");
    assert_eq!(event.model_name, Mail::http_base_path());
    assert_eq!(event.data["id"], "m1");
    assert_eq!(event.data["subject"], "hello from imap");
}

/// `apply_replicated_update` broadcasts an `"update"` SSE event (PUT-style,
/// not `"patched"`) with the new item payload.
#[tokio::test]
async fn issue_89_replicated_update_broadcasts_update() {
    let (handler, broadcaster, _tmp) = build_handler_with_broadcaster().await;

    // Seed: insert first so the update path doesn't fall through to
    // `apply_replicated_item` (see `declarative.rs:593` — UPDATE on a
    // missing key short-circuits to create). We drain the seed `"create"`
    // event before the assertion-of-interest.
    handler
        .apply_replicated_item(Mail { id: "m1".to_string(), subject: "v1".to_string() })
        .await
        .expect("seed insert");

    let mut rx = broadcaster.subscribe(Mail::http_base_path()).await;

    let updated = Mail { id: "m1".to_string(), subject: "v2".to_string() };
    handler
        .apply_replicated_update("m1", updated.clone())
        .await
        .expect("programmatic update");

    let event = timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("SSE event received within 2s")
        .expect("channel open");

    assert_eq!(
        event.operation, "update",
        "replicated UPDATE must emit `\"update\"` (PUT-semantics), not `\"patched\"`"
    );
    assert_eq!(event.data["id"], "m1");
    assert_eq!(event.data["subject"], "v2");
}

/// `apply_replicated_delete` broadcasts a `"delete"` SSE event with the
/// pre-removal item payload when the key existed.
#[tokio::test]
async fn issue_89_replicated_delete_broadcasts_delete() {
    let (handler, broadcaster, _tmp) = build_handler_with_broadcaster().await;

    handler
        .apply_replicated_item(Mail { id: "m1".to_string(), subject: "goodbye".to_string() })
        .await
        .expect("seed insert");

    let mut rx = broadcaster.subscribe(Mail::http_base_path()).await;

    let existed = handler.apply_replicated_delete("m1").await.expect("delete");
    assert!(existed, "key existed before delete");

    let event = timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("SSE event received within 2s")
        .expect("channel open");

    assert_eq!(event.operation, "delete");
    assert_eq!(event.data["id"], "m1");
    assert_eq!(
        event.data["subject"], "goodbye",
        "delete event payload must carry the pre-removal item so subscribers can react before dropping it"
    );
}

/// Idempotent no-op delete (key not present) must NOT broadcast — a
/// no-op is not activity. This pins the `if let Some(item)` branch in
/// `apply_replicated_delete` and prevents a future refactor from leaking
/// phantom delete events for keys that were never there.
#[tokio::test]
async fn issue_89_replicated_delete_missing_key_does_not_broadcast() {
    let (handler, broadcaster, _tmp) = build_handler_with_broadcaster().await;

    let mut rx = broadcaster.subscribe(Mail::http_base_path()).await;

    let existed = handler.apply_replicated_delete("ghost").await.expect("delete");
    assert!(!existed, "key did not exist — idempotent no-op");

    // Receiver should NOT yield any event. Wait a short window — long
    // enough to catch a stray broadcast, short enough not to slow the
    // suite materially.
    let result = timeout(Duration::from_millis(200), rx.recv()).await;
    assert!(
        result.is_err(),
        "idempotent no-op delete must not broadcast, but channel yielded: {:?}",
        result
    );
}
