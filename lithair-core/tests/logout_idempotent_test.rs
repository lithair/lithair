//! Logout is idempotent and expiry-aware; `/auth/validate` applies the gate's
//! liveness rule (post-#220 unification, D7).
//!
//! Real server on a random port, the store reached through the builder's
//! `session_manager()` (an `Arc<SessionManager<PersistentSessionStore>>`
//! after D8) so expired sessions can be planted directly.
//! 1. logout without any token → 401 + the clearing `Set-Cookie`;
//! 2. logout with a dead cookie (session deleted from the store) → 401 + clear;
//! 3. logout with a live Bearer AND a different live cookie → both sessions
//!    gone from the store, 200 + clear;
//! 4. `/auth/validate` on a session whose `expires_at` is past → `valid:false`,
//!    and the logout with that expired token → 401 + clear, entry swept.

use chrono::{Duration as ChronoDuration, Utc};
use lithair_core::app::LithairServer;
use lithair_core::rbac::{RbacUser, ServerRbacConfig};
use lithair_core::session::{PersistentSessionStore, Session, SessionManager, SessionStore};
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

const CLEAR: &str = "session_token=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Lax";

#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
struct Account {
    #[http(expose)]
    id: String,
    #[http(expose)]
    name: String,
}

async fn spawn_server(data_dir: &std::path::Path) -> (String, Arc<PersistentSessionStore>) {
    let port = portpicker::pick_unused_port().expect("free port available");
    let base_url = format!("http://127.0.0.1:{}", port);

    let rbac = ServerRbacConfig::new()
        .with_user(RbacUser::new("alice", "s3cret", "Admin"))
        .with_session_store(data_dir.join("sessions").to_string_lossy().to_string())
        .with_session_duration(1234);

    let builder = LithairServer::new()
        .with_host("127.0.0.1")
        .with_port(port)
        .with_model::<Account>(
            data_dir.join("accounts").to_string_lossy().to_string(),
            "/api/accounts",
        )
        .with_rbac_config(rbac)
        .with_models_require_session(true);

    // D8: the registered shape is the manager, and it hands out the store.
    let manager: Arc<SessionManager<PersistentSessionStore>> = builder
        .session_manager()
        .expect("with_rbac_config registers a session store")
        .downcast()
        .expect("with_rbac_config registers an Arc<SessionManager<PersistentSessionStore>>");
    let store = manager.store();

    tokio::spawn(async move {
        if let Err(e) = builder.serve().await {
            eprintln!("test server error: {}", e);
        }
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("client");
    for _ in 0..50 {
        if let Ok(resp) = client.get(format!("{}/health", base_url)).send().await {
            if resp.status().is_success() {
                return (base_url, store);
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("test server failed to start on port {}", port);
}

async fn login(client: &reqwest::Client, base: &str) -> String {
    let resp = client
        .post(format!("{}/auth/login", base))
        .json(&serde_json::json!({"username": "alice", "password": "s3cret"}))
        .send()
        .await
        .expect("login sent");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("json body");
    body["session_token"].as_str().expect("token in body").to_string()
}

fn set_cookie(resp: &reqwest::Response) -> Option<String> {
    resp.headers().get("set-cookie").map(|v| v.to_str().expect("ascii").to_string())
}

#[tokio::test]
async fn logout_is_idempotent_and_expiry_aware() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let (base, store) = spawn_server(tmp.path()).await;
    let client = reqwest::Client::new(); // no cookie store: forwarded by hand
    let logout_url = format!("{}/auth/logout", base);

    // 1. No token at all: 401, but the browser still gets the clear.
    let resp = client.post(&logout_url).send().await.expect("logout sent");
    assert_eq!(resp.status(), 401);
    assert_eq!(set_cookie(&resp).as_deref(), Some(CLEAR), "clear on the no-token path");

    // 2. Dead cookie: the session was deleted from the store → 401 + clear.
    let dead = login(&client, &base).await;
    store.delete(&dead).await.expect("delete");
    let resp = client
        .post(&logout_url)
        .header("cookie", format!("session_token={dead}"))
        .send()
        .await
        .expect("logout sent");
    assert_eq!(resp.status(), 401);
    assert_eq!(set_cookie(&resp).as_deref(), Some(CLEAR), "clear on the dead-cookie path");

    // 3. Live Bearer + different live cookie: both sessions end, 200 + clear.
    let bearer = login(&client, &base).await;
    let cookie = login(&client, &base).await;
    assert_ne!(bearer, cookie);
    let resp = client
        .post(&logout_url)
        .bearer_auth(&bearer)
        .header("cookie", format!("session_token={cookie}"))
        .send()
        .await
        .expect("logout sent");
    assert_eq!(resp.status(), 200);
    assert_eq!(set_cookie(&resp).as_deref(), Some(CLEAR));
    assert!(store.get(&bearer).await.expect("get").is_none(), "Bearer session must be gone");
    assert!(store.get(&cookie).await.expect("get").is_none(), "cookie session must be gone");
    for token in [&bearer, &cookie] {
        let resp = client
            .get(format!("{}/api/accounts", base))
            .bearer_auth(token)
            .send()
            .await
            .expect("gated request sent");
        assert_eq!(resp.status(), 401, "{token} must no longer pass the gate");
    }

    // 4. Expired session planted in the store: validate says false, the gate
    //    401s, the logout 401s + clears and sweeps the entry.
    let expired = "expired-token".to_string();
    store
        .set(Session::new(expired.clone(), Utc::now() - ChronoDuration::seconds(5)))
        .await
        .expect("set");
    let resp = client
        .get(format!("{}/auth/validate", base))
        .bearer_auth(&expired)
        .send()
        .await
        .expect("validate sent");
    let body: serde_json::Value = resp.json().await.expect("json");
    assert_eq!(body["valid"], false, "an expired session is not a session");
    let resp = client
        .get(format!("{}/api/accounts", base))
        .header("cookie", format!("session_token={expired}"))
        .send()
        .await
        .expect("gated request sent");
    assert_eq!(resp.status(), 401);
    let resp = client
        .post(&logout_url)
        .header("cookie", format!("session_token={expired}"))
        .send()
        .await
        .expect("logout sent");
    assert_eq!(resp.status(), 401);
    assert_eq!(set_cookie(&resp).as_deref(), Some(CLEAR), "clear on the expired path");
    assert!(
        store.get(&expired).await.expect("get").is_none(),
        "expired entry swept by logout"
    );

    // Nominal path unchanged: 200 + clear, session gone.
    let token = login(&client, &base).await;
    let resp = client.post(&logout_url).bearer_auth(&token).send().await.expect("logout sent");
    assert_eq!(resp.status(), 200);
    assert_eq!(set_cookie(&resp).as_deref(), Some(CLEAR));
    assert!(store.get(&token).await.expect("get").is_none());
}
