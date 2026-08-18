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
