//! OpenID Connect relying party for custom handlers (feature `oidc`).
//!
//! The provider authenticates; the application authorizes. A successful login
//! yields only a verified `(issuer, subject)` pair bound to a fresh server-side
//! session. Provider claims never become permissions, and provider tokens are
//! discarded after verification. See `docs/features/security/oidc.md`.
use crate::app::{response, RouteRequest, RouteResponse};
use crate::session::{Session, SessionStore};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, Utc};
use http::{header, Method, StatusCode};
use openidconnect::core::{
    CoreAuthenticationFlow, CoreClient, CoreJwsSigningAlgorithm, CoreProviderMetadata,
};
use openidconnect::url;
use openidconnect::{
    AsyncHttpClient, AuthorizationCode, ClaimsVerificationError, ClientId, ClientSecret, CsrfToken,
    EndpointMaybeSet, EndpointNotSet, EndpointSet, HttpRequest, HttpResponse, IssuerUrl, Nonce,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, SignatureVerificationError,
};
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, RwLock};

/// Callback path registered by `with_oidc`, appended to the public origin.
pub const CALLBACK_PATH: &str = "/auth/oidc/callback";
pub const LOGIN_PATH: &str = "/auth/oidc/login";
pub const LOGOUT_PATH: &str = "/auth/oidc/logout";
const ISSUER_KEY: &str = "lithair.oidc.issuer";
const SUBJECT_KEY: &str = "lithair.oidc.subject";
const MAX_BODY: usize = 1024 * 1024;
const MAX_RETURN_TO: usize = 2048;
const JWKS_REFRESH_INTERVAL: Duration = Duration::from_secs(60);

type Client = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

/// Operator configuration. Incomplete or inconsistent values fail `discover`.
#[derive(Debug, Clone)]
pub struct OidcConfig {
    issuer: String,
    client_id: String,
    // `ClientSecret`'s Debug output is redacted; it is never logged or returned.
    client_secret: Option<ClientSecret>,
    public_origin: String,
    scopes: Vec<String>,
    login_ttl: Duration,
    session_ttl: Duration,
    max_pending_logins: usize,
    http_timeout: Duration,
    insecure_loopback: bool,
}

impl OidcConfig {
    /// `public_origin` is the application's external origin, e.g.
    /// `https://app.example.com`; the callback is `{origin}/auth/oidc/callback`.
    pub fn new(
        issuer: impl Into<String>,
        client_id: impl Into<String>,
        public_origin: impl Into<String>,
    ) -> Self {
        Self {
            issuer: issuer.into(),
            client_id: client_id.into(),
            client_secret: None,
            public_origin: public_origin.into(),
            scopes: Vec::new(),
            login_ttl: Duration::from_secs(300),
            session_ttl: Duration::from_secs(8 * 3600),
            max_pending_logins: 10_000,
            http_timeout: Duration::from_secs(5),
            insecure_loopback: false,
        }
    }
    /// Confidential client secret. Prefer [`Self::with_client_secret_env`].
    pub fn with_client_secret(mut self, secret: impl Into<String>) -> Self {
        self.client_secret = Some(ClientSecret::new(secret.into()));
        self
    }
    /// Read the client secret from an environment variable at startup.
    pub fn with_client_secret_env(self, variable: &str) -> anyhow::Result<Self> {
        let secret = std::env::var(variable)
            .map_err(|_| anyhow::anyhow!("OIDC client secret variable {variable} is not set"))?;
        Ok(self.with_client_secret(secret))
    }
    /// Scopes requested in addition to `openid`.
    pub fn with_scopes(mut self, scopes: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.scopes = scopes.into_iter().map(Into::into).collect();
        self
    }
    /// How long a started login may take to complete (default 5 minutes).
    pub fn with_login_ttl(mut self, ttl: Duration) -> Self {
        self.login_ttl = ttl;
        self
    }
    /// Absolute session lifetime; never extended by activity (default 8 hours).
    pub fn with_session_ttl(mut self, ttl: Duration) -> Self {
        self.session_ttl = ttl;
        self
    }
    /// Bound on concurrently pending logins (default 10 000); more get 503.
    pub fn with_max_pending_logins(mut self, max: usize) -> Self {
        self.max_pending_logins = max;
        self
    }
    /// Timeout for each discovery, JWKS and token request (default 5 seconds).
    pub fn with_http_timeout(mut self, timeout: Duration) -> Self {
        self.http_timeout = timeout;
        self
    }
    /// **Tests only.** Accept `http://` for the issuer, provider endpoints and
    /// public origin when they are loopback addresses. Never enable it in a
    /// deployment: cookies lose `Secure` and the `__Host-` prefix.
    pub fn allow_insecure_loopback_for_tests(mut self) -> Self {
        self.insecure_loopback = true;
        self
    }

    fn check_url(&self, what: &str, value: &url::Url) -> anyhow::Result<()> {
        let loopback = matches!(value.host(), Some(url::Host::Ipv4(ip)) if ip.is_loopback())
            || matches!(value.host(), Some(url::Host::Ipv6(ip)) if ip.is_loopback())
            || value.host_str() == Some("localhost");
        match value.scheme() {
            "https" => Ok(()),
            "http" if self.insecure_loopback && loopback => Ok(()),
            _ => anyhow::bail!("OIDC {what} must use https (got {value})"),
        }
    }
}

/// The authenticated external identity. It proves who signed in with which
/// provider, nothing more: resolve the application principal and check its
/// current tenant membership inside the command that uses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedIdentity {
    pub issuer: String,
    pub subject: String,
    /// Absolute session expiry.
    pub expires_at: DateTime<Utc>,
}

/// Why a request's identity was refused rather than merely absent.
#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("cross-site request rejected")]
    CrossSite,
    #[error("ambiguous session cookie")]
    AmbiguousCookie,
    #[error("session store unavailable: {0}")]
    Store(anyhow::Error),
}

struct Attempt {
    verifier: PkceCodeVerifier,
    nonce: Nonce,
    binding: [u8; 32],
    return_to: String,
    started: Instant,
}

struct Inner {
    config: OidcConfig,
    /// The client and the time of the last key refresh (`None` before any).
    client: RwLock<(Client, Option<Instant>)>,
    issuer: IssuerUrl,
    redirect: RedirectUrl,
    http: Http,
    store: Arc<dyn SessionStore>,
    attempts: Mutex<HashMap<String, Attempt>>,
    secure: bool,
}

/// A discovered relying party. Clone it into handlers; clones share state.
/// Pending logins live in this process only: a restart or a second instance
/// behind a load balancer invalidates them (the user retries the login).
#[derive(Clone)]
pub struct Oidc {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Oidc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Oidc")
            .field("config", &self.inner.config)
            .finish_non_exhaustive()
    }
}

impl Oidc {
    /// Validate the configuration and fetch provider metadata and keys. Any
    /// problem fails here, before a route is served. `store` must be the same
    /// store given to `LithairServer::with_sessions`.
    pub async fn discover(
        config: OidcConfig,
        store: Arc<dyn SessionStore>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(!config.client_id.is_empty(), "OIDC client_id is required");
        anyhow::ensure!(config.client_secret.is_some(), "OIDC client secret is required");
        anyhow::ensure!(
            !config.login_ttl.is_zero() && config.login_ttl <= Duration::from_secs(3600),
            "OIDC login TTL must be within 1 hour"
        );
        // Browsers cap cookie lifetimes at 400 days; this also keeps expiry
        // arithmetic from overflowing at login time.
        anyhow::ensure!(
            !config.session_ttl.is_zero()
                && config.session_ttl <= Duration::from_secs(400 * 86_400),
            "OIDC session TTL must be within 400 days"
        );
        anyhow::ensure!(config.max_pending_logins > 0, "OIDC pending login bound must be positive");
        let origin = url::Url::parse(&config.public_origin)?;
        config.check_url("public origin", &origin)?;
        anyhow::ensure!(
            origin.path() == "/"
                && origin.query().is_none()
                && origin.fragment().is_none()
                && origin.username().is_empty()
                && origin.password().is_none(),
            "OIDC public origin must be scheme://host[:port] only"
        );
        let public_origin = origin.origin().ascii_serialization();
        let issuer = IssuerUrl::new(config.issuer.clone())?;
        config.check_url("issuer", issuer.url())?;
        let http = Http::new(&config)?;
        let redirect = RedirectUrl::new(format!("{public_origin}{CALLBACK_PATH}"))?;
        let client = discover_client(&config, &http, issuer.clone(), redirect.clone()).await?;
        let secure = origin.scheme() == "https";
        let config = OidcConfig { public_origin, ..config };
        Ok(Self {
            inner: Arc::new(Inner {
                config,
                client: RwLock::new((client, None)),
                issuer,
                redirect,
                http,
                store,
                attempts: Mutex::new(HashMap::new()),
                secure,
            }),
        })
    }

    /// The verified identity of this request, or `None` without a live login
    /// session. Only the dedicated session cookie is read: `Authorization`,
    /// identity headers and request bodies are ignored. Unsafe methods must
    /// carry this application's `Origin` (or `Sec-Fetch-Site: same-origin`).
    pub async fn identity(
        &self,
        req: &RouteRequest,
    ) -> Result<Option<VerifiedIdentity>, IdentityError> {
        self.identity_of(req).await
    }

    /// [`Self::identity`] as a ready HTTP rejection: 401 without a session,
    /// 403 for a cross-site request, 400 for an ambiguous cookie, 503 when the
    /// session store fails.
    pub async fn require(&self, req: &RouteRequest) -> Result<VerifiedIdentity, RouteResponse> {
        match self.identity_of(req).await {
            Ok(Some(identity)) => Ok(identity),
            Ok(None) => Err(error(StatusCode::UNAUTHORIZED, "authentication required")),
            Err(IdentityError::CrossSite) => {
                Err(error(StatusCode::FORBIDDEN, "cross-site request rejected"))
            }
            Err(IdentityError::AmbiguousCookie) => {
                Err(error(StatusCode::BAD_REQUEST, "ambiguous session cookie"))
            }
            Err(IdentityError::Store(_)) => {
                Err(error(StatusCode::SERVICE_UNAVAILABLE, "session store unavailable"))
            }
        }
    }

    async fn identity_of<B>(
        &self,
        req: &http::Request<B>,
    ) -> Result<Option<VerifiedIdentity>, IdentityError> {
        let id = match self.cookie(req, &self.session_cookie()) {
            Cookie::Missing => return Ok(None),
            Cookie::Ambiguous => return Err(IdentityError::AmbiguousCookie),
            Cookie::One(id) => id,
        };
        if !self.same_origin(req) {
            return Err(IdentityError::CrossSite);
        }
        let Some(session) = self.inner.store.get(&id).await.map_err(IdentityError::Store)? else {
            return Ok(None);
        };
        if session.is_expired() {
            let _ = self.inner.store.delete(&id).await;
            return Ok(None);
        }
        let (Some(issuer), Some(subject)) =
            (session.get::<String>(ISSUER_KEY), session.get::<String>(SUBJECT_KEY))
        else {
            // A session created by another mechanism is not an OIDC login.
            return Ok(None);
        };
        if issuer != self.inner.config.issuer {
            return Ok(None);
        }
        Ok(Some(VerifiedIdentity { issuer, subject, expires_at: session.expires_at }))
    }

    /// `GET /auth/oidc/login[?return_to=/path]`: start a code flow.
    pub(crate) async fn login(&self, req: RouteRequest) -> RouteResponse {
        let return_to = match query(&req, "return_to") {
            None => "/".to_string(),
            Some(path) if local_path(&path) => path,
            Some(_) => return error(StatusCode::BAD_REQUEST, "return_to must be a local path"),
        };
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let client = self.inner.client.read().await.0.clone();
        let mut request = client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                CsrfToken::new_random,
                Nonce::new_random,
            )
            .set_pkce_challenge(challenge);
        for scope in &self.inner.config.scopes {
            request = request.add_scope(Scope::new(scope.clone()));
        }
        let (url, state, nonce) = request.url();
        let binding = random_token();
        {
            let mut attempts = self.inner.attempts.lock().await;
            let ttl = self.inner.config.login_ttl;
            attempts.retain(|_, attempt| attempt.started.elapsed() < ttl);
            if attempts.len() >= self.inner.config.max_pending_logins {
                return error(StatusCode::SERVICE_UNAVAILABLE, "too many pending logins");
            }
            attempts.insert(
                state.secret().clone(),
                Attempt {
                    verifier,
                    nonce,
                    binding: Sha256::digest(binding.as_bytes()).into(),
                    return_to,
                    started: Instant::now(),
                },
            );
        }
        let mut resp = redirect(StatusCode::FOUND, url.as_str());
        let max_age = self.inner.config.login_ttl.as_secs().max(1);
        self.set_cookie(&mut resp, &self.binding_cookie(), &binding, Some(max_age));
        resp
    }

    /// `GET /auth/oidc/callback?code=..&state=..`: finish a code flow.
    pub(crate) async fn callback(&self, req: RouteRequest) -> RouteResponse {
        let mut resp = self.finish(&req).await.unwrap_or_else(|e| e);
        // The binding cookie is single-use whatever the outcome.
        self.set_cookie(&mut resp, &self.binding_cookie(), "", Some(0));
        resp
    }

    async fn finish(&self, req: &RouteRequest) -> Result<RouteResponse, RouteResponse> {
        let state = query(req, "state")
            .ok_or_else(|| error(StatusCode::BAD_REQUEST, "missing login state"))?;
        // Removing the attempt is the single-use point: a replayed or
        // concurrent callback finds nothing.
        let attempt = self
            .inner
            .attempts
            .lock()
            .await
            .remove(&state)
            .ok_or_else(|| error(StatusCode::BAD_REQUEST, "unknown, used or expired login"))?;
        if attempt.started.elapsed() >= self.inner.config.login_ttl {
            return Err(error(StatusCode::BAD_REQUEST, "unknown, used or expired login"));
        }
        let binding = match self.cookie(req, &self.binding_cookie()) {
            Cookie::One(value) => Sha256::digest(value.as_bytes()),
            _ => return Err(error(StatusCode::BAD_REQUEST, "login started in another browser")),
        };
        if binding.as_slice() != attempt.binding {
            return Err(error(StatusCode::BAD_REQUEST, "login started in another browser"));
        }
        if query(req, "error").is_some() {
            return Err(error(StatusCode::UNAUTHORIZED, "the provider denied the login"));
        }
        let code = query(req, "code")
            .ok_or_else(|| error(StatusCode::BAD_REQUEST, "missing authorization code"))?;
        let subject = self.exchange(code, attempt.verifier, &attempt.nonce).await?;

        // Fresh session id on every login; an earlier OIDC session is ended.
        if let Cookie::One(previous) = self.cookie(req, &self.session_cookie()) {
            let _ = self.inner.store.delete(&previous).await;
        }
        let id = random_token();
        let ttl = chrono::Duration::from_std(self.inner.config.session_ttl)
            .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "invalid session TTL"))?;
        let mut session = Session::new(id.clone(), Utc::now() + ttl);
        session
            .set(ISSUER_KEY, &self.inner.config.issuer)
            .and_then(|_| session.set(SUBJECT_KEY, &subject))
            .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "session encoding failed"))?;
        self.inner
            .store
            .set(session)
            .await
            .map_err(|_| error(StatusCode::SERVICE_UNAVAILABLE, "session store unavailable"))?;
        let mut resp = redirect(StatusCode::SEE_OTHER, &attempt.return_to);
        let max_age = self.inner.config.session_ttl.as_secs().max(1);
        self.set_cookie(&mut resp, &self.session_cookie(), &id, Some(max_age));
        Ok(resp)
    }

    async fn exchange(
        &self,
        code: String,
        verifier: PkceCodeVerifier,
        nonce: &Nonce,
    ) -> Result<String, RouteResponse> {
        let client = self.inner.client.read().await.0.clone();
        let tokens = client
            .exchange_code(AuthorizationCode::new(code))
            .map_err(|_| error(StatusCode::BAD_GATEWAY, "provider has no token endpoint"))?
            .set_pkce_verifier(verifier)
            .request_async(&self.inner.http)
            .await
            .map_err(|e| {
                log::warn!("OIDC token exchange failed: {e}");
                error(StatusCode::BAD_GATEWAY, "token exchange failed")
            })?;
        let id_token = openidconnect::TokenResponse::id_token(&tokens)
            .ok_or_else(|| error(StatusCode::BAD_GATEWAY, "provider returned no ID token"))?;
        let verify = |client: &Client| {
            id_token
                .claims(&client.id_token_verifier().set_allowed_algs(ALLOWED_ALGS.to_vec()), nonce)
                .map(|claims| claims.subject().to_string())
        };
        match verify(&client) {
            Ok(subject) => Ok(subject),
            // Provider key rotation: refetch keys (rate-limited), verify once more.
            Err(ClaimsVerificationError::SignatureVerification(
                SignatureVerificationError::NoMatchingKey,
            )) => {
                let client = self.refresh().await;
                verify(&client).map_err(rejected)
            }
            Err(e) => Err(rejected(e)),
        }
    }

    async fn refresh(&self) -> Client {
        let mut current = self.inner.client.write().await;
        if current.1.is_none_or(|last| last.elapsed() >= JWKS_REFRESH_INTERVAL) {
            let inner = &self.inner;
            match discover_client(
                &inner.config,
                &inner.http,
                inner.issuer.clone(),
                inner.redirect.clone(),
            )
            .await
            {
                Ok(client) => *current = (client, Some(Instant::now())),
                Err(e) => {
                    current.1 = Some(Instant::now());
                    log::warn!("OIDC key refresh failed: {e}");
                }
            }
        }
        current.0.clone()
    }

    /// `POST /auth/oidc/logout`: end the session server-side, then clear it.
    pub(crate) async fn logout(&self, req: RouteRequest) -> RouteResponse {
        let id = match self.cookie(&req, &self.session_cookie()) {
            Cookie::Missing => None,
            Cookie::Ambiguous => {
                return error(StatusCode::BAD_REQUEST, "ambiguous session cookie");
            }
            Cookie::One(id) => Some(id),
        };
        if !self.same_origin(&req) {
            return error(StatusCode::FORBIDDEN, "cross-site request rejected");
        }
        if let Some(id) = id {
            if self.inner.store.delete(&id).await.is_err() {
                return error(StatusCode::SERVICE_UNAVAILABLE, "session store unavailable");
            }
        }
        let mut resp = response::json(StatusCode::NO_CONTENT, "");
        self.set_cookie(&mut resp, &self.session_cookie(), "", Some(0));
        resp
    }

    /// Unsafe methods must come from this application's origin.
    fn same_origin<B>(&self, req: &http::Request<B>) -> bool {
        if matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS) {
            return true;
        }
        let header = |name: &str| req.headers().get(name).and_then(|v| v.to_str().ok());
        match header("origin") {
            Some(origin) => origin.eq_ignore_ascii_case(&self.inner.config.public_origin),
            None => header("sec-fetch-site") == Some("same-origin"),
        }
    }

    fn cookie<B>(&self, req: &http::Request<B>, name: &str) -> Cookie {
        let mut found = req
            .headers()
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(';'))
            .filter_map(|pair| pair.trim().split_once('='))
            .filter(|(key, _)| *key == name)
            .map(|(_, value)| value.trim().to_string());
        match (found.next(), found.next()) {
            (None, _) => Cookie::Missing,
            (Some(value), None) if !value.is_empty() => Cookie::One(value),
            _ => Cookie::Ambiguous,
        }
    }

    fn session_cookie(&self) -> String {
        self.cookie_name("lithair-oidc")
    }
    fn binding_cookie(&self) -> String {
        self.cookie_name("lithair-oidc-login")
    }
    fn cookie_name(&self, base: &str) -> String {
        if self.inner.secure {
            format!("__Host-{base}")
        } else {
            base.to_string()
        }
    }

    fn set_cookie(&self, resp: &mut RouteResponse, name: &str, value: &str, max_age: Option<u64>) {
        let mut cookie = format!("{name}={value}; Path=/; HttpOnly; SameSite=Lax");
        if let Some(max_age) = max_age {
            cookie.push_str(&format!("; Max-Age={max_age}"));
        }
        if self.inner.secure {
            cookie.push_str("; Secure");
        }
        if let Ok(value) = cookie.parse() {
            resp.headers_mut().append(header::SET_COOKIE, value);
        }
    }
}

const ALLOWED_ALGS: &[CoreJwsSigningAlgorithm] = &[
    CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256,
    CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha384,
    CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha512,
    CoreJwsSigningAlgorithm::RsaSsaPssSha256,
    CoreJwsSigningAlgorithm::RsaSsaPssSha384,
    CoreJwsSigningAlgorithm::RsaSsaPssSha512,
    CoreJwsSigningAlgorithm::EcdsaP256Sha256,
    CoreJwsSigningAlgorithm::EcdsaP384Sha384,
    CoreJwsSigningAlgorithm::EdDsa,
];

enum Cookie {
    Missing,
    One(String),
    Ambiguous,
}

async fn discover_client(
    config: &OidcConfig,
    http: &Http,
    issuer: IssuerUrl,
    redirect: RedirectUrl,
) -> anyhow::Result<Client> {
    // Discovery checks that the document's issuer equals the configured one.
    let metadata = CoreProviderMetadata::discover_async(issuer, http)
        .await
        .map_err(|e| anyhow::anyhow!("OIDC discovery failed: {e}"))?;
    config.check_url("authorization endpoint", metadata.authorization_endpoint().url())?;
    let token = metadata
        .token_endpoint()
        .ok_or_else(|| anyhow::anyhow!("OIDC provider has no token endpoint"))?;
    config.check_url("token endpoint", token.url())?;
    config.check_url("JWKS URI", metadata.jwks_uri().url())?;
    Ok(CoreClient::from_provider_metadata(
        metadata,
        ClientId::new(config.client_id.clone()),
        config.client_secret.clone(),
    )
    .set_redirect_uri(redirect))
}

fn rejected(e: ClaimsVerificationError) -> RouteResponse {
    log::warn!("OIDC ID token rejected: {e}");
    error(StatusCode::UNAUTHORIZED, "invalid identity token")
}

fn random_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// A same-application path: one leading slash, no scheme-relative `//` or `\`.
fn local_path(path: &str) -> bool {
    path.starts_with('/')
        && !path.starts_with("//")
        && !path.contains('\\')
        && path.len() <= MAX_RETURN_TO
        && path.bytes().all(|b| b.is_ascii_graphic())
}

fn query<B>(req: &http::Request<B>, name: &str) -> Option<String> {
    let query = req.uri().query()?;
    let mut values = url::form_urlencoded::parse(query.as_bytes())
        .filter(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned());
    // A repeated parameter is ambiguous; treat it as absent.
    match (values.next(), values.next()) {
        (Some(value), None) => Some(value),
        _ => None,
    }
}

fn error(status: StatusCode, message: &str) -> RouteResponse {
    let mut resp = response::json_value(status, &serde_json::json!({ "error": message }));
    no_store(&mut resp);
    resp
}

fn redirect(status: StatusCode, location: &str) -> RouteResponse {
    let mut resp = response::json(status, "");
    if let Ok(value) = location.parse() {
        resp.headers_mut().insert(header::LOCATION, value);
    }
    no_store(&mut resp);
    resp
}

fn no_store(resp: &mut RouteResponse) {
    resp.headers_mut()
        .insert(header::CACHE_CONTROL, http::HeaderValue::from_static("no-store"));
}

/// Provider HTTP: TLS-validated, no redirects, bounded time and body size.
struct Http {
    client: reqwest::Client,
    config: OidcConfig,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct HttpError(String);

impl Http {
    fn new(config: &OidcConfig) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(config.http_timeout)
            .connect_timeout(config.http_timeout)
            .build()?;
        Ok(Self { client, config: config.clone() })
    }
}

impl<'c> AsyncHttpClient<'c> for Http {
    type Error = HttpError;
    type Future = Pin<Box<dyn Future<Output = Result<HttpResponse, HttpError>> + Send + 'c>>;

    fn call(&'c self, request: HttpRequest) -> Self::Future {
        Box::pin(async move {
            let url = url::Url::parse(&request.uri().to_string())
                .map_err(|e| HttpError(e.to_string()))?;
            self.config
                .check_url("provider URL", &url)
                .map_err(|e| HttpError(e.to_string()))?;
            let request =
                reqwest::Request::try_from(request).map_err(|e| HttpError(e.to_string()))?;
            let mut resp =
                self.client.execute(request).await.map_err(|e| HttpError(e.to_string()))?;
            if resp.status().is_redirection() {
                return Err(HttpError("provider redirects are not followed".into()));
            }
            let mut builder = http::Response::builder().status(resp.status());
            for (name, value) in resp.headers() {
                builder = builder.header(name, value);
            }
            let mut body = Vec::new();
            while let Some(chunk) = resp.chunk().await.map_err(|e| HttpError(e.to_string()))? {
                if body.len() + chunk.len() > MAX_BODY {
                    return Err(HttpError("provider response exceeds 1 MiB".into()));
                }
                body.extend_from_slice(&chunk);
            }
            builder.body(body).map_err(|e| HttpError(e.to_string()))
        })
    }
}
