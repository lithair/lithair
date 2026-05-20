//! Regression tests for issue #86 — `with_handler(handler, base_path)` must
//! respect `with_models_require_session(true)`.
//!
//! Pre-fix, `with_handler` registered its CRUD routes via raw `with_route(...)`
//! calls that internally called `handler.handle_request(...)`. That dispatch
//! path *does* honor `DeclarativeHttpHandler::require_session`, but the flag
//! was never flipped, so the gate silently never fired even when the operator
//! had explicitly opted into `with_models_require_session(true)`.
//!
//! Net effect (the issue body's exact repro): switching from
//! `.with_model::<Mail>("./data/mails", "/api/mails")` to
//! `.with_handler(mail_handler, "/api/mails")` to gain programmatic access
//! silently made `/api/mails` publicly readable. These tests pin that
//! behavior back: the gate now fires on the `with_handler` path identically
//! to the `with_model` path.
//!
//! Mirrors the harness in `tests/require_session_test.rs` (real
//! `LithairServer` on a random port, hit via `reqwest`) so the regression
//! cannot hide behind a parallel test-only dispatch path.

use chrono::Utc;
use lithair_core::app::LithairServer;
use lithair_core::http::DeclarativeHttpHandler;
use lithair_core::session::{PersistentSessionStore, Session, SessionManager, SessionStore};
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

/// Minimal model registered via `with_handler(...)`. Same shape as the
/// `Account` model in `require_session_test.rs` — the test target is the
/// builder path, not the model itself.
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
struct Mail {
    #[http(expose)]
    id: String,
    #[http(expose)]
    subject: String,
}

/// Spin up a `LithairServer` on a random port that registers `Mail` via
/// `with_handler(...)` rather than `with_model(...)`. Returns the base URL
/// and (when sessions were enabled) a valid Bearer token.
///
/// Mirrors `spawn_server` from `require_session_test.rs` but exercises the
/// `with_handler` registration path — the whole point of issue #86.
async fn spawn_server(
    require_session: bool,
    data_dir: &std::path::Path,
) -> (String, Option<String>) {
    let port = portpicker::pick_unused_port().expect("free port available");
    let base_url = format!("http://127.0.0.1:{}", port);

    let session_dir = data_dir.join("sessions");
    let mail_dir = data_dir.join("mails");
    std::fs::create_dir_all(&session_dir).expect("create session dir");
    std::fs::create_dir_all(&mail_dir).expect("create mail dir");

    // Seed a session before handing the store to the server so the test
    // has a valid Bearer token to present.
    let store = PersistentSessionStore::new(session_dir.clone()).expect("session store");
    let token = format!("test-session-{}", uuid::Uuid::new_v4());
    let session = Session::new(token.clone(), Utc::now() + chrono::Duration::hours(1));
    store.set(session).await.expect("seed session");

    // Build the handler externally — this is precisely the shape that the
    // issue body's repro uses (`Arc::new(DeclarativeHttpHandler::<Mail>::
    // new_with_replay(...))`). The caller retains an Arc clone (here: by
    // dropping after `with_handler`, but the registration path itself is
    // what triggered the regression — it clones the Arc into five route
    // closures).
    let mail_handler = Arc::new(
        DeclarativeHttpHandler::<Mail>::new_with_replay(mail_dir.to_string_lossy().as_ref())
            .await
            .expect("mail handler"),
    );

    let mut builder = LithairServer::new()
        .with_host("127.0.0.1")
        .with_port(port)
        .with_handler(mail_handler, "/api/mails");

    // Recreate the store inside the server so it loads the seeded event
    // log. Wrap in SessionManager exactly like the issue's example does —
    // that exercises the same `Arc<SessionManager<S>>` downcast branch the
    // gate uses for `has_valid_session`.
    let store = PersistentSessionStore::new(session_dir).expect("session store");
    let manager = SessionManager::new(store);
    builder = builder.with_sessions(manager);

    // NOTE on ordering: `with_handler` was called *before* both
    // `with_sessions` and `with_models_require_session`. Pre-fix, this
    // ordering broke the gate completely (route closures captured the
    // handler Arc before the flag was ever set, and the post-factory
    // wiring loop in `LithairServer::serve()` only touched
    // `model_infos` — never the externally-provided handler).
    //
    // Post-fix, `with_handler` records a deferred gate-applier callback
    // that's invoked from `serve()` after all builder methods have run,
    // so this ordering is now valid.
    if require_session {
        builder = builder.with_models_require_session(true);
    }

    tokio::spawn(async move {
        if let Err(e) = builder.serve().await {
            eprintln!("test server error: {}", e);
        }
    });

    // Readiness probe matches `tests/require_session_test.rs`.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("reqwest client");
    let health_url = format!("{}/health", base_url);
    for _ in 0..50 {
        if let Ok(resp) = client.get(&health_url).send().await {
            if resp.status().is_success() {
                return (base_url, Some(token));
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("test server failed to start on port {}", port);
}

/// **Issue #86 exact repro**: `with_sessions` + `with_models_require_session(true)`
/// + `with_handler` → GET without an Authorization header must return 401, not
/// 200. Pre-fix, this returned 200 with the full collection — a silent
/// security regression that the issue body documents in detail.
#[tokio::test]
async fn issue_86_repro_with_handler_gate_fires_no_session() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, _token) = spawn_server(true, tmp.path()).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/api/mails", base))
        .send()
        .await
        .expect("request sent");
    assert_eq!(
        resp.status(),
        401,
        "issue #86 regression: with_handler must honor with_models_require_session(true) — \
         GET /api/mails without auth must be 401, not 200"
    );
}

/// Companion: with the flag OFF, the `with_handler` path must continue to
/// behave exactly as before (open route). This pins backward-compat for
/// every existing `with_handler` user who never opted into session gating.
#[tokio::test]
async fn flag_off_with_handler_stays_open() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, _token) = spawn_server(false, tmp.path()).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/api/mails", base))
        .send()
        .await
        .expect("request sent");
    assert_eq!(
        resp.status(),
        200,
        "with flag off, with_handler routes must stay open (backward compat)"
    );
}

/// Companion: gate fires on POST too. The issue body's repro only shows GET,
/// but the gate is method-agnostic (except OPTIONS) — pinning POST avoids a
/// future regression where only the read path is gated.
#[tokio::test]
async fn issue_86_gate_fires_on_post() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, _token) = spawn_server(true, tmp.path()).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/mails", base))
        .json(&serde_json::json!({"id": "m1", "subject": "hi"}))
        .send()
        .await
        .expect("request sent");
    assert_eq!(resp.status(), 401, "with flag on, with_handler POST without auth must be 401");
}
