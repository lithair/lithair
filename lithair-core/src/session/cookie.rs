//! Secure cookie management for sessions
//!
//! [`CookieConfig`] is the single authority for the session cookie: the RBAC
//! login/logout emit it through [`SessionCookie`], and every extractor (the
//! model gate, the route guards, `/auth/validate`, `SessionMiddleware`) reads
//! the cookie name from the same struct — so the emitted and expected names
//! can never drift again (issue #219).

use super::{SameSitePolicy, SESSION_COOKIE_NAME};
use crate::config::SessionsConfig;
use chrono::{DateTime, Utc};
use http::Request;
use std::sync::Arc;

/// Cookie configuration
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CookieConfig {
    /// Emit and read the session cookie at all (default: `true`).
    ///
    /// `false` (`[sessions] cookie_enabled = false` / `LT_SESSION_COOKIE_ENABLED=false`)
    /// is Bearer-only mode: the RBAC login answers with the token in the JSON
    /// body only (no `Set-Cookie`), the logout emits no clear, and no
    /// extractor (gate, guards, `/auth/validate`, logout, `SessionMiddleware`)
    /// looks at the `Cookie:` header.
    pub enabled: bool,

    /// Cookie name (default: [`SESSION_COOKIE_NAME`])
    pub name: String,

    /// Cookie domain
    pub domain: Option<String>,

    /// Cookie path
    pub path: String,

    /// Secure flag (HTTPS only)
    pub secure: bool,

    /// HttpOnly flag (no JavaScript access)
    pub http_only: bool,

    /// SameSite policy
    pub same_site: SameSitePolicy,

    /// Max age in seconds
    pub max_age: Option<i64>,

    /// Emit the cookie under the `__Host-` prefix (`__Host-<name>`).
    ///
    /// Browsers then refuse the cookie unless it is `Secure`, has `Path=/`
    /// and no `Domain` — which shuts the sub-domain shadowing / slot-sharing
    /// attacks. Lithair forces those three attributes when this is on and
    /// [`CookieConfig::validate`] rejects an explicit `domain`. Opt-in in 1.x.
    pub host_prefix: bool,

    /// Reject cross-site state-changing requests that authenticate with the
    /// session cookie (CSRF defense, issue #225). See [`CrossSiteCheck`].
    pub cross_site_check: CrossSiteCheck,
}

/// Cross-site request policy for cookie-authenticated unsafe methods
/// (`POST`/`PUT`/`PATCH`/`DELETE`), issue #225.
///
/// A cookie rides along on cross-site requests, so every state-changing
/// endpoint that accepts it is CSRF-relevant. Under `Enforce` (the default)
/// such a request is rejected with `403` when `Sec-Fetch-Site: cross-site`
/// is present — falling back to an `Origin`/`Referer` vs `Host` comparison
/// when the header is absent; a request carrying none of those headers is
/// allowed (curl, native clients). Bearer-authenticated requests are never
/// checked: an attacker cannot forge the `Authorization` header cross-site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CrossSiteCheck {
    /// Reject cookie-authenticated cross-site mutations with `403` (default).
    #[default]
    Enforce,
    /// Skip the check — for setups that legitimately POST cross-site
    /// (separate front domain) until they configure CORS properly.
    Off,
}

impl Default for CookieConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            name: SESSION_COOKIE_NAME.to_string(),
            domain: None,
            path: "/".to_string(),
            secure: true,    // HTTPS by default
            http_only: true, // XSS protection
            same_site: SameSitePolicy::Lax,
            max_age: Some(86400), // 24 hours
            host_prefix: false,
            cross_site_check: CrossSiteCheck::Enforce,
        }
    }
}

impl CookieConfig {
    /// The name the cookie is emitted and read under: `__Host-<name>` when
    /// [`host_prefix`](Self::host_prefix) is on, `<name>` otherwise.
    pub fn effective_name(&self) -> String {
        if self.host_prefix {
            format!("__Host-{}", self.name)
        } else {
            self.name.clone()
        }
    }

    /// Reject combinations a browser would silently drop.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.name.is_empty() {
            anyhow::bail!("session cookie: the name must not be empty");
        }
        if self.host_prefix && self.domain.is_some() {
            anyhow::bail!(
                "session cookie: `host_prefix` (__Host-{}) forbids a Domain attribute — \
                 drop `domain` or turn `host_prefix` off",
                self.name
            );
        }
        Ok(())
    }
}

/// `[sessions]` config / `LT_SESSION_COOKIE_*` env vars → cookie attributes.
impl From<&SessionsConfig> for CookieConfig {
    fn from(sessions: &SessionsConfig) -> Self {
        Self {
            enabled: sessions.cookie_enabled,
            secure: sessions.cookie_secure,
            http_only: sessions.cookie_httponly,
            same_site: match sessions.cookie_samesite.as_str() {
                "Strict" => SameSitePolicy::Strict,
                "None" => SameSitePolicy::None,
                _ => SameSitePolicy::Lax,
            },
            cross_site_check: match sessions.cross_site_check.as_str() {
                "Off" => CrossSiteCheck::Off,
                _ => CrossSiteCheck::Enforce,
            },
            ..Self::default()
        }
    }
}

/// The cookie configuration in force for a request.
///
/// `LithairServer` inserts the effective `Arc<CookieConfig>` (TOML/env
/// defaults, overridden by `with_session_cookie`) into every request's
/// extensions at dispatch; requests built outside a server (tests, direct
/// handler calls) fall back to [`CookieConfig::default`].
pub fn effective_cookie_config<B>(req: &Request<B>) -> Arc<CookieConfig> {
    req.extensions().get::<Arc<CookieConfig>>().cloned().unwrap_or_default()
}

/// Cross-site check for a cookie-authenticated state-changing request
/// (issue #225). Returns `true` when the request must be rejected with 403.
///
/// The CALLER establishes that the credential is cookie-borne (a Bearer
/// request is not CSRF-forgeable and is never passed here — see
/// `crate::http::declarative::cookie_auth_cross_site_blocked`). This function
/// then applies the fetch-metadata-first policy:
///
/// - safe method (`GET`/`HEAD`/`OPTIONS`/...) or `cross_site_check: Off`
///   → allow;
/// - `Sec-Fetch-Site: same-origin` / `same-site` / `none` → allow,
///   `cross-site` → block;
/// - header absent (older browser, curl, native app): compare the `Origin`
///   header's host (and port, when it names one) against `Host`; absent that,
///   the `Referer`'s host; mismatch → block;
/// - none of the three headers present → allow (non-browser clients).
pub(crate) fn cross_site_request_blocked<B>(req: &Request<B>, config: &CookieConfig) -> bool {
    use http::Method;
    if config.cross_site_check == CrossSiteCheck::Off {
        return false;
    }
    if !matches!(*req.method(), Method::POST | Method::PUT | Method::PATCH | Method::DELETE) {
        return false;
    }

    let header = |name: &str| req.headers().get(name).and_then(|v| v.to_str().ok());

    let (checked, origin) = if let Some(site) = header("sec-fetch-site") {
        (site.eq_ignore_ascii_case("cross-site"), format!("Sec-Fetch-Site: {site}"))
    } else {
        // Fallback: Origin first, else Referer, against the Host header.
        // `authority_of` also swallows `Origin: null` (opaque origin, e.g. a
        // sandboxed iframe): "null" never matches a real Host → block.
        let host = header("host");
        let claimed = header("origin").or_else(|| header("referer"));
        match (claimed, host) {
            (Some(claimed), Some(host)) => (
                !authority_matches_host(authority_of(claimed), host),
                format!("origin: {claimed}"),
            ),
            _ => (false, String::new()),
        }
    };

    if checked {
        log::warn!(
            "cross-site request rejected: {} {} ({})",
            req.method(),
            req.uri().path(),
            origin
        );
    }
    checked
}

/// `host[:port]` part of an `Origin`/`Referer` value: scheme stripped, cut at
/// the first `/`. `Origin: null` comes back as `"null"`.
fn authority_of(value: &str) -> &str {
    let rest = value.split_once("://").map_or(value, |(_, rest)| rest);
    rest.split(['/', '?', '#']).next().unwrap_or(rest)
}

/// Case-insensitive host comparison: the host parts must match, and when the
/// claimed authority names a port it must match `Host`'s too (a claimed
/// authority without a port matches any — scheme-default ports are the
/// browser's business).
fn authority_matches_host(claimed: &str, host: &str) -> bool {
    let (claimed_host, claimed_port) = split_port(claimed);
    let (host_host, host_port) = split_port(host);
    claimed_host.eq_ignore_ascii_case(host_host)
        && claimed_port.is_none_or(|p| host_port == Some(p))
}

/// `host[:port]` → `(host, Some(port))` when the suffix after the last `:` is
/// numeric — which leaves bare IPv6 literals (`[::1]`) whole.
fn split_port(authority: &str) -> (&str, Option<&str>) {
    match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
            (host, Some(port))
        }
        _ => (authority, None),
    }
}

/// Value of the `name` cookie in a `Cookie:` header, if present and non-empty.
pub(crate) fn cookie_value(cookie_header: &str, name: &str) -> Option<String> {
    cookie_header.split(';').find_map(|cookie| {
        let value = cookie.trim().strip_prefix(name)?.strip_prefix('=')?.trim();
        (!value.is_empty()).then(|| value.to_string())
    })
}

/// Session cookie builder
pub struct SessionCookie {
    config: CookieConfig,
}

impl SessionCookie {
    /// Create a new session cookie builder
    pub fn new(config: CookieConfig) -> Self {
        Self { config }
    }

    /// The configuration this builder emits and reads with.
    pub fn config(&self) -> &CookieConfig {
        &self.config
    }

    /// Build a Set-Cookie header value
    pub fn build_set_cookie(&self, session_id: &str, expires_at: Option<DateTime<Utc>>) -> String {
        let mut parts = vec![format!("{}={}", self.config.effective_name(), session_id)];
        self.push_scope(&mut parts);
        if let Some(max_age) = self.config.max_age {
            parts.push(format!("Max-Age={}", max_age));
        }
        if let Some(expires) = expires_at {
            parts.push(format!("Expires={}", expires.format("%a, %d %b %Y %H:%M:%S GMT")));
        }
        self.push_flags(&mut parts);
        parts.join("; ")
    }

    /// Build a delete cookie header: the exact attributes of the Set-Cookie
    /// (Domain/Path/Secure/HttpOnly/SameSite) with `Max-Age=0` — a browser
    /// only drops a cookie when the scope matches the one it was set with.
    pub fn build_delete_cookie(&self) -> String {
        let mut parts = vec![format!("{}=", self.config.effective_name())];
        self.push_scope(&mut parts);
        parts.push("Max-Age=0".to_string());
        self.push_flags(&mut parts);
        parts.join("; ")
    }

    /// Domain + Path. `__Host-` forbids Domain and pins Path=/.
    fn push_scope(&self, parts: &mut Vec<String>) {
        if self.config.host_prefix {
            parts.push("Path=/".to_string());
            return;
        }
        if let Some(ref domain) = self.config.domain {
            parts.push(format!("Domain={}", domain));
        }
        parts.push(format!("Path={}", self.config.path));
    }

    /// Secure + HttpOnly + SameSite. `__Host-` forces Secure.
    fn push_flags(&self, parts: &mut Vec<String>) {
        if self.config.secure || self.config.host_prefix {
            parts.push("Secure".to_string());
        }
        if self.config.http_only {
            parts.push("HttpOnly".to_string());
        }
        parts.push(format!("SameSite={}", self.config.same_site.as_str()));
    }

    /// Extract session ID from Cookie header
    pub fn extract_from_header(&self, cookie_header: &str) -> Option<String> {
        cookie_value(cookie_header, &self.config.effective_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Everything but `Max-Age`, in emission order.
    fn attrs(cookie: &str) -> Vec<&str> {
        cookie.split("; ").skip(1).filter(|a| !a.starts_with("Max-Age=")).collect()
    }

    #[test]
    fn default_set_cookie_is_the_documented_shape() {
        let cookie = SessionCookie::new(CookieConfig::default());
        assert_eq!(
            cookie.build_set_cookie("abc123", None),
            "session_token=abc123; Path=/; Max-Age=86400; Secure; HttpOnly; SameSite=Lax"
        );
        assert_eq!(
            cookie.build_delete_cookie(),
            "session_token=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Lax"
        );
    }

    #[test]
    fn test_build_set_cookie() {
        let config = CookieConfig {
            enabled: true,
            name: "session_id".to_string(),
            domain: Some("example.com".to_string()),
            path: "/app".to_string(),
            secure: true,
            http_only: true,
            same_site: SameSitePolicy::Strict,
            max_age: Some(3600),
            host_prefix: false,
            cross_site_check: CrossSiteCheck::Enforce,
        };

        let cookie = SessionCookie::new(config);
        let set_cookie = cookie.build_set_cookie("abc123", None);
        assert_eq!(
            set_cookie,
            "session_id=abc123; Domain=example.com; Path=/app; Max-Age=3600; Secure; HttpOnly; SameSite=Strict"
        );
    }

    /// D12(b): the clear reproduces the set's scope and flags exactly.
    #[test]
    fn delete_cookie_repeats_the_set_cookie_attributes() {
        for config in [
            CookieConfig::default(),
            CookieConfig {
                domain: Some("example.com".to_string()),
                path: "/app".to_string(),
                secure: false,
                http_only: false,
                same_site: SameSitePolicy::None,
                ..CookieConfig::default()
            },
            CookieConfig { host_prefix: true, ..CookieConfig::default() },
        ] {
            let cookie = SessionCookie::new(config);
            let set = cookie.build_set_cookie("abc123", None);
            let clear = cookie.build_delete_cookie();
            assert_eq!(attrs(&set), attrs(&clear), "set={set} clear={clear}");
            assert!(clear.contains("; Max-Age=0"), "{clear}");
            assert_eq!(set.split('=').next(), clear.split('=').next());
        }
    }

    /// D12(c): `[sessions] cookie_secure=false` (or `LT_SESSION_COOKIE_SECURE`)
    /// drops `Secure` — the config is consumed.
    #[test]
    fn sessions_config_drives_the_cookie_flags() {
        let sessions = SessionsConfig {
            cookie_secure: false,
            cookie_httponly: false,
            cookie_samesite: "Strict".to_string(),
            ..SessionsConfig::default()
        };
        let cookie = SessionCookie::new(CookieConfig::from(&sessions));
        assert_eq!(
            cookie.build_set_cookie("t", None),
            "session_token=t; Path=/; Max-Age=86400; SameSite=Strict"
        );
        assert_eq!(CookieConfig::from(&SessionsConfig::default()), CookieConfig::default());
    }

    /// D12(d): `__Host-` prefix — name prefixed, Secure forced, Path pinned,
    /// no Domain, and an explicit Domain refused.
    #[test]
    fn host_prefix_forces_the_browser_invariants() {
        let config = CookieConfig {
            host_prefix: true,
            secure: false,
            path: "/app".to_string(),
            ..CookieConfig::default()
        };
        assert_eq!(config.effective_name(), "__Host-session_token");
        assert!(config.validate().is_ok());
        let cookie = SessionCookie::new(config);
        assert_eq!(
            cookie.build_set_cookie("t", None),
            "__Host-session_token=t; Path=/; Max-Age=86400; Secure; HttpOnly; SameSite=Lax"
        );
        assert_eq!(cookie.extract_from_header("__Host-session_token=t"), Some("t".to_string()));
        assert_eq!(cookie.extract_from_header("session_token=t"), None);

        let with_domain =
            CookieConfig { host_prefix: true, domain: Some("x.io".into()), ..Default::default() };
        let err = with_domain.validate().expect_err("Domain + __Host- must be refused");
        assert!(err.to_string().contains("__Host-session_token"), "{err}");
    }

    #[test]
    fn test_extract_from_header() {
        let cookie = SessionCookie::new(CookieConfig::default());

        let header = "session_token=abc123; other=value";
        assert_eq!(cookie.extract_from_header(header), Some("abc123".to_string()));

        let header = "other=value; session_token=xyz789";
        assert_eq!(cookie.extract_from_header(header), Some("xyz789".to_string()));

        assert_eq!(cookie.extract_from_header("other=value"), None);
        assert_eq!(cookie.extract_from_header("session_token="), None);
        assert_eq!(cookie.extract_from_header("session_tokenx=1"), None);
    }

    /// Issue #225 — the cross-site check semantics, case by case.
    #[test]
    fn cross_site_check_semantics() {
        let config = CookieConfig::default(); // Enforce
        let req = |method: &str, headers: &[(&str, &str)]| {
            let mut b = Request::builder().method(method).uri("/api/accounts");
            for (k, v) in headers {
                b = b.header(*k, *v);
            }
            b.body(()).unwrap()
        };

        // Safe methods are never blocked, even declared cross-site.
        for method in ["GET", "HEAD", "OPTIONS"] {
            let r = req(method, &[("sec-fetch-site", "cross-site")]);
            assert!(!cross_site_request_blocked(&r, &config), "{method}");
        }

        // Sec-Fetch-Site decides when present.
        for (site, blocked) in [
            ("same-origin", false),
            ("same-site", false),
            ("none", false),
            ("cross-site", true),
        ] {
            let r = req("POST", &[("sec-fetch-site", site)]);
            assert_eq!(cross_site_request_blocked(&r, &config), blocked, "{site}");
        }

        // Absent → Origin vs Host fallback (port compared when Origin has one).
        for (origin, host, blocked) in [
            ("https://app.example.com", "app.example.com", false),
            ("https://APP.example.COM", "app.example.com", false),
            ("http://app.example.com:8080", "app.example.com:8080", false),
            ("http://app.example.com", "app.example.com:8080", false), // no claimed port
            ("http://app.example.com:9999", "app.example.com:8080", true),
            ("https://evil.example.net", "app.example.com", true),
            ("null", "app.example.com", true), // opaque origin (sandboxed iframe)
            ("http://[::1]:8080", "[::1]:8080", false),
        ] {
            let r = req("POST", &[("origin", origin), ("host", host)]);
            assert_eq!(cross_site_request_blocked(&r, &config), blocked, "{origin} vs {host}");
        }

        // No Origin → Referer fallback, host part of the full URL.
        let r = req("POST", &[("referer", "https://evil.example.net/page"), ("host", "app.io")]);
        assert!(cross_site_request_blocked(&r, &config));
        let r = req("POST", &[("referer", "https://app.io/form?x=1"), ("host", "app.io")]);
        assert!(!cross_site_request_blocked(&r, &config));

        // No Sec-Fetch-Site, no Origin, no Referer → allow (curl, native apps).
        let r = req("DELETE", &[("host", "app.io")]);
        assert!(!cross_site_request_blocked(&r, &config));

        // Off disarms everything.
        let off = CookieConfig { cross_site_check: CrossSiteCheck::Off, ..Default::default() };
        let r = req("POST", &[("sec-fetch-site", "cross-site")]);
        assert!(!cross_site_request_blocked(&r, &off));
    }

    /// Issue #225 — the caller-side gate only fires for cookie-borne tokens.
    #[test]
    fn cross_site_gate_ignores_bearer_and_anonymous_requests() {
        use crate::http::declarative::cookie_auth_cross_site_blocked;
        let cross_site = ("sec-fetch-site", "cross-site");

        // Cookie credential + cross-site → blocked.
        let req = Request::builder()
            .method("POST")
            .header("cookie", "session_token=tok")
            .header(cross_site.0, cross_site.1)
            .body(())
            .unwrap();
        assert!(cookie_auth_cross_site_blocked(&req));

        // Bearer wins the extraction even when the cookie rides along → allowed.
        let req = Request::builder()
            .method("POST")
            .header("authorization", "Bearer tok")
            .header("cookie", "session_token=tok")
            .header(cross_site.0, cross_site.1)
            .body(())
            .unwrap();
        assert!(!cookie_auth_cross_site_blocked(&req));

        // No credential at all → nothing to protect.
        let req = Request::builder()
            .method("POST")
            .header(cross_site.0, cross_site.1)
            .body(())
            .unwrap();
        assert!(!cookie_auth_cross_site_blocked(&req));

        // Off via the request's effective config.
        let mut req = Request::builder()
            .method("POST")
            .header("cookie", "session_token=tok")
            .header(cross_site.0, cross_site.1)
            .body(())
            .unwrap();
        req.extensions_mut().insert(Arc::new(CookieConfig {
            cross_site_check: CrossSiteCheck::Off,
            ..Default::default()
        }));
        assert!(!cookie_auth_cross_site_blocked(&req));
    }

    /// The `[sessions] cross_site_check` string reaches the CookieConfig.
    #[test]
    fn sessions_config_drives_the_cross_site_check() {
        let sessions =
            SessionsConfig { cross_site_check: "Off".to_string(), ..SessionsConfig::default() };
        assert_eq!(CookieConfig::from(&sessions).cross_site_check, CrossSiteCheck::Off);
        assert_eq!(
            CookieConfig::from(&SessionsConfig::default()).cross_site_check,
            CrossSiteCheck::Enforce
        );
    }

    #[test]
    fn effective_cookie_config_reads_the_request_extension_or_defaults() {
        let bare = Request::builder().body(()).unwrap();
        assert_eq!(*effective_cookie_config(&bare), CookieConfig::default());

        let custom = Arc::new(CookieConfig { name: "sid".to_string(), ..Default::default() });
        let mut req = Request::builder().body(()).unwrap();
        req.extensions_mut().insert(Arc::clone(&custom));
        assert_eq!(effective_cookie_config(&req).name, "sid");
    }
}
