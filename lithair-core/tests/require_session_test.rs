//! Regression tests for issue #78 — `with_models_require_session(true)` must
//! gate **every** auto-generated `/api/{model}` endpoint, returning HTTP 401
//! when no valid session is present, and pass through when one is.
//!
//! Pre-fix, `with_sessions(session_manager) + with_model::<T>(...)` left the
//! auto-generated CRUD endpoints fully open: the model handler was never
//! told to enforce a session check, and `with_model` did not even thread
//! the session store through to the handler. Confirmed against
//! `lithair-core/src/app/builder.rs:1503` (pre-fix `with_model` factory)
//! and `lithair-core/src/http/declarative.rs:649` (the single dispatch
//! point for all CRUD methods on `/api/{model}`).
//!
//! These tests spin up a real `LithairServer` on a random port for each
//! case and hit it via `reqwest`, mirroring the cucumber-tests pattern in
//! `cucumber-tests/src/features/world.rs`. That's heavier than a unit
//! test but it exercises the actual production path (dispatch goes through
//! `app/mod.rs::handle_model_request` → `DeclarativeHttpHandler::handle_request`)
//! rather than a parallel test-only harness, so a regression cannot hide
//! behind a different code path.

use chrono::Utc;
use lithair_core::app::LithairServer;
use lithair_core::session::{PersistentSessionStore, Session, SessionManager, SessionStore};
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Minimal model registered via `with_model(...)`. Lifecycle is the default
/// (no `#[lifecycle(...)]` attributes), so no validation rules are involved
/// — these tests are about the session gate, not field validation.
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
struct Account {
    #[http(expose)]
    id: String,
    #[http(expose)]
    name: String,
}

/// Spin up a `LithairServer` on a random port, returning the base URL and
/// (when the test seeded a session) a valid session id usable as a Bearer
/// token.
///
/// `require_session` toggles the new builder flag. `with_session_store`
/// controls whether the server is given a session store at all — needed
/// to test the "flag on but no store wired" fallback path.
async fn spawn_server(
    require_session: bool,
    with_session_store: bool,
    data_dir: &std::path::Path,
) -> (String, Option<String>) {
    let port = portpicker::pick_unused_port().expect("free port available");
    let base_url = format!("http://127.0.0.1:{}", port);

    // Two distinct subdirs so multiple parallel tests don't share state.
    let session_dir = data_dir.join("sessions");
    let account_dir = data_dir.join("accounts");
    std::fs::create_dir_all(&session_dir).expect("create session dir");
    std::fs::create_dir_all(&account_dir).expect("create account dir");

    // Seed a session before handing the store to the server, so the test
    // has a valid token to present. The session id is the Bearer token —
    // that's the contract `extract_role_from_request` / `has_valid_session`
    // both use.
    let seeded_token = if with_session_store {
        let store = PersistentSessionStore::new(session_dir.clone()).expect("session store");
        let token = format!("test-session-{}", uuid::Uuid::new_v4());
        let session = Session::new(token.clone(), Utc::now() + chrono::Duration::hours(1));
        store.set(session).await.expect("seed session");
        Some(token)
    } else {
        None
    };

    let mut builder = LithairServer::new()
        .with_host("127.0.0.1")
        .with_port(port)
        .with_model::<Account>(account_dir.to_string_lossy().to_string(), "/api/accounts");

    if with_session_store {
        // Recreate the store inside the server so it loads the seeded
        // event log. Wrap in SessionManager exactly like the issue's
        // example does — that exercises the `Arc<SessionManager<S>>`
        // downcast branch of `has_valid_session`.
        let store = PersistentSessionStore::new(session_dir).expect("session store");
        let manager = SessionManager::new(store);
        builder = builder.with_sessions(manager);
    }

    // NOTE on ordering: `with_model` was called above, BEFORE both
    // `with_sessions` and `with_models_require_session`. The fact that
    // these tests still pass is the proof that the post-factory wiring
    // in `LithairServer::serve()` correctly decouples flag-setting from
    // model registration. Pre-fix this exact ordering broke the gate.
    if require_session {
        builder = builder.with_models_require_session(true);
    }

    tokio::spawn(async move {
        if let Err(e) = builder.serve().await {
            eprintln!("test server error: {}", e);
        }
    });

    // Wait for the listener to come up. The /health endpoint is the
    // canonical readiness probe per `lithair-core/src/app/ops_endpoints.rs`.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("reqwest client");
    let health_url = format!("{}/health", base_url);
    for _ in 0..50 {
        if let Ok(resp) = client.get(&health_url).send().await {
            if resp.status().is_success() {
                return (base_url, seeded_token);
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("test server failed to start on port {}", port);
}

/// Backward-compat: with the flag off (the default), the auto-generated
/// GET /api/{model} endpoint stays open. This must remain true forever —
/// every existing Lithair user relies on it.
#[tokio::test]
async fn flag_off_no_session_get_returns_200() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, _token) = spawn_server(false, false, tmp.path()).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/api/accounts", base))
        .send()
        .await
        .expect("request sent");
    assert_eq!(
        resp.status(),
        200,
        "with flag off and no session, GET /api/accounts must be 200 (backward compat)"
    );
}

/// Flag on, no session → 401 on GET. This is the core of issue #78.
#[tokio::test]
async fn flag_on_no_session_get_returns_401() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, _token) = spawn_server(true, true, tmp.path()).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/api/accounts", base))
        .send()
        .await
        .expect("request sent");
    assert_eq!(resp.status(), 401, "with flag on and no session, GET must be 401");

    // Canonical Lithair error shape: {"error": "<message>"}. Matches the
    // existing 401 emitted by `with_model_full`'s RBAC path at
    // `lithair-core/src/http/declarative.rs:902-906`.
    let body: serde_json::Value = resp.json().await.expect("json body");
    assert_eq!(body["error"], "Authentication required", "401 body must match canonical shape");
}

/// Flag on, valid Bearer token → 200 on GET. The gate must NOT add latency
/// or transform the response when auth succeeds.
#[tokio::test]
async fn flag_on_valid_session_get_returns_200() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, token) = spawn_server(true, true, tmp.path()).await;
    let token = token.expect("seeded session token");

    let resp = reqwest::Client::new()
        .get(format!("{}/api/accounts", base))
        .bearer_auth(&token)
        .send()
        .await
        .expect("request sent");
    assert_eq!(resp.status(), 200, "with flag on and valid session, GET must be 200");
}

/// Flag on, no session → 401 on POST. The gate must hit before the request
/// body is parsed (no JSON validation should fire on an unauthenticated
/// create attempt).
#[tokio::test]
async fn flag_on_no_session_post_returns_401() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, _token) = spawn_server(true, true, tmp.path()).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/accounts", base))
        .json(&serde_json::json!({"id": "a1", "name": "alice"}))
        .send()
        .await
        .expect("request sent");
    assert_eq!(resp.status(), 401, "with flag on and no session, POST must be 401");
}

/// Flag on, no session → 401 on PATCH /api/{model}/:id.
///
/// The issue spec calls for PATCH coverage explicitly. Note that
/// `handle_request` dispatches PATCH at
/// `lithair-core/src/http/declarative.rs:714`, so the gate (added just
/// above the match block) fires before that arm.
#[tokio::test]
async fn flag_on_no_session_patch_returns_401() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, _token) = spawn_server(true, true, tmp.path()).await;

    let resp = reqwest::Client::new()
        .patch(format!("{}/api/accounts/a1", base))
        .json(&serde_json::json!({"name": "bob"}))
        .send()
        .await
        .expect("request sent");
    assert_eq!(resp.status(), 401, "with flag on and no session, PATCH must be 401");
}

/// Flag on, no session → 401 on DELETE /api/{model}/:id.
#[tokio::test]
async fn flag_on_no_session_delete_returns_401() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, _token) = spawn_server(true, true, tmp.path()).await;

    let resp = reqwest::Client::new()
        .delete(format!("{}/api/accounts/a1", base))
        .send()
        .await
        .expect("request sent");
    assert_eq!(resp.status(), 401, "with flag on and no session, DELETE must be 401");
}

/// OPTIONS preflight stays open even with the flag on. Returning 401 on
/// CORS preflight would break browsers before they ever issue the real
/// request (preflight does not carry credentials). The gate explicitly
/// exempts `Method::OPTIONS` at `lithair-core/src/http/declarative.rs`.
#[tokio::test]
async fn flag_on_options_preflight_passes() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, _token) = spawn_server(true, true, tmp.path()).await;

    let resp = reqwest::Client::new()
        .request(reqwest::Method::OPTIONS, format!("{}/api/accounts", base))
        .send()
        .await
        .expect("request sent");
    assert!(
        resp.status().is_success() || resp.status() == 204,
        "OPTIONS preflight must NOT be gated (got {})",
        resp.status()
    );
}

/// Cookie fallback: the gate accepts `Cookie: session_token=<id>` when no
/// Authorization header is provided. This matches the pre-existing pattern
/// already used by `/auth/validate` (`builder.rs:642`) and `route_guard.rs:167`,
/// so browser clients that rely on cookie sessions are not broken when the
/// new gate is turned on. (CodeRabbit / Gemini review feedback on PR #79.)
#[tokio::test]
async fn flag_on_cookie_session_get_returns_200() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, token) = spawn_server(true, true, tmp.path()).await;
    let token = token.expect("seeded session token");

    let resp = reqwest::Client::new()
        .get(format!("{}/api/accounts", base))
        .header("cookie", format!("session_token={}", token))
        .send()
        .await
        .expect("request sent");
    assert_eq!(resp.status(), 200, "Cookie session must be accepted by the gate");
}

/// Lowercase `bearer` scheme should be accepted. RFC 6750 mandates "Bearer"
/// but real-world clients send mixed case; the framework's tolerance here
/// avoids surprise 401s for clients that follow common practice.
/// (Gemini review feedback on PR #79.)
#[tokio::test]
async fn flag_on_lowercase_bearer_get_returns_200() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, token) = spawn_server(true, true, tmp.path()).await;
    let token = token.expect("seeded session token");

    let resp = reqwest::Client::new()
        .get(format!("{}/api/accounts", base))
        .header("authorization", format!("bearer {}", token)) // lowercase
        .send()
        .await
        .expect("request sent");
    assert_eq!(resp.status(), 200, "lowercase `bearer` scheme must be accepted");
}

// ------------------------------------------------------------------------
// `with_model_full(...)` exemption: the issue #78 gate is intentionally
// scoped to the simple-CRUD path (`with_model` / `with_declarative_model`).
// Handlers registered via `with_model_full(...)` have their own RBAC story
// and the global flag does NOT cover them. This test verifies that
// distinction holds by spinning up a server with the flag ON and a
// `with_model_full` registration, then confirming an anonymous GET still
// passes (the RBAC checker is not configured for this test, so the
// pre-existing `with_model_full` path is open-by-default). (CodeRabbit
// review feedback on PR #79.)
// ------------------------------------------------------------------------

/// Mirror model for `with_model_full` — same shape as Account, distinct
/// type to avoid event-store collision with the other tests.
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
struct AccountFull {
    #[http(expose)]
    id: String,
    #[http(expose)]
    name: String,
}

#[tokio::test]
async fn flag_on_with_model_full_path_is_not_gated() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let port = portpicker::pick_unused_port().expect("free port");
    let session_dir = tmp.path().join("sessions");
    let account_dir = tmp.path().join("accounts-full");
    std::fs::create_dir_all(&session_dir).expect("create session dir");
    std::fs::create_dir_all(&account_dir).expect("create account dir");

    let store = PersistentSessionStore::new(session_dir.clone()).expect("session store");
    let manager = SessionManager::new(store);

    let builder = LithairServer::new()
        .with_host("127.0.0.1")
        .with_port(port)
        .with_sessions(manager)
        .with_models_require_session(true)
        // `with_model_full` registers a handler with its own RBAC opt-in.
        // The issue #78 gate does NOT cover this path — operators who
        // want gating here must use the RBAC checker, not the new flag.
        .with_model_full::<AccountFull>(
            account_dir.to_string_lossy().to_string(),
            "/api/accounts-full",
            None, // no RBAC checker → fully open per existing semantics
            None, // no per-model session store override
        );

    tokio::spawn(async move {
        let _ = builder.serve().await;
    });

    // Wait for readiness.
    let client = reqwest::Client::builder().timeout(Duration::from_secs(2)).build().unwrap();
    let health = format!("http://127.0.0.1:{}/health", port);
    for _ in 0..50 {
        if let Ok(r) = client.get(&health).send().await {
            if r.status().is_success() {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // No session, no Authorization header — but `with_model_full`
    // registration must NOT be 401 by the issue #78 gate.
    let resp = client
        .get(format!("http://127.0.0.1:{}/api/accounts-full", port))
        .send()
        .await
        .expect("request sent");
    assert_ne!(
        resp.status(),
        401,
        "with_model_full handlers must NOT be gated by with_models_require_session — got 401 anyway"
    );
}
