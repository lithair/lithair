//! Signed local OIDC provider and a test application using a real session
//! store. Shared by the lithair-core tests and the Gherkin runner. The provider
//! and `allow_insecure_loopback_for_tests` are fixture-only.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use http_body_util::BodyExt;
use lithair_core::app::{response, LithairServer, RouteRequest};
use lithair_core::oidc::{Oidc, OidcConfig};
use lithair_core::session::{MemorySessionStore, SessionManager};
use openidconnect::core::{
    CoreIdToken, CoreIdTokenClaims, CoreJsonWebKeySet, CoreJwsSigningAlgorithm,
    CoreRsaPrivateSigningKey,
};
use openidconnect::{
    Audience, EmptyAdditionalClaims, IssuerUrl, JsonWebKeyId, Nonce, PrivateSigningKey,
    StandardClaims, SubjectIdentifier,
};
use reqwest::StatusCode;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::{net::TcpListener, sync::oneshot};

fn pem() -> &'static [String; 2] {
    static KEYS: OnceLock<[String; 2]> = OnceLock::new();
    KEYS.get_or_init(|| {
        use rsa::pkcs1::EncodeRsaPrivateKey;
        [0, 1].map(|_| {
            let key = rsa::RsaPrivateKey::new(&mut rand::rngs::OsRng, 2048).unwrap();
            key.to_pkcs1_pem(rsa::pkcs8::LineEnding::LF).unwrap().to_string()
        })
    })
}
fn signing_key(index: usize, kid: &str) -> CoreRsaPrivateSigningKey {
    CoreRsaPrivateSigningKey::from_pem(&pem()[index], Some(JsonWebKeyId::new(kid.into()))).unwrap()
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    Valid,
    ForeignSignature,
    WrongIssuer,
    WrongAudience,
    Expired,
    WrongNonce,
    TokenFailure,
    NoIdToken,
}

pub struct Grant {
    nonce: String,
    challenge: String,
    redirect_uri: String,
    subject: String,
}

pub struct ProviderState {
    issuer: String,
    pub mode: Mode,
    /// Index of the published and signing key, and its key id.
    pub key: (usize, &'static str),
    pub codes: HashMap<String, Grant>,
    /// Replaces the discovery response: status, extra headers, body.
    pub discovery: Option<(u16, Vec<(&'static str, String)>, String)>,
}

pub struct Server {
    url: String,
    stop: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<anyhow::Result<()>>,
}
impl Drop for Server {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        self.task.abort();
    }
}
pub async fn serve(
    builder: lithair_core::app::LithairServerBuilder,
    listener: TcpListener,
) -> Server {
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = builder.with_admin_panel(false).build().unwrap();
    let (stop, done) = oneshot::channel();
    let task = tokio::spawn(server.serve_with_listener(listener, async {
        let _ = done.await;
    }));
    let client = reqwest::Client::new();
    tokio::time::timeout(Duration::from_secs(10), async {
        while client.get(format!("{url}/health")).send().await.is_err() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("fixture server did not become ready");
    Server { url, stop: Some(stop), task }
}

pub struct Provider {
    pub server: Server,
    state: Arc<Mutex<ProviderState>>,
}
impl Provider {
    pub async fn start() -> Self {
        // RSA key generation takes seconds in debug builds: never inside a
        // request handler, where it would stall discovery past its timeout.
        tokio::task::spawn_blocking(pem).await.unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let issuer = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(ProviderState {
            issuer: issuer.clone(),
            mode: Mode::Valid,
            key: (0, "k1"),
            codes: HashMap::new(),
            discovery: None,
        }));
        let (discovery, jwks, token) = (state.clone(), state.clone(), state.clone());
        let builder = LithairServer::new()
            .with_route_async(http::Method::GET, "/.well-known/openid-configuration", move |_| {
                let state = discovery.clone();
                async move {
                    let state = state.lock().unwrap();
                    if let Some((status, headers, body)) = &state.discovery {
                        let mut resp = response::json(
                            http::StatusCode::from_u16(*status).unwrap(),
                            body.clone(),
                        );
                        for (name, value) in headers {
                            resp.headers_mut().insert(*name, value.parse().unwrap());
                        }
                        return Ok(resp);
                    }
                    let base = &state.issuer;
                    Ok(response::json_value(
                        http::StatusCode::OK,
                        &json!({
                            "issuer": base,
                            "authorization_endpoint": format!("{base}/authorize"),
                            "token_endpoint": format!("{base}/token"),
                            "jwks_uri": format!("{base}/jwks"),
                            "response_types_supported": ["code"],
                            "subject_types_supported": ["public"],
                            "id_token_signing_alg_values_supported": ["RS256"],
                            "token_endpoint_auth_methods_supported": ["client_secret_basic"],
                            "code_challenge_methods_supported": ["S256"],
                        }),
                    ))
                }
            })
            .with_route_async(http::Method::GET, "/jwks", move |_| {
                let state = jwks.clone();
                async move {
                    let (index, kid) = state.lock().unwrap().key;
                    let keys =
                        CoreJsonWebKeySet::new(vec![signing_key(index, kid).as_verification_key()]);
                    Ok(response::json_value(http::StatusCode::OK, &serde_json::to_value(keys)?))
                }
            })
            .with_route_async(http::Method::POST, "/token", move |req: RouteRequest| {
                let state = token.clone();
                async move { Ok(issue_tokens(req, state).await) }
            });
        let server = serve(builder, listener).await;
        Self { server, state }
    }
    pub fn url(&self) -> &str {
        &self.server.url
    }
    pub fn set(&self, change: impl FnOnce(&mut ProviderState)) {
        change(&mut self.state.lock().unwrap());
    }
    /// The authorization step a browser would perform at the provider.
    pub fn authorize(&self, login: &Login, subject: &str) -> String {
        let code = format!("code-{}", rand::random::<u64>());
        self.state.lock().unwrap().codes.insert(
            code.clone(),
            Grant {
                nonce: login.params["nonce"].clone(),
                challenge: login.params["code_challenge"].clone(),
                redirect_uri: login.params["redirect_uri"].clone(),
                subject: subject.into(),
            },
        );
        code
    }
}

async fn issue_tokens(
    req: RouteRequest,
    state: Arc<Mutex<ProviderState>>,
) -> lithair_core::app::RouteResponse {
    let basic =
        format!("Basic {}", base64::engine::general_purpose::STANDARD.encode("client:secret"));
    let authorized =
        req.headers().get("authorization").and_then(|v| v.to_str().ok()) == Some(&basic);
    let body = req.into_body().collect().await.unwrap().to_bytes();
    let form: HashMap<String, String> =
        openidconnect::url::form_urlencoded::parse(&body).into_owned().collect();
    let mut state = state.lock().unwrap();
    let grant = form.get("code").and_then(|code| state.codes.remove(code));
    let Some(grant) = grant.filter(|grant| {
        authorized
            && form.get("grant_type").map(String::as_str) == Some("authorization_code")
            && form.get("redirect_uri") == Some(&grant.redirect_uri)
            && form.get("code_verifier").is_some_and(|verifier| {
                URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())) == grant.challenge
            })
    }) else {
        return response::json_value(
            http::StatusCode::BAD_REQUEST,
            &json!({"error": "invalid_grant"}),
        );
    };
    let mode = state.mode;
    if mode == Mode::TokenFailure {
        return response::json(http::StatusCode::INTERNAL_SERVER_ERROR, "{}");
    }
    let now = chrono::Utc::now();
    let issuer =
        if mode == Mode::WrongIssuer { "http://127.0.0.1:1".into() } else { state.issuer.clone() };
    let audience = if mode == Mode::WrongAudience { "someone-else" } else { "client" };
    let expiry = if mode == Mode::Expired {
        now - chrono::Duration::minutes(5)
    } else {
        now + chrono::Duration::minutes(5)
    };
    let nonce = if mode == Mode::WrongNonce { "forged".into() } else { grant.nonce };
    let claims = CoreIdTokenClaims::new(
        IssuerUrl::new(issuer).unwrap(),
        vec![Audience::new(audience.into())],
        expiry,
        now - chrono::Duration::minutes(10),
        StandardClaims::new(SubjectIdentifier::new(grant.subject)),
        EmptyAdditionalClaims {},
    )
    .set_nonce(Some(Nonce::new(nonce)));
    // A foreign key reuses the published key id: only the signature differs.
    let (index, kid) = state.key;
    let signer = signing_key(if mode == Mode::ForeignSignature { 1 - index } else { index }, kid);
    let id_token = CoreIdToken::new(
        claims,
        &signer,
        CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256,
        None,
        None,
    )
    .unwrap();
    let mut body = json!({"access_token": "opaque", "token_type": "Bearer", "expires_in": 60});
    if mode != Mode::NoIdToken {
        body["id_token"] = serde_json::to_value(&id_token).unwrap();
    }
    response::json_value(http::StatusCode::OK, &body)
}

pub struct App {
    pub server: Server,
}
impl App {
    pub async fn start(provider: &Provider, store: Arc<MemorySessionStore>) -> Self {
        Self::with(provider, store, |config| config).await
    }
    pub async fn with(
        provider: &Provider,
        store: Arc<MemorySessionStore>,
        configure: impl FnOnce(OidcConfig) -> OidcConfig,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let config = OidcConfig::new(provider.url(), "client", origin)
            .with_client_secret("secret")
            .allow_insecure_loopback_for_tests();
        let oidc = Oidc::discover(configure(config), store.clone()).await.unwrap();
        let (whoami, command) = (oidc.clone(), oidc.clone());
        let builder = LithairServer::new()
            .with_sessions(SessionManager::from_arc(store))
            .with_oidc(oidc)
            .with_route_async(http::Method::GET, "/whoami", move |req: RouteRequest| {
                let oidc = whoami.clone();
                async move {
                    Ok(match oidc.require(&req).await {
                        Ok(id) => response::json_value(
                            http::StatusCode::OK,
                            &json!({"issuer": id.issuer, "subject": id.subject}),
                        ),
                        Err(rejection) => rejection,
                    })
                }
            })
            .with_route_async(http::Method::POST, "/api/command", move |req: RouteRequest| {
                let oidc = command.clone();
                async move {
                    let identity = match oidc.require(&req).await {
                        Ok(identity) => identity,
                        Err(rejection) => return Ok(rejection),
                    };
                    let body = req.into_body().collect().await?.to_bytes();
                    Ok(response::json_value(
                        http::StatusCode::OK,
                        &json!({"subject": identity.subject, "ignored": String::from_utf8_lossy(&body)}),
                    ))
                }
            });
        Self { server: serve(builder, listener).await }
    }
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.server.url)
    }
}

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}
pub fn set_cookie(resp: &reqwest::Response, name: &str) -> Option<String> {
    resp.headers().get_all("set-cookie").iter().find_map(|value| {
        let value = value.to_str().unwrap();
        value.strip_prefix(&format!("{name}="))?.split(';').next().map(str::to_string)
    })
}
pub fn set_cookie_line(resp: &reqwest::Response, name: &str) -> String {
    resp.headers()
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .find(|v| v.starts_with(&format!("{name}=")))
        .unwrap()
}

pub struct Login {
    pub params: HashMap<String, String>,
    pub binding: String,
}
pub async fn start_login(app: &App, return_to: Option<&str>) -> Login {
    let mut url = app.url("/auth/oidc/login");
    if let Some(path) = return_to {
        url = format!("{url}?return_to={}", urlencoding::encode(path));
    }
    let resp = client().get(url).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::FOUND);
    let location =
        openidconnect::url::Url::parse(resp.headers()["location"].to_str().unwrap()).unwrap();
    let params: HashMap<String, String> = location.query_pairs().into_owned().collect();
    assert_eq!(params["code_challenge_method"], "S256");
    assert_eq!(params["response_type"], "code");
    assert!(params["scope"].split(' ').any(|s| s == "openid"));
    let binding = set_cookie(&resp, "lithair-oidc-login").unwrap();
    Login { params, binding }
}
pub async fn callback(
    app: &App,
    state: &str,
    code: &str,
    binding: Option<&str>,
) -> reqwest::Response {
    let mut req = client().get(app.url(&format!("/auth/oidc/callback?state={state}&code={code}")));
    if let Some(binding) = binding {
        req = req.header("cookie", format!("lithair-oidc-login={binding}"));
    }
    req.send().await.unwrap()
}
/// Full login as `subject`; returns the session cookie value.
pub async fn login(app: &App, provider: &Provider, subject: &str) -> String {
    let login = start_login(app, None).await;
    let code = provider.authorize(&login, subject);
    let resp = callback(app, &login.params["state"], &code, Some(&login.binding)).await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER, "{:?}", resp.text().await);
    set_cookie(&resp, "lithair-oidc").unwrap()
}
pub async fn whoami(app: &App, cookie: &str) -> reqwest::Response {
    client()
        .get(app.url("/whoami"))
        .header("cookie", format!("lithair-oidc={cookie}"))
        .send()
        .await
        .unwrap()
}

/// Login, verified identity that no body/header/Bearer can change, logout revocation.
pub async fn login_yields_a_verified_identity_until_logout() {
    let provider = Provider::start().await;
    let app = App::start(&provider, Arc::new(MemorySessionStore::new())).await;
    let login = start_login(&app, Some("/orders?page=2")).await;
    let code = provider.authorize(&login, "alice");
    let resp = callback(&app, &login.params["state"], &code, Some(&login.binding)).await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    assert_eq!(resp.headers()["location"], "/orders?page=2");
    assert_eq!(resp.headers()["cache-control"], "no-store");
    let line = set_cookie_line(&resp, "lithair-oidc");
    assert!(line.contains("HttpOnly") && line.contains("SameSite=Lax") && line.contains("Path=/"));
    assert!(set_cookie_line(&resp, "lithair-oidc-login").contains("Max-Age=0"));
    let cookie = set_cookie(&resp, "lithair-oidc").unwrap();
    // Provider tokens never reach the browser.
    assert!(!format!("{:?}", resp.headers()).contains("opaque"));

    let me: Value = whoami(&app, &cookie).await.json().await.unwrap();
    assert_eq!(me, json!({"issuer": provider.url(), "subject": "alice"}));

    // The subject cannot be changed through the body, headers or a Bearer token.
    let resp = client()
        .post(app.url("/api/command"))
        .header("cookie", format!("lithair-oidc={cookie}"))
        .header("origin", &app.server.url)
        .header("x-forwarded-user", "mallory")
        .header("x-lithair-subject", "mallory")
        .header("authorization", "Bearer mallory")
        .json(&json!({"subject": "mallory", "issuer": "https://evil"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.json::<Value>().await.unwrap()["subject"], "alice");

    let logout = client()
        .post(app.url("/auth/oidc/logout"))
        .header("cookie", format!("lithair-oidc={cookie}"))
        .header("origin", &app.server.url)
        .send()
        .await
        .unwrap();
    assert_eq!(logout.status(), StatusCode::NO_CONTENT);
    assert!(set_cookie_line(&logout, "lithair-oidc").contains("Max-Age=0"));
    // Revoked server-side: replaying the old cookie does not work.
    assert_eq!(whoami(&app, &cookie).await.status(), StatusCode::UNAUTHORIZED);
}
