//! Issue #219 — the RBAC login issues a `session_token` cookie and the logout
//! accepts one (Bearer or cookie) and clears it.
//!
//! Real server on a random port: login → `Set-Cookie` present → gated request
//! with that cookie alone (no Bearer) → 200 → logout with that cookie alone →
//! `Set-Cookie` with `Max-Age=0` → the gated request 401s again.

use lithair_core::app::LithairServer;
use lithair_core::rbac::{RbacUser, ServerRbacConfig};
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
struct Account {
    #[http(expose)]
    id: String,
    #[http(expose)]
    name: String,
}

async fn spawn_server(data_dir: &std::path::Path) -> String {
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
                return base_url;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("test server failed to start on port {}", port);
}

#[tokio::test]
async fn login_sets_cookie_gate_accepts_it_logout_clears_it() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let base = spawn_server(tmp.path()).await;
    let client = reqwest::Client::new(); // no cookie store: we forward the cookie by hand

    // Login: body unchanged, plus the Set-Cookie header.
    let resp = client
        .post(format!("{}/auth/login", base))
        .json(&serde_json::json!({"username": "alice", "password": "s3cret"}))
        .send()
        .await
        .expect("login sent");
    assert_eq!(resp.status(), 200);
    let set_cookie = resp
        .headers()
        .get("set-cookie")
        .expect("login must set the session cookie")
        .to_str()
        .expect("ascii")
        .to_string();
    let body: serde_json::Value = resp.json().await.expect("json body");
    let token = body["session_token"].as_str().expect("token in body").to_string();
    assert_eq!(
        set_cookie,
        format!("session_token={token}; Path=/; HttpOnly; SameSite=Strict; Secure; Max-Age=1234")
    );
    let cookie_pair = set_cookie.split(';').next().expect("pair").to_string();

    // Gated request with the cookie alone (no Bearer) passes.
    let resp = client
        .get(format!("{}/api/accounts", base))
        .header("cookie", &cookie_pair)
        .send()
        .await
        .expect("gated request sent");
    assert_eq!(resp.status(), 200, "the cookie the login issued must satisfy the gate");

    // Logout with the cookie alone clears it.
    let resp = client
        .post(format!("{}/auth/logout", base))
        .header("cookie", &cookie_pair)
        .send()
        .await
        .expect("logout sent");
    assert_eq!(resp.status(), 200, "logout must accept the session cookie");
    let clear = resp.headers().get("set-cookie").expect("logout must clear the cookie");
    assert_eq!(
        clear.to_str().expect("ascii"),
        "session_token=; Path=/; HttpOnly; SameSite=Strict; Secure; Max-Age=0"
    );

    // The session is gone: same cookie now 401s.
    let resp = client
        .get(format!("{}/api/accounts", base))
        .header("cookie", &cookie_pair)
        .send()
        .await
        .expect("gated request sent");
    assert_eq!(resp.status(), 401);
}
