//! Regression tests for issue #80 — `SessionManager::new(arc_store)` produced
//! a `SessionManager<Arc<PersistentSessionStore>>` (double-Arc) shape that
//! `has_valid_session` did not handle, silently returning 401 on every
//! request even when a valid token was presented.
//!
//! See:
//! - `lithair-core/src/session/manager.rs` (`new` vs `from_arc`)
//! - `lithair-core/src/http/declarative.rs:has_valid_session`
//! - `lithair-core/src/app/mod.rs::serve` (fail-fast downcast at startup)

use chrono::Utc;
use lithair_core::app::LithairServer;
use lithair_core::session::{PersistentSessionStore, Session, SessionManager, SessionStore};
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
struct Account {
    #[http(expose)]
    id: String,
    #[http(expose)]
    name: String,
}

/// Spin up a server using `SessionManager::from_arc(arc_store)`. This is the
/// "I already have an Arc<Store> for my login handler" path — the one the
/// example previously got wrong by calling `SessionManager::new(arc_store)`.
/// Now the constructor is split so the consumer can pick the right one.
#[tokio::test]
async fn from_arc_constructor_gates_correctly() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let session_dir = tmp.path().join("sessions");
    let account_dir = tmp.path().join("accounts");
    std::fs::create_dir_all(&session_dir).expect("create session dir");
    std::fs::create_dir_all(&account_dir).expect("create account dir");

    // Seed a session in the event log so the server-side store loads it.
    let token = {
        let store = PersistentSessionStore::new(session_dir.clone()).expect("session store");
        let token = format!("test-session-{}", uuid::Uuid::new_v4());
        let session = Session::new(token.clone(), Utc::now() + chrono::Duration::hours(1));
        store.set(session).await.expect("seed session");
        token
    };

    let port = portpicker::pick_unused_port().expect("free port");
    let base = format!("http://127.0.0.1:{}", port);

    // The consumer pattern that issue #80 was about: keep an
    // `Arc<PersistentSessionStore>` to share with a login handler, and
    // build the manager from that Arc. With the new `from_arc` constructor
    // this produces `SessionManager<PersistentSessionStore>` — the shape
    // `has_valid_session` knows how to downcast.
    let store = Arc::new(PersistentSessionStore::new(session_dir).expect("session store"));
    let _shared_for_login = store.clone(); // mirror real consumer usage
    let manager = SessionManager::from_arc(store);

    let builder = LithairServer::new()
        .with_host("127.0.0.1")
        .with_port(port)
        .with_sessions(manager)
        .with_models_require_session(true)
        .with_model::<Account>(account_dir.to_string_lossy().to_string(), "/api/accounts");

    tokio::spawn(async move {
        if let Err(e) = builder.serve().await {
            eprintln!("test server error: {}", e);
        }
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("reqwest");
    let health = format!("{}/health", base);
    let mut ready = false;
    for _ in 0..50 {
        if let Ok(r) = client.get(&health).send().await {
            if r.status().is_success() {
                ready = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(ready, "server failed to start");

    // Valid token → 200. This is the regression: pre-fix, this was 401.
    let resp = client
        .get(format!("{}/api/accounts", base))
        .bearer_auth(&token)
        .send()
        .await
        .expect("request sent");
    assert_eq!(
        resp.status(),
        200,
        "valid token via from_arc constructor must be accepted (issue #80 regression)"
    );

    // No token → still 401 (gate is engaged).
    let resp = client.get(format!("{}/api/accounts", base)).send().await.expect("request sent");
    assert_eq!(resp.status(), 401, "no token must still 401 (gate engaged)");
}

/// Fail-fast at boot: if a `SessionManager<Arc<PersistentSessionStore>>` ever
/// reaches `serve()` with `with_models_require_session(true)`, the server
/// must refuse to start with a clear diagnostic — never silently 401 every
/// request.
///
/// This is defense in depth: after the constructor split, the bad shape
/// should be hard to construct accidentally, but if it does happen (e.g.
/// a consumer reaches for an older API, or a different store wrapper type
/// slips in), `serve()` catches it before binding the port.
#[tokio::test]
async fn boot_rejects_unrecognized_session_store_shape() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let session_dir = tmp.path().join("sessions");
    let account_dir = tmp.path().join("accounts");
    std::fs::create_dir_all(&session_dir).expect("create session dir");
    std::fs::create_dir_all(&account_dir).expect("create account dir");

    // Construct the broken double-Arc shape by calling
    // `SessionManager::new(arc_store)` — the exact mis-use from issue #80.
    // S resolves to `Arc<PersistentSessionStore>` here, so the internal
    // field is `Arc<Arc<PersistentSessionStore>>` and the registered
    // `session_manager` is `Arc<SessionManager<Arc<PersistentSessionStore>>>`.
    let arc_store = Arc::new(PersistentSessionStore::new(session_dir).expect("session store"));
    let manager = SessionManager::new(arc_store);

    let port = portpicker::pick_unused_port().expect("free port");

    let result = LithairServer::new()
        .with_host("127.0.0.1")
        .with_port(port)
        .with_sessions(manager)
        .with_models_require_session(true)
        .with_model::<Account>(account_dir.to_string_lossy().to_string(), "/api/accounts")
        .serve()
        .await;

    let err = result.expect_err("serve() must refuse to start with the broken double-Arc shape");
    let msg = format!("{:#}", err);
    assert!(
        msg.contains("session store"),
        "diagnostic must mention session store; got: {msg}"
    );
    assert!(
        msg.contains("from_arc") || msg.contains("Arc"),
        "diagnostic must point at the Arc / from_arc fix; got: {msg}"
    );
}

/// With the flag OFF, an unrecognized shape must NOT cause a boot failure —
/// the gate is opt-in, and other parts of the framework (e.g. cookie auth
/// on user-defined routes) may not need the same downcast contract. This
/// preserves backward compatibility for every consumer who is not opting
/// into the new gate.
#[tokio::test]
async fn boot_does_not_reject_unrecognized_shape_when_flag_off() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let session_dir = tmp.path().join("sessions");
    let account_dir = tmp.path().join("accounts");
    std::fs::create_dir_all(&session_dir).expect("create session dir");
    std::fs::create_dir_all(&account_dir).expect("create account dir");

    let arc_store = Arc::new(PersistentSessionStore::new(session_dir).expect("session store"));
    let manager = SessionManager::new(arc_store);

    let port = portpicker::pick_unused_port().expect("free port");
    let base = format!("http://127.0.0.1:{}", port);

    let builder = LithairServer::new()
        .with_host("127.0.0.1")
        .with_port(port)
        .with_sessions(manager) // flag OFF (default)
        .with_model::<Account>(account_dir.to_string_lossy().to_string(), "/api/accounts");

    tokio::spawn(async move {
        let _ = builder.serve().await;
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("reqwest");
    let health = format!("{}/health", base);
    let mut ready = false;
    for _ in 0..50 {
        if let Ok(r) = client.get(&health).send().await {
            if r.status().is_success() {
                ready = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(ready, "server must start when flag is OFF, even with double-Arc shape");
}

/// Backward-compat: the original `new(store_by_value)` path still works.
/// The existing tests in `require_session_test.rs` already cover this in
/// detail; this is a smoke test that the constructor itself didn't regress.
#[tokio::test]
async fn new_constructor_by_value_still_works() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let session_dir = tmp.path().join("sessions");

    let store = PersistentSessionStore::new(session_dir).expect("session store");
    let manager = SessionManager::new(store);

    // Setting and getting a session through the manager must work.
    let token = "smoke-token".to_string();
    let session = Session::new(token.clone(), Utc::now() + chrono::Duration::hours(1));
    manager.set_session(session).await.expect("set session");
    let got = manager.get_session(&token).await.expect("get session");
    assert!(got.is_some(), "new(store_by_value) must round-trip a session");
}
