//! Host-header based routing primitive.
//!
//! This module provides a reusable primitive for branching on the HTTP
//! `Host:` header before any path-based routing is performed. It is the
//! building block that lets a single [`crate::app::LithairServer`] instance
//! host several sites (virtual hosts) directly, without a reverse proxy in
//! front.
//!
//! # Why a dedicated primitive?
//!
//! Before this module, `lithair-core` had only path-based routing
//! (`router.rs`). When a single process needs to serve e.g.
//! `lithair.net` and `arcker.org` on the same port with different
//! frontends, we need to branch on the `Host:` header first.
//!
//! The [`HostRouter`] type is intentionally small and generic: it maps a
//! normalized hostname to an arbitrary value `T`. Consumers decide what
//! that value is — a nested router, a frontend engine handle, a struct
//! describing which models are exposed on this vhost, etc.
//!
//! # Host normalization
//!
//! Host matching follows RFC 3986 / RFC 7230 semantics for authority
//! comparison:
//!
//! - Case-insensitive: `Arcker.ORG` matches `arcker.org`.
//! - Port-stripping: `arcker.org:443` matches `arcker.org`.
//! - Trailing dots and surrounding whitespace are trimmed.
//! - IPv6 literals in brackets (`[::1]:8080`) are preserved as `[::1]`.
//!
//! Lookup is `O(1)` via a [`HashMap`].
//!
//! # Fallback semantics
//!
//! A [`HostRouter`] may declare a *default* value via
//! [`HostRouter::with_default`]. When a request arrives for a host that
//! is not registered:
//!
//! - If a default is set, the default value is returned.
//! - Otherwise, [`HostRouter::lookup`] returns `None`. Callers are free
//!   to translate this into either `404 Not Found` or the more specific
//!   `421 Misdirected Request`. `lithair-core`'s integration layer uses
//!   `404` for backward compatibility with the existing behaviour when
//!   no route matches, unless
//!   [`crate::app::LithairServerBuilder::strict_host_routing`] is set,
//!   in which case it answers `421 Misdirected Request`. See
//!   [`NoMatch`] (returned by [`HostRouter::lookup_strict`]) for the
//!   sentinel that drives that decision.
//!
//! # Example
//!
//! ```
//! use lithair_core::http::host_router::HostRouter;
//!
//! // Map each hostname to a small label. In practice this would be a
//! // nested router or a frontend handle.
//! let router: HostRouter<&'static str> = HostRouter::new()
//!     .with_host("arcker.org", "arcker")
//!     .with_host("lithair.net", "lithair")
//!     .with_default("fallback");
//!
//! // Exact match
//! assert_eq!(router.lookup("arcker.org"), Some(&"arcker"));
//! // Case-insensitive
//! assert_eq!(router.lookup("ARCKER.ORG"), Some(&"arcker"));
//! // Port stripped
//! assert_eq!(router.lookup("arcker.org:443"), Some(&"arcker"));
//! // Unknown host -> default
//! assert_eq!(router.lookup("unknown.test"), Some(&"fallback"));
//! ```
//!
//! # Recommended pattern at the server level
//!
//! Use [`crate::app::LithairServerBuilder::with_vhost`] to declare vhosts
//! at the builder level — that helper composes on top of this primitive
//! and keeps existing path-only apps working unchanged:
//!
//! ```ignore
//! use lithair_core::LithairServer;
//!
//! LithairServer::new()
//!     .with_vhost("arcker.org",  |v| v.with_frontend_at("/", "sites/arcker.org"))
//!     .with_vhost("lithair.net", |v| v.with_frontend_at("/", "public"))
//!     .with_default_vhost(|v| v.with_frontend_at("/", "public"))
//!     .serve()
//!     .await?;
//! ```

use std::collections::HashMap;

/// A sentinel returned when no host matches and no default is configured.
///
/// Callers that want to respond with `421 Misdirected Request` instead
/// of `404 Not Found` can check for this explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoMatch;

/// A `O(1)` router that maps HTTP `Host:` header values to an arbitrary
/// value `T`.
///
/// Matching is case-insensitive and strips the `:port` suffix if present.
/// See the [module documentation](crate::http::host_router) for details.
#[derive(Debug, Clone)]
pub struct HostRouter<T> {
    /// Key is always stored normalized (lower-case, no port, no dot suffix).
    entries: HashMap<String, T>,
    default: Option<T>,
}

impl<T> HostRouter<T> {
    /// Create an empty router with no registered hosts and no default.
    pub fn new() -> Self {
        Self { entries: HashMap::new(), default: None }
    }

    /// Register a host-to-value mapping.
    ///
    /// `host` is normalized on insertion, so subsequent lookups with
    /// different casing or an explicit port still match.
    ///
    /// If the same normalized host is registered twice, the second call
    /// overwrites the first.
    pub fn with_host(mut self, host: impl AsRef<str>, value: T) -> Self {
        self.insert(host, value);
        self
    }

    /// Register a host-to-value mapping in place.
    ///
    /// Returns the previous value for this host if one existed.
    pub fn insert(&mut self, host: impl AsRef<str>, value: T) -> Option<T> {
        let key = normalize_host(host.as_ref());
        self.entries.insert(key, value)
    }

    /// Set the value returned when no host matches.
    pub fn with_default(mut self, value: T) -> Self {
        self.default = Some(value);
        self
    }

    /// Set the default in place.
    pub fn set_default(&mut self, value: T) {
        self.default = Some(value);
    }

    /// Returns the value registered for `host`, falling back to the
    /// default if one is configured.
    ///
    /// Returns `None` only when no entry matches and no default is set.
    pub fn lookup(&self, host: &str) -> Option<&T> {
        let key = normalize_host(host);
        if let Some(v) = self.entries.get(&key) {
            return Some(v);
        }
        self.default.as_ref()
    }

    /// Same as [`HostRouter::lookup`] but returns a [`NoMatch`] error
    /// when nothing matches, making it easier for callers to map to
    /// `421 Misdirected Request`.
    pub fn lookup_strict(&self, host: &str) -> Result<&T, NoMatch> {
        self.lookup(host).ok_or(NoMatch)
    }

    /// Number of registered hosts (not counting the default).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` if no hosts are registered. The default does not count.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `true` if any hosts or a default are configured.
    pub fn has_entries(&self) -> bool {
        !self.entries.is_empty() || self.default.is_some()
    }

    /// Iterator over registered `(normalized_host, value)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &T)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Reference to the default, if any.
    pub fn default_value(&self) -> Option<&T> {
        self.default.as_ref()
    }
}

impl<T> Default for HostRouter<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Normalize a host value coming from an HTTP `Host:` header or URI
/// authority for use as a [`HostRouter`] key.
///
/// The rules are:
///
/// 1. Trim surrounding ASCII whitespace.
/// 2. Strip a single trailing dot (FQDN form).
/// 3. Strip `:port` if present. IPv6 literals (`[::1]:8080`) are
///    handled so the bracketed address is preserved.
/// 4. Lower-case ASCII characters.
///
/// Non-ASCII hosts are left unchanged after the steps above. IDN hosts
/// should be passed in their ASCII (Punycode) form by the caller.
pub fn normalize_host(raw: &str) -> String {
    let trimmed = raw.trim();
    // Strip the port first, so we then see any trailing dot that was
    // attached to the hostname itself (e.g. `arcker.org.:443`).
    let without_port = strip_port(trimmed);
    let without_trailing_dot = without_port.strip_suffix('.').unwrap_or(without_port);
    without_trailing_dot.to_ascii_lowercase()
}

/// Strip the `:port` suffix from a host string, if any, while keeping
/// IPv6 bracketed literals intact.
fn strip_port(host: &str) -> &str {
    // IPv6 literal: `[::1]` or `[::1]:8080`.
    if let Some(rest) = host.strip_prefix('[') {
        // Find the closing bracket.
        if let Some(end) = rest.find(']') {
            // Keep the brackets so the literal round-trips cleanly.
            return &host[..end + 2];
        }
        // Malformed IPv6 literal — leave as-is.
        return host;
    }

    // Non-bracketed: the port (if any) starts at the LAST colon, but
    // only if there is exactly one. A bare IPv6 without brackets would
    // have several colons, and we don't strip anything in that case.
    match host.rfind(':') {
        Some(idx) if host.matches(':').count() == 1 => &host[..idx],
        _ => host,
    }
}

/// Extract the host from a `hyper::Request`. Returns `None` if neither
/// the URI authority nor the `Host:` header is present.
///
/// The returned value is *not* normalized — pass it to [`normalize_host`]
/// or to [`HostRouter::lookup`] (which normalizes internally).
pub fn host_from_request<B>(req: &hyper::Request<B>) -> Option<&str> {
    // HTTP/2 conveys the authority in the URI. Fall back to the Host
    // header for HTTP/1.x.
    if let Some(authority) = req.uri().authority() {
        return Some(authority.as_str());
    }
    req.headers().get(hyper::header::HOST).and_then(|v| v.to_str().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_router_returns_none() {
        let router: HostRouter<u8> = HostRouter::new();
        assert!(router.lookup("arcker.org").is_none());
        assert!(router.lookup_strict("arcker.org").is_err());
        assert!(router.is_empty());
        assert_eq!(router.len(), 0);
    }

    #[test]
    fn exact_host_match() {
        let r = HostRouter::new().with_host("arcker.org", "A").with_host("lithair.net", "L");
        assert_eq!(r.lookup("arcker.org"), Some(&"A"));
        assert_eq!(r.lookup("lithair.net"), Some(&"L"));
    }

    #[test]
    fn case_insensitive_match() {
        let r = HostRouter::new().with_host("arcker.org", "A");
        assert_eq!(r.lookup("ARCKER.ORG"), Some(&"A"));
        assert_eq!(r.lookup("Arcker.Org"), Some(&"A"));
    }

    #[test]
    fn insertion_normalizes_key() {
        // Mixed case on registration must also be normalized so that
        // duplicate keys collide.
        let mut r: HostRouter<&'static str> = HostRouter::new();
        assert!(r.insert("ARCKER.ORG", "first").is_none());
        assert_eq!(r.insert("arcker.org", "second"), Some("first"));
        assert_eq!(r.lookup("arcker.org"), Some(&"second"));
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn port_is_stripped() {
        let r = HostRouter::new().with_host("arcker.org", "A");
        assert_eq!(r.lookup("arcker.org:443"), Some(&"A"));
        assert_eq!(r.lookup("arcker.org:80"), Some(&"A"));
        assert_eq!(r.lookup("ARCKER.ORG:8080"), Some(&"A"));
    }

    #[test]
    fn trailing_dot_is_stripped() {
        let r = HostRouter::new().with_host("arcker.org", "A");
        assert_eq!(r.lookup("arcker.org."), Some(&"A"));
        assert_eq!(r.lookup("arcker.org.:443"), Some(&"A"));
    }

    #[test]
    fn surrounding_whitespace_is_stripped() {
        let r = HostRouter::new().with_host("arcker.org", "A");
        assert_eq!(r.lookup("  arcker.org  "), Some(&"A"));
    }

    #[test]
    fn default_fallback_used_when_no_match() {
        let r = HostRouter::new().with_host("arcker.org", "A").with_default("fallback");
        assert_eq!(r.lookup("unknown.test"), Some(&"fallback"));
        assert_eq!(r.lookup("arcker.org"), Some(&"A"));
    }

    #[test]
    fn default_not_used_when_host_matches() {
        let r = HostRouter::new().with_host("arcker.org", "A").with_default("fallback");
        assert_eq!(r.lookup("arcker.org"), Some(&"A"));
    }

    #[test]
    fn no_match_without_default_returns_none() {
        let r = HostRouter::new().with_host("arcker.org", "A");
        assert!(r.lookup("unknown.test").is_none());
        assert_eq!(r.lookup_strict("unknown.test"), Err(NoMatch));
    }

    #[test]
    fn ipv6_literal_is_preserved() {
        let r = HostRouter::new().with_host("[::1]", "localv6");
        assert_eq!(r.lookup("[::1]"), Some(&"localv6"));
        assert_eq!(r.lookup("[::1]:8080"), Some(&"localv6"));
    }

    #[test]
    fn bare_ipv6_without_brackets_is_not_split() {
        // `::1` has multiple colons; we don't try to guess where the
        // port would be, so it's used as-is (lowercased).
        let r = HostRouter::new().with_host("fe80::1", "v6");
        assert_eq!(r.lookup("fe80::1"), Some(&"v6"));
        // Unrelated host: still no match.
        assert!(r.lookup("other").is_none());
    }

    #[test]
    fn ipv4_with_port_is_split() {
        let r = HostRouter::new().with_host("127.0.0.1", "local");
        assert_eq!(r.lookup("127.0.0.1"), Some(&"local"));
        assert_eq!(r.lookup("127.0.0.1:18321"), Some(&"local"));
    }

    #[test]
    fn normalize_host_basic_cases() {
        assert_eq!(normalize_host("arcker.org"), "arcker.org");
        assert_eq!(normalize_host("ARCKER.ORG"), "arcker.org");
        assert_eq!(normalize_host("arcker.org:443"), "arcker.org");
        assert_eq!(normalize_host(" arcker.org. "), "arcker.org");
        assert_eq!(normalize_host("[::1]:8080"), "[::1]");
    }

    #[test]
    fn host_router_lookup_is_o1_shaped() {
        // Smoke-test: inserting many hosts and looking up the last still
        // works (and would be unacceptably slow if the impl were linear
        // under a real benchmark — here we just assert correctness).
        let mut r: HostRouter<usize> = HostRouter::new();
        for i in 0..10_000usize {
            r.insert(format!("host-{i}.test"), i);
        }
        assert_eq!(r.lookup("host-9999.test"), Some(&9_999));
        assert_eq!(r.lookup("HOST-9999.TEST:443"), Some(&9_999));
        assert!(r.lookup("missing.test").is_none());
    }

    #[test]
    fn host_from_request_prefers_uri_authority() {
        use http_body_util::Empty;
        use hyper::Request;

        let req = Request::builder()
            .uri("http://arcker.org:443/")
            .header(hyper::header::HOST, "lithair.net")
            .body(Empty::<bytes::Bytes>::new())
            .unwrap();
        // Authority takes precedence over Host: header (matches the
        // HTTP/2 "authority wins" rule from RFC 7540 §8.1.2.3).
        assert_eq!(host_from_request(&req), Some("arcker.org:443"));
    }

    #[test]
    fn host_from_request_falls_back_to_header() {
        use http_body_util::Empty;
        use hyper::Request;

        let req = Request::builder()
            .uri("/path-only")
            .header(hyper::header::HOST, "arcker.org")
            .body(Empty::<bytes::Bytes>::new())
            .unwrap();
        assert_eq!(host_from_request(&req), Some("arcker.org"));
    }

    #[test]
    fn host_from_request_returns_none_when_missing() {
        use http_body_util::Empty;
        use hyper::Request;

        let req = Request::builder().uri("/path-only").body(Empty::<bytes::Bytes>::new()).unwrap();
        assert!(host_from_request(&req).is_none());
    }
}
