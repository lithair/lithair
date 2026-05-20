//! Regression / acceptance tests for issue #85 — the
//! `with_model_ref::<T>(data_path, base_path)` builder method that
//! returns an `Arc<DeclarativeHttpHandler<T>>` to the caller so they can
//! drive the model programmatically (background workers, OAuth
//! callbacks, scheduled jobs) **while** still getting the session gate
//! that `with_models_require_session(true)` provides.
//!
//! These tests verify three things together (the issue body's core
//! requirements):
//!
//! 1. The handle is usable from outside HTTP — calling
//!    `apply_replicated_item(...)` on the returned `Arc` does land the
//!    item, and a subsequent gated HTTP GET (with valid session) sees
//!    it.
//! 2. The session gate fires on the `with_model_ref` path identically to
//!    the `with_model` path (no auth → 401). This pins the issue body's
//!    explicit requirement that the new API "MUST automatically inherit
//!    `with_models_require_session(true)` semantics".
//! 3. Programmatic writes do NOT bypass the session gate's read side —
//!    after a programmatic write, an unauthenticated GET still returns
//!    401 (write is internal, read is still HTTP-gated).
//!
//! Mirrors the harness in `tests/require_session_test.rs` and
//! `tests/with_handler_session_gate_test.rs`.

use chrono::Utc;
use lithair_core::app::LithairServer;
use lithair_core::session::{PersistentSessionStore, Session, SessionManager, SessionStore};
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Minimal model exercised through `with_model_ref`. The `#[http(expose)]`
/// attributes are what `DeclarativeModel` keys off to surface fields in
/// the auto-generated `/api/{model}` payloads.
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
struct Mail {
    #[http(expose)]
    id: String,
    #[http(expose)]
    subject: String,
}

/// Spawn a server using `with_model_ref` and return:
/// - the base URL
/// - a valid Bearer token (seeded into the session store before the server starts)
/// - the `Arc<DeclarativeHttpHandler<Mail>>` returned to the caller (the
///   whole point of issue #85 — programmatic write access).
async fn spawn_server(
    data_dir: &std::path::Path,
) -> (String, String, std::sync::Arc<lithair_core::http::DeclarativeHttpHandler<Mail>>) {
    let port = portpicker::pick_unused_port().expect("free port available");
    let base_url = format!("http://127.0.0.1:{}", port);

    let session_dir = data_dir.join("sessions");
    let mail_dir = data_dir.join("mails");
    std::fs::create_dir_all(&session_dir).expect("create session dir");
    std::fs::create_dir_all(&mail_dir).expect("create mail dir");

    // Seed a session before the server boots so the test has a valid
    // token to present on positive-path tests.
    let seed_store = PersistentSessionStore::new(session_dir.clone()).expect("session store");
    let token = format!("test-session-{}", uuid::Uuid::new_v4());
    let session = Session::new(token.clone(), Utc::now() + chrono::Duration::hours(1));
    seed_store.set(session).await.expect("seed session");
    drop(seed_store);

    // The session store the server will use: recreate so it loads the
    // seeded event log.
    let store = PersistentSessionStore::new(session_dir).expect("session store");
    let manager = SessionManager::new(store);

    let (builder, mail_handler) = LithairServer::new()
        .with_host("127.0.0.1")
        .with_port(port)
        .with_sessions(manager)
        .with_models_require_session(true)
        .with_model_ref::<Mail>(mail_dir.to_string_lossy().to_string(), "/api/mails")
        .await
        .expect("with_model_ref");

    let returned_handle = mail_handler.clone();

    tokio::spawn(async move {
        if let Err(e) = builder.serve().await {
            eprintln!("test server error: {}", e);
        }
    });

    // Readiness probe.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("reqwest client");
    let health_url = format!("{}/health", base_url);
    for _ in 0..50 {
        if let Ok(resp) = client.get(&health_url).send().await {
            if resp.status().is_success() {
                return (base_url, token, returned_handle);
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("test server failed to start on port {}", port);
}

/// **Issue #85 requirement 1**: the returned handle is usable from outside
/// HTTP. Writing through `apply_replicated_item(...)` lands the item, and
/// a gated GET with a valid session sees it.
///
/// This is the IMAP-sync-worker use case from the issue body: the worker
/// holds the `Arc<DeclarativeHttpHandler<Mail>>` and writes mails as they
/// arrive from `FETCH`, while the HTTP API stays session-gated.
#[tokio::test]
async fn issue_85_programmatic_write_visible_through_gated_get() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, token, mail_handler) = spawn_server(tmp.path()).await;

    // Programmatic write — no HTTP involvement.
    let item = Mail { id: "m1".to_string(), subject: "hello from imap".to_string() };
    mail_handler.apply_replicated_item(item).await.expect("programmatic write");

    // Read back through the HTTP API with a valid session.
    let resp = reqwest::Client::new()
        .get(format!("{}/api/mails", base))
        .bearer_auth(&token)
        .send()
        .await
        .expect("request sent");
    assert_eq!(resp.status(), 200, "gated GET with valid session must be 200");

    // GET /api/{model} returns a wrapped envelope:
    // {"data": [...], "total": N, "skip": ..., "take": ..., "has_more": ...}
    // (see `DeclarativeHttpHandler::handle_list` in
    // lithair-core/src/http/declarative.rs).
    let body: serde_json::Value = resp.json().await.expect("json body");
    let items = body["data"].as_array().expect("response.data is an array");
    assert_eq!(items.len(), 1, "programmatic write must be visible through the API");
    assert_eq!(items[0]["id"], "m1");
    assert_eq!(items[0]["subject"], "hello from imap");
    assert_eq!(body["total"], 1);
}

/// **Issue #85 requirement 2**: the returned handle inherits
/// `with_models_require_session(true)` semantics. An unauthenticated GET
/// must return 401, just like on the `with_model` path. This is the
/// security invariant that makes `with_model_ref` safe to use in place
/// of `with_handler` for the LensMail flow.
#[tokio::test]
async fn issue_85_with_model_ref_gate_fires_no_session() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, _token, _handle) = spawn_server(tmp.path()).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/api/mails", base))
        .send()
        .await
        .expect("request sent");
    assert_eq!(
        resp.status(),
        401,
        "with_model_ref must honor with_models_require_session(true) — \
         GET without auth must be 401"
    );
}

/// **Issue #85 requirement 3**: programmatic writes via the handle do NOT
/// bypass the read-side gate. After a programmatic write, an
/// unauthenticated GET must still return 401. (The write went straight to
/// the in-memory storage + event log; the gate sits on the HTTP path, not
/// on the storage layer.)
#[tokio::test]
async fn issue_85_programmatic_write_does_not_bypass_read_gate() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, _token, mail_handler) = spawn_server(tmp.path()).await;

    let item = Mail { id: "m1".to_string(), subject: "leaked?".to_string() };
    mail_handler.apply_replicated_item(item).await.expect("programmatic write");

    let resp = reqwest::Client::new()
        .get(format!("{}/api/mails", base))
        .send()
        .await
        .expect("request sent");
    assert_eq!(
        resp.status(),
        401,
        "programmatic writes must not leak through the unauthenticated read path"
    );
}

/// Companion: `with_model_ref` without the flag set continues to work
/// (returns the handle, no gate). This pins backward-compat for callers
/// who want the handle but didn't opt into session gating.
#[tokio::test]
async fn with_model_ref_no_gate_returns_handle() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let port = portpicker::pick_unused_port().expect("free port available");
    let base = format!("http://127.0.0.1:{}", port);
    let mail_dir = tmp.path().join("mails");
    std::fs::create_dir_all(&mail_dir).expect("create mail dir");

    let (builder, mail_handler) = LithairServer::new()
        .with_host("127.0.0.1")
        .with_port(port)
        .with_model_ref::<Mail>(mail_dir.to_string_lossy().to_string(), "/api/mails")
        .await
        .expect("with_model_ref");

    tokio::spawn(async move {
        if let Err(e) = builder.serve().await {
            eprintln!("test server error: {}", e);
        }
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("reqwest client");
    let health_url = format!("{}/health", base);
    for _ in 0..50 {
        if let Ok(resp) = client.get(&health_url).send().await {
            if resp.status().is_success() {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Programmatic write works.
    let item = Mail { id: "m1".to_string(), subject: "open".to_string() };
    mail_handler.apply_replicated_item(item).await.expect("programmatic write");

    // Unauthenticated GET works (flag is off — backward-compat).
    let resp = reqwest::Client::new()
        .get(format!("{}/api/mails", base))
        .send()
        .await
        .expect("request");
    assert_eq!(resp.status(), 200, "flag off, GET must be 200 (backward compat)");
    let body: serde_json::Value = resp.json().await.expect("json body");
    let items = body["data"].as_array().expect("response.data is an array");
    assert_eq!(items.len(), 1);
}
