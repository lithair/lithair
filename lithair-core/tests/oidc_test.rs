//! OIDC relying party against a signed local provider and a real session store.
#![cfg(feature = "oidc")]
#[path = "support/oidc_fixture.rs"]
mod fixture;
use fixture::*;
use lithair_core::oidc::{Oidc, OidcConfig};
use lithair_core::session::{MemorySessionStore, Session, SessionStore};
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn login_yields_a_verified_identity_until_logout() {
    fixture::login_yields_a_verified_identity_until_logout().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn state_is_required_bound_to_the_browser_single_use_and_expiring() {
    let provider = Provider::start().await;
    let app = App::with(&provider, Arc::new(MemorySessionStore::new()), |c| {
        c.with_login_ttl(Duration::from_secs(1))
    })
    .await;
    let bad = |path: &str| client().get(app.url(path)).send();
    assert_eq!(
        bad("/auth/oidc/callback?code=x").await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        bad("/auth/oidc/callback?state=nope&code=x").await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    for target in ["//evil.example", "https://evil.example", "/\\evil", "relative"] {
        let url = format!("/auth/oidc/login?return_to={}", urlencoding::encode(target));
        assert_eq!(bad(&url).await.unwrap().status(), StatusCode::BAD_REQUEST, "{target}");
    }

    // Browser mismatch: no binding cookie, then another browser's cookie.
    for binding in [None, Some("another-browser")] {
        let login = start_login(&app, None).await;
        let code = provider.authorize(&login, "alice");
        let resp = callback(&app, &login.params["state"], &code, binding).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert!(set_cookie(&resp, "lithair-oidc").is_none());
        // The attempt was consumed: even the right browser cannot finish it.
        let retry = callback(&app, &login.params["state"], &code, Some(&login.binding)).await;
        assert_eq!(retry.status(), StatusCode::BAD_REQUEST);
    }

    // Replay after success.
    let login = start_login(&app, None).await;
    let code = provider.authorize(&login, "alice");
    let first = callback(&app, &login.params["state"], &code, Some(&login.binding)).await;
    assert_eq!(first.status(), StatusCode::SEE_OTHER);
    let replay = callback(&app, &login.params["state"], &code, Some(&login.binding)).await;
    assert_eq!(replay.status(), StatusCode::BAD_REQUEST);

    // Expiry.
    let login = start_login(&app, None).await;
    let code = provider.authorize(&login, "alice");
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let late = callback(&app, &login.params["state"], &code, Some(&login.binding)).await;
    assert_eq!(late.status(), StatusCode::BAD_REQUEST);
    assert!(set_cookie(&late, "lithair-oidc").is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_callbacks_issue_one_session() {
    let provider = Provider::start().await;
    let store = Arc::new(MemorySessionStore::new());
    let app = App::start(&provider, store.clone()).await;
    let login = start_login(&app, None).await;
    let code = provider.authorize(&login, "alice");
    let calls = (0..8).map(|_| callback(&app, &login.params["state"], &code, Some(&login.binding)));
    let results = futures::future::join_all(calls).await;
    let sessions: Vec<_> = results.iter().filter_map(|r| set_cookie(r, "lithair-oidc")).collect();
    assert_eq!(sessions.len(), 1);
    assert_eq!(results.iter().filter(|r| r.status() == StatusCode::SEE_OTHER).count(), 1);
    assert_eq!(store.count().await.unwrap(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn invalid_tokens_and_provider_failures_create_no_session() {
    let provider = Provider::start().await;
    let store = Arc::new(MemorySessionStore::new());
    let app = App::start(&provider, store.clone()).await;
    for (mode, status) in [
        (Mode::ForeignSignature, StatusCode::UNAUTHORIZED),
        (Mode::WrongIssuer, StatusCode::UNAUTHORIZED),
        (Mode::WrongAudience, StatusCode::UNAUTHORIZED),
        (Mode::Expired, StatusCode::UNAUTHORIZED),
        (Mode::WrongNonce, StatusCode::UNAUTHORIZED),
        (Mode::TokenFailure, StatusCode::BAD_GATEWAY),
        (Mode::NoIdToken, StatusCode::BAD_GATEWAY),
    ] {
        provider.set(|p| p.mode = mode);
        let login = start_login(&app, None).await;
        let code = provider.authorize(&login, "alice");
        let resp = callback(&app, &login.params["state"], &code, Some(&login.binding)).await;
        assert_eq!(resp.status(), status, "{mode:?}");
        assert!(set_cookie(&resp, "lithair-oidc").is_none(), "{mode:?}");
    }
    // A code the provider refuses (unknown code / PKCE mismatch) is a token failure.
    provider.set(|p| p.mode = Mode::Valid);
    let login = start_login(&app, None).await;
    let resp = callback(&app, &login.params["state"], "unknown-code", Some(&login.binding)).await;
    assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
    // The provider denying the login.
    let login = start_login(&app, None).await;
    let url = app.url(&format!(
        "/auth/oidc/callback?state={}&error=access_denied",
        login.params["state"]
    ));
    let resp = client()
        .get(url)
        .header("cookie", format!("lithair-oidc-login={}", login.binding))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(store.count().await.unwrap(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rotated_provider_keys_are_refetched() {
    let provider = Provider::start().await;
    let app = App::start(&provider, Arc::new(MemorySessionStore::new())).await;
    login(&app, &provider, "alice").await;
    provider.set(|p| p.key = (1, "k2"));
    let cookie = login(&app, &provider, "bob").await;
    assert_eq!(whoami(&app, &cookie).await.json::<Value>().await.unwrap()["subject"], "bob");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn discovery_and_configuration_fail_closed() {
    let provider = Provider::start().await;
    let store: Arc<dyn SessionStore> = Arc::new(MemorySessionStore::new());
    let config = || {
        OidcConfig::new(provider.url(), "client", "http://127.0.0.1:9")
            .with_client_secret("secret")
            .allow_insecure_loopback_for_tests()
    };
    Oidc::discover(config(), store.clone()).await.unwrap();
    let invalid = [
        OidcConfig::new(provider.url(), "client", "http://127.0.0.1:9")
            .with_client_secret("secret"),
        OidcConfig::new("http://idp.example", "client", "https://app.example")
            .with_client_secret("s")
            .allow_insecure_loopback_for_tests(),
        config()
            .with_client_secret("secret")
            .with_scopes(["openid"])
            .with_login_ttl(Duration::ZERO),
        config().with_session_ttl(Duration::from_secs(401 * 86_400)),
        OidcConfig::new(provider.url(), "client", "http://127.0.0.1:9")
            .allow_insecure_loopback_for_tests(),
        OidcConfig::new(provider.url(), "", "http://127.0.0.1:9")
            .with_client_secret("s")
            .allow_insecure_loopback_for_tests(),
        OidcConfig::new(provider.url(), "client", "http://127.0.0.1:9/app")
            .with_client_secret("s")
            .allow_insecure_loopback_for_tests(),
        OidcConfig::new(provider.url(), "client", "http://app.example")
            .with_client_secret("s")
            .allow_insecure_loopback_for_tests(),
    ];
    for config in invalid {
        assert!(Oidc::discover(config.clone(), store.clone()).await.is_err(), "{config:?}");
    }
    // The secret never appears in diagnostics.
    assert!(!format!("{:?}", config()).contains("secret\""));

    let big = "x".repeat(2 * 1024 * 1024);
    for discovery in [
        (200, vec![], json!({"issuer": "http://127.0.0.1:1"}).to_string()),
        (302, vec![("location", format!("{}/elsewhere", provider.url()))], String::new()),
        (200, vec![], big),
        (500, vec![], String::new()),
    ] {
        provider.set(|p| p.discovery = Some(discovery.clone()));
        assert!(Oidc::discover(config(), store.clone()).await.is_err(), "{}", discovery.0);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn forged_cookies_bearer_and_cross_site_requests_are_refused() {
    let provider = Provider::start().await;
    let store = Arc::new(MemorySessionStore::new());
    let app = App::start(&provider, store.clone()).await;
    let cookie = login(&app, &provider, "alice").await;
    let post = |cookie: String| {
        client().post(app.url("/api/command")).header("cookie", cookie).json(&json!({}))
    };
    // Unknown session id, a non-OIDC session, and a valid id sent as Bearer.
    assert_eq!(whoami(&app, "forged").await.status(), StatusCode::UNAUTHORIZED);
    let expires = chrono::Utc::now() + chrono::Duration::hours(1);
    store.set(Session::new("password-session".into(), expires)).await.unwrap();
    assert_eq!(whoami(&app, "password-session").await.status(), StatusCode::UNAUTHORIZED);
    let bearer = client().get(app.url("/whoami")).bearer_auth(&cookie).send().await.unwrap();
    assert_eq!(bearer.status(), StatusCode::UNAUTHORIZED);
    // Two session cookies are ambiguous, even when one is valid.
    let both = client()
        .get(app.url("/whoami"))
        .header("cookie", format!("lithair-oidc={cookie}; lithair-oidc=forged"))
        .send()
        .await
        .unwrap();
    assert_eq!(both.status(), StatusCode::BAD_REQUEST);

    let session = format!("lithair-oidc={cookie}");
    let cross = post(session.clone())
        .header("origin", "http://evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(cross.status(), StatusCode::FORBIDDEN);
    let anonymous = post(session.clone()).send().await.unwrap();
    assert_eq!(anonymous.status(), StatusCode::FORBIDDEN, "no Origin, no fetch metadata");
    let fetch = post(session.clone())
        .header("sec-fetch-site", "same-origin")
        .send()
        .await
        .unwrap();
    assert_eq!(fetch.status(), StatusCode::OK);
    let logout = client()
        .post(app.url("/auth/oidc/logout"))
        .header("cookie", &session)
        .header("origin", "http://evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(logout.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        whoami(&app, &cookie).await.status(),
        StatusCode::OK,
        "cross-site logout ignored"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sessions_expire_absolutely_and_do_not_survive_a_lost_store() {
    let provider = Provider::start().await;
    let app = App::with(&provider, Arc::new(MemorySessionStore::new()), |c| {
        c.with_session_ttl(Duration::from_secs(1))
    })
    .await;
    let cookie = login(&app, &provider, "alice").await;
    assert_eq!(whoami(&app, &cookie).await.status(), StatusCode::OK);
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(whoami(&app, &cookie).await.status(), StatusCode::UNAUTHORIZED);

    // Restart with a fresh in-memory store: reauthentication is required.
    let app = App::start(&provider, Arc::new(MemorySessionStore::new())).await;
    let cookie = login(&app, &provider, "alice").await;
    drop(app);
    let restarted = App::start(&provider, Arc::new(MemorySessionStore::new())).await;
    assert_eq!(whoami(&restarted, &cookie).await.status(), StatusCode::UNAUTHORIZED);
}
