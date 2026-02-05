//! Secure cookie management for sessions

use super::SameSitePolicy;
use chrono::{DateTime, Utc};

/// Cookie configuration
#[derive(Debug, Clone)]
pub struct CookieConfig {
    /// Cookie name
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
}

impl Default for CookieConfig {
    fn default() -> Self {
        Self {
            name: "session_id".to_string(),
            domain: None,
            path: "/".to_string(),
            secure: true,    // HTTPS by default
            http_only: true, // XSS protection
            same_site: SameSitePolicy::Lax,
            max_age: Some(86400), // 24 hours
        }
    }
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

    /// Build a Set-Cookie header value
    #[allow(dead_code)]
    pub fn build_set_cookie(&self, session_id: &str, expires_at: Option<DateTime<Utc>>) -> String {
        let mut parts = vec![format!("{}={}", self.config.name, session_id)];

        // Domain
        if let Some(ref domain) = self.config.domain {
            parts.push(format!("Domain={}", domain));
        }

        // Path
        parts.push(format!("Path={}", self.config.path));

        // Max-Age
        if let Some(max_age) = self.config.max_age {
            parts.push(format!("Max-Age={}", max_age));
        }

        // Expires (if provided)
        if let Some(expires) = expires_at {
            parts.push(format!("Expires={}", expires.format("%a, %d %b %Y %H:%M:%S GMT")));
        }

        // Secure
        if self.config.secure {
            parts.push("Secure".to_string());
        }

        // HttpOnly
        if self.config.http_only {
            parts.push("HttpOnly".to_string());
        }

        // SameSite
        let same_site = match self.config.same_site {
            SameSitePolicy::Strict => "Strict",
            SameSitePolicy::Lax => "Lax",
            SameSitePolicy::None => "None",
        };
        parts.push(format!("SameSite={}", same_site));

        parts.join("; ")
    }

    /// Build a delete cookie header (Max-Age=0)
    #[allow(dead_code)]
    pub fn build_delete_cookie(&self) -> String {
        format!("{}=; Path={}; Max-Age=0", self.config.name, self.config.path)
    }

    /// Extract session ID from Cookie header
    pub fn extract_from_header(&self, cookie_header: &str) -> Option<String> {
        cookie_header.split(';').find_map(|cookie| {
            let cookie = cookie.trim();
            cookie.strip_prefix(&format!("{}=", self.config.name)).map(|value| value.to_string())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_set_cookie() {
        let config = CookieConfig {
            name: "session_id".to_string(),
            domain: Some("example.com".to_string()),
            path: "/".to_string(),
            secure: true,
            http_only: true,
            same_site: SameSitePolicy::Strict,
            max_age: Some(3600),
        };

        let cookie = SessionCookie::new(config);
        let set_cookie = cookie.build_set_cookie("abc123", None);

        assert!(set_cookie.contains("session_id=abc123"));
        assert!(set_cookie.contains("Domain=example.com"));
        assert!(set_cookie.contains("Path=/"));
        assert!(set_cookie.contains("Max-Age=3600"));
        assert!(set_cookie.contains("Secure"));
        assert!(set_cookie.contains("HttpOnly"));
        assert!(set_cookie.contains("SameSite=Strict"));
    }

    #[test]
    fn test_extract_from_header() {
        let config = CookieConfig::default();
        let cookie = SessionCookie::new(config);

        let header = "session_id=abc123; other=value";
        assert_eq!(cookie.extract_from_header(header), Some("abc123".to_string()));

        let header = "other=value; session_id=xyz789";
        assert_eq!(cookie.extract_from_header(header), Some("xyz789".to_string()));

        let header = "other=value";
        assert_eq!(cookie.extract_from_header(header), None);
    }

    #[test]
    fn test_delete_cookie() {
        let config = CookieConfig::default();
        let cookie = SessionCookie::new(config);

        let delete = cookie.build_delete_cookie();
        assert!(delete.contains("Max-Age=0"));
        assert!(delete.contains("session_id="));
    }
}
