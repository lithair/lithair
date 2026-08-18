//! Steps for `features/core/sessions.feature` — the browser session journey
//! on a real in-process `LithairServer` (`with_rbac_config` +
//! `with_models_require_session(true)`): login → cookie → gated route →
//! logout → 401.

use cucumber::{given, then, when, World as CucumberWorld};
use lithair_core::app::LithairServer;
use lithair_core::rbac::{RbacUser, ServerRbacConfig};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, lithair_core::DeclarativeModel)]
struct Account {
    #[http(expose)]
    id: String,
    #[http(expose)]
    name: String,
}

#[derive(Debug, Default, CucumberWorld)]
pub struct SessionsWorld {
    pub temp_dir: Option<tempfile::TempDir>,
    pub base_url: String,
    /// `name=value` pair of the cookie the login issued.
    pub cookie: Option<String>,
    pub last_status: Option<u16>,
    pub last_set_cookie: Option<String>,
}

impl SessionsWorld {
    fn client() -> reqwest::Client {
        // No cookie store on purpose: the steps forward the cookie by hand so
        // the scenario proves the cookie alone (no Bearer) satisfies the gate.
        reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("http client")
    }

    fn record(&mut self, resp: reqwest::Response) {
        self.last_status = Some(resp.status().as_u16());
        self.last_set_cookie =
            resp.headers().get("set-cookie").map(|v| v.to_str().expect("ascii").to_string());
    }

    fn cookie_header(&self) -> String {
        self.cookie.clone().expect("the login must have issued a cookie first")
    }
}

#[given(expr = "a server with RBAC auth routes and session-gated models")]
async fn given_server(world: &mut SessionsWorld) {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let port = portpicker::pick_unused_port().expect("free port");
    let base_url = format!("http://127.0.0.1:{port}");

    let rbac = ServerRbacConfig::new()
        .with_user(RbacUser::new("alice", "s3cret", "Admin"))
        .with_session_store(tmp.path().join("sessions").to_string_lossy().to_string());
    let builder = LithairServer::new()
        .with_host("127.0.0.1")
        .with_port(port)
        .with_model::<Account>(tmp.path().join("accounts").to_string_lossy(), "/api/accounts")
        .with_rbac_config(rbac)
        .with_models_require_session(true);
    tokio::spawn(async move {
        if let Err(e) = builder.serve().await {
            eprintln!("sessions BDD server error: {e}");
        }
    });

    let client = SessionsWorld::client();
    let mut up = false;
    for _ in 0..50 {
        if let Ok(resp) = client.get(format!("{base_url}/health")).send().await {
            if resp.status().is_success() {
                up = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(up, "server did not come up on {base_url}");

    world.temp_dir = Some(tmp);
    world.base_url = base_url;
}

#[when(expr = "I POST valid credentials to \\/auth\\/login")]
async fn when_login(world: &mut SessionsWorld) {
    let resp = SessionsWorld::client()
        .post(format!("{}/auth/login", world.base_url))
        .json(&serde_json::json!({"username": "alice", "password": "s3cret"}))
        .send()
        .await
        .expect("login sent");
    world.record(resp);
    world.cookie = world
        .last_set_cookie
        .as_deref()
        .and_then(|c| c.split(';').next())
        .map(str::to_string);
}

#[when(expr = "I GET \\/api\\/accounts with the session cookie only")]
async fn when_get_gated_with_cookie(world: &mut SessionsWorld) {
    let resp = SessionsWorld::client()
        .get(format!("{}/api/accounts", world.base_url))
        .header("cookie", world.cookie_header())
        .send()
        .await
        .expect("gated request sent");
    world.record(resp);
}

#[when(expr = "I GET \\/api\\/accounts without credentials")]
async fn when_get_gated_anonymous(world: &mut SessionsWorld) {
    let resp = SessionsWorld::client()
        .get(format!("{}/api/accounts", world.base_url))
        .send()
        .await
        .expect("gated request sent");
    world.record(resp);
}

#[when(expr = "I POST \\/auth\\/logout with the session cookie only")]
async fn when_logout_with_cookie(world: &mut SessionsWorld) {
    let resp = SessionsWorld::client()
        .post(format!("{}/auth/logout", world.base_url))
        .header("cookie", world.cookie_header())
        .send()
        .await
        .expect("logout sent");
    world.record(resp);
}

#[then(expr = "the response status should be {int}")]
async fn then_status(world: &mut SessionsWorld, expected: u16) {
    assert_eq!(world.last_status, Some(expected), "unexpected status");
}

#[then(expr = "the response should set the {string} cookie")]
async fn then_sets_cookie(world: &mut SessionsWorld, name: String) {
    let set_cookie = world.last_set_cookie.as_deref().expect("no Set-Cookie header");
    let (pair, attrs) = set_cookie.split_once(';').unwrap_or((set_cookie, ""));
    let (cookie_name, value) = pair.split_once('=').expect("cookie pair");
    assert_eq!(cookie_name, name);
    assert!(!value.is_empty(), "the login cookie must carry the session token: {set_cookie}");
    assert!(
        !attrs.contains("Max-Age=0"),
        "the login cookie must not be a clear: {set_cookie}"
    );
}

#[then(expr = "the response should clear the {string} cookie")]
async fn then_clears_cookie(world: &mut SessionsWorld, name: String) {
    let set_cookie = world.last_set_cookie.as_deref().expect("no Set-Cookie header");
    assert!(
        set_cookie.starts_with(&format!("{name}=;")) && set_cookie.contains("Max-Age=0"),
        "expected an empty {name} cookie with Max-Age=0, got: {set_cookie}"
    );
}
