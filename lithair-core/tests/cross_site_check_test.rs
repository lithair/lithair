//! Issue #225 — cross-site request check on cookie-authenticated
//! state-changing endpoints, on a real server.
//!
//! Unsafe methods (`POST/PUT/PATCH/DELETE`) whose credential is the session
//! cookie are rejected with 403 when the request is cross-site
//! (`Sec-Fetch-Site: cross-site`, or a mismatching `Origin`/`Referer` when
//! the header is absent). Bearer requests, safe methods, same-origin
//! requests, and header-less non-browser clients all keep working, and
//! `cross_site_check: Off` disarms the check entirely. The logout 403 must
//! not itself log the victim out (no session deleted, no clearing cookie).

use lithair_core::app::LithairServer;
use lithair_core::rbac::{RbacUser, ServerRbacConfig};
use lithair_core::session::{CookieConfig, CrossSiteCheck};
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
        .with_session_store(data_dir.join("sessions").to_string_lossy().to_string());

    let mut builder = LithairServer::new()
        .with_host("127.0.0.1")
        .with_port(port)
        .with_model::<Account>(
            data_dir.join("accounts").to_string_lossy().to_string(),
            "/api/accounts",
        )
        .with_rbac_config(rbac)
        .with_models_require_session(true);
    if let Some(cookie) = cookie {
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

/// Login and return `(cookie_pair, token)`.
async fn login(client: &reqwest::Client, base: &str) -> (String, String) {
    let resp = client
        .post(format!("{}/auth/login", base))
        .json(&serde_json::json!({"username": "alice", "password": "s3cret"}))
        .send()
        .await
        .expect("login sent");
    assert_eq!(resp.status(), 200);
    let cookie_pair = resp
        .headers()
        .get("set-cookie")
        .expect("login sets the cookie")
        .to_str()
        .expect("ascii")
        .split(';')
        .next()
        .expect("pair")
        .to_string();
    let body: serde_json::Value = resp.json().await.expect("json body");
    let token = body["session_token"].as_str().expect("token in body").to_string();
    (cookie_pair, token)
}

fn account_body(id: &str) -> serde_json::Value {
    serde_json::json!({"id": id, "name": "n"})
}

#[tokio::test]
async fn cookie_authenticated_cross_site_mutations_are_rejected() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let base = spawn_server(tmp.path(), None).await;
    let client = reqwest::Client::new();
    let (cookie, token) = login(&client, &base).await;
    let api = format!("{}/api/accounts", base);

    // Cookie + Sec-Fetch-Site: cross-site on a POST → 403, nothing written.
    let resp = client
        .post(&api)
        .header("cookie", &cookie)
        .header("sec-fetch-site", "cross-site")
        .json(&account_body("a1"))
        .send()
        .await
        .expect("sent");
    assert_eq!(resp.status(), 403);
    assert_eq!(resp.text().await.expect("body"), r#"{"error":"cross-site request rejected"}"#);

    // Same request as a GET (safe method) → allowed.
    let resp = client
        .get(&api)
        .header("cookie", &cookie)
        .header("sec-fetch-site", "cross-site")
        .send()
        .await
        .expect("sent");
    assert_eq!(resp.status(), 200);

    // Bearer + cross-site → allowed (the header is not forgeable cross-site).
    let resp = client
        .post(&api)
        .bearer_auth(&token)
        .header("sec-fetch-site", "cross-site")
        .json(&account_body("a2"))
        .send()
        .await
        .expect("sent");
    assert!(resp.status().is_success(), "bearer cross-site: {}", resp.status());

    // Cookie + same-origin → allowed.
    let resp = client
        .post(&api)
        .header("cookie", &cookie)
        .header("sec-fetch-site", "same-origin")
        .json(&account_body("a3"))
        .send()
        .await
        .expect("sent");
    assert!(resp.status().is_success(), "same-origin: {}", resp.status());

    // No Sec-Fetch-Site: Origin matching Host → allowed…
    let host = base.strip_prefix("http://").expect("http base");
    let resp = client
        .post(&api)
        .header("cookie", &cookie)
        .header("origin", &base)
        .json(&account_body("a4"))
        .send()
        .await
        .expect("sent");
    assert!(
        resp.status().is_success(),
        "origin {} vs host {}: {}",
        base,
        host,
        resp.status()
    );

    // …a mismatching Origin → 403.
    let resp = client
        .post(&api)
        .header("cookie", &cookie)
        .header("origin", "https://evil.example.net")
        .json(&account_body("a5"))
        .send()
        .await
        .expect("sent");
    assert_eq!(resp.status(), 403);

    // No Sec-Fetch-Site, no Origin, no Referer (curl-style) → allowed.
    let resp = client
        .post(&api)
        .header("cookie", &cookie)
        .json(&account_body("a6"))
        .send()
        .await
        .expect("sent");
    assert!(resp.status().is_success(), "header-less client: {}", resp.status());
}

#[tokio::test]
async fn cross_site_logout_is_rejected_and_the_session_survives() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let base = spawn_server(tmp.path(), None).await;
    let client = reqwest::Client::new();
    let (cookie, _token) = login(&client, &base).await;

    // Forced-logout attempt: cross-site POST riding the cookie → 403 and NO
    // clearing Set-Cookie (the 403 must not be a forced-logout vector itself).
    let resp = client
        .post(format!("{}/auth/logout", base))
        .header("cookie", &cookie)
        .header("sec-fetch-site", "cross-site")
        .send()
        .await
        .expect("logout sent");
    assert_eq!(resp.status(), 403);
    assert!(
        resp.headers().get("set-cookie").is_none(),
        "the cross-site 403 must not clear the cookie"
    );

    // The session is still alive: the gate still accepts the cookie.
    let resp = client
        .get(format!("{}/api/accounts", base))
        .header("cookie", &cookie)
        .send()
        .await
        .expect("gated request sent");
    assert_eq!(resp.status(), 200, "the session must survive the rejected logout");

    // A legitimate (same-origin) logout still works.
    let resp = client
        .post(format!("{}/auth/logout", base))
        .header("cookie", &cookie)
        .header("sec-fetch-site", "same-origin")
        .send()
        .await
        .expect("logout sent");
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn off_disarms_the_check() {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let cookie_config =
        CookieConfig { cross_site_check: CrossSiteCheck::Off, ..CookieConfig::default() };
    let base = spawn_server(tmp.path(), Some(cookie_config)).await;
    let client = reqwest::Client::new();
    let (cookie, _token) = login(&client, &base).await;

    let resp = client
        .post(format!("{}/api/accounts", base))
        .header("cookie", &cookie)
        .header("sec-fetch-site", "cross-site")
        .json(&account_body("b1"))
        .send()
        .await
        .expect("sent");
    assert!(resp.status().is_success(), "Off must allow cross-site: {}", resp.status());
}
