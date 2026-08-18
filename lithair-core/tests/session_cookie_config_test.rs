//! `CookieConfig` is the single cookie authority (post-#220 unification).
//!
//! Real servers on random ports, one process so the env vars are ours:
//! 1. `LT_SESSION_COOKIE_SECURE=false` + `LT_SESSION_COOKIE_SAMESITE=Strict`
//!    are finally consumed — the login's `Set-Cookie` drops `Secure` and
//!    says `SameSite=Strict`, the logout clears with the same attributes.
//! 2. `with_session_cookie(host_prefix: true)` wins over the env: the cookie
//!    is `__Host-session_token`, `Secure` is forced back on, and the gate,
//!    the route guard, `/auth/validate` and the logout all read that name.

use lithair_core::app::LithairServer;
use lithair_core::http::RouteGuard;
use lithair_core::rbac::{RbacUser, ServerRbacConfig};
use lithair_core::session::CookieConfig;
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

async fn spawn_server(data_dir: &std::path::Path, cookie: Option<CookieConfig>) -> String {
    let port = portpicker::pick_unused_port().expect("free port available");
    let base_url = format!("http://127.0.0.1:{}", port);

    let rbac = ServerRbacConfig::new()
        .with_user(RbacUser::new("alice", "s3cret", "Admin"))
        .with_session_store(data_dir.join("sessions").to_string_lossy().to_string())
        .with_session_duration(1234);

    let mut builder = LithairServer::new()
        .with_host("127.0.0.1")
        .with_port(port)
        .with_model::<Account>(
            data_dir.join("accounts").to_string_lossy().to_string(),
            "/api/accounts",
        )
        .with_route_guard(
            "/guarded/*",
            RouteGuard::RequireAuth { redirect_to: None, exclude: vec![] },
        )
        .with_route(hyper::Method::GET, "/guarded/ping", |_req| {
            Box::pin(async {
                Ok(hyper::Response::builder()
                    .status(200)
                    .body(http_body_util::BodyExt::boxed(http_body_util::Full::new(
                        bytes::Bytes::from_static(b"pong"),
                    )))
                    .unwrap())
            })
        })
        .with_rbac_config(rbac)
        .with_models_require_session(true);
    if let Some(cookie) = cookie {
        // Deliberately AFTER with_rbac_config: the override is order-independent.
        builder = builder.with_session_cookie(cookie);
    }

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

/// Login, assert the exact `Set-Cookie`, drive the cookie alone through the
/// gate, a route guard and `/auth/validate`, then logout and assert the clear.
async fn login_use_logout(base: &str, name: &str, set_tail: &str, clear_tail: &str) {
    let client = reqwest::Client::new(); // no cookie store: forwarded by hand

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
    let token = body["session_token"].as_str().expect("token in body");
    assert_eq!(set_cookie, format!("{name}={token}; {set_tail}"));
    let cookie_pair = format!("{name}={token}");

    for path in ["/api/accounts", "/guarded/ping"] {
        let resp = client
            .get(format!("{}{}", base, path))
            .header("cookie", &cookie_pair)
            .send()
            .await
            .expect("request sent");
        assert_eq!(resp.status(), 200, "{path} must accept the cookie {name}");
    }
    let resp = client
        .get(format!("{}/auth/validate", base))
        .header("cookie", &cookie_pair)
        .send()
        .await
        .expect("validate sent");
    let body: serde_json::Value = resp.json().await.expect("json");
    assert_eq!(body["valid"], true, "/auth/validate must read the cookie {name}");

    let resp = client
        .post(format!("{}/auth/logout", base))
        .header("cookie", &cookie_pair)
        .send()
        .await
        .expect("logout sent");
    assert_eq!(resp.status(), 200, "logout must accept the cookie {name}");
    let clear = resp.headers().get("set-cookie").expect("logout must clear the cookie");
    assert_eq!(clear.to_str().expect("ascii"), format!("{name}=; {clear_tail}"));

    let resp = client
        .get(format!("{}/api/accounts", base))
        .header("cookie", &cookie_pair)
        .send()
        .await
        .expect("gated request sent");
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn env_vars_drive_the_cookie_and_the_builder_override_wins() {
    // Process-wide, so both scenarios live in this one test.
    std::env::set_var("LT_SESSION_COOKIE_SECURE", "false");
    std::env::set_var("LT_SESSION_COOKIE_SAMESITE", "Strict");

    // 1. Env consumed: no `Secure`, `SameSite=Strict`.
    let tmp = tempfile::tempdir().expect("tmpdir");
    let base = spawn_server(tmp.path(), None).await;
    login_use_logout(
        &base,
        "session_token",
        "Path=/; Max-Age=1234; HttpOnly; SameSite=Strict",
        "Path=/; Max-Age=0; HttpOnly; SameSite=Strict",
    )
    .await;

    // 2. Builder override (`__Host-`): name prefixed, Secure forced, env ignored.
    let tmp = tempfile::tempdir().expect("tmpdir");
    let base = spawn_server(
        tmp.path(),
        Some(CookieConfig { host_prefix: true, ..CookieConfig::default() }),
    )
    .await;
    login_use_logout(
        &base,
        "__Host-session_token",
        "Path=/; Max-Age=1234; Secure; HttpOnly; SameSite=Lax",
        "Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Lax",
    )
    .await;
}
