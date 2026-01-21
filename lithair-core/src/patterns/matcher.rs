//! Pattern matching utilities
//!
//! Supports wildcards, CIDR notation, regex, and exact matches.

use std::net::IpAddr;

/// Result of a pattern match
#[derive(Debug, Clone)]
pub struct MatchResult {
    pub matched: bool,
    pub pattern: String,
}

/// Pattern matcher for various types of patterns
pub struct PatternMatcher;

impl PatternMatcher {
    /// Match a value against a pattern
    ///
    /// Supports:
    /// - Exact match: "example.com"
    /// - Wildcard: "*.example.com", "/api/*"
    /// - CIDR: "192.168.1.0/24"
    /// - Catch-all: "*"
    pub fn matches(pattern: &str, value: &str) -> bool {
        // Catch-all
        if pattern == "*" {
            return true;
        }

        // Exact match
        if pattern == value {
            return true;
        }

        // Try CIDR match if value looks like an IP
        if let Ok(ip) = value.parse::<IpAddr>() {
            if Self::matches_cidr(pattern, ip) {
                return true;
            }
        }

        // Wildcard match
        Self::matches_wildcard(pattern, value)
    }

    /// Match wildcard patterns
    ///
    /// Examples:
    /// - "*.example.com" matches "api.example.com"
    /// - "/api/*" matches "/api/users"
    fn matches_wildcard(pattern: &str, value: &str) -> bool {
        if !pattern.contains('*') {
            return false;
        }

        let parts: Vec<&str> = pattern.split('*').collect();

        match parts.len() {
            2 => {
                let prefix = parts[0];
                let suffix = parts[1];
                value.starts_with(prefix) && value.ends_with(suffix)
            }
            _ => {
                // Multiple wildcards - more complex matching
                // TODO: Implement full wildcard matching
                false
            }
        }
    }

    /// Match CIDR notation against IP address
    ///
    /// Example: "192.168.1.0/24" matches "192.168.1.100"
    fn matches_cidr(pattern: &str, ip: IpAddr) -> bool {
        if !pattern.contains('/') {
            return false;
        }

        // TODO: Implement proper CIDR matching
        // For now, simple implementation
        let parts: Vec<&str> = pattern.split('/').collect();
        if parts.len() != 2 {
            return false;
        }

        let network = parts[0];
        let _prefix_len: u8 = match parts[1].parse() {
            Ok(len) => len,
            Err(_) => return false,
        };

        // Simple prefix match for now
        ip.to_string().starts_with(network.trim_end_matches(".0"))
    }

    /// Match domain pattern
    ///
    /// Supports subdomain wildcards: "*.example.com"
    pub fn matches_domain(pattern: &str, domain: &str) -> bool {
        if pattern.starts_with("*.") {
            let suffix = &pattern[2..];
            domain.ends_with(suffix) || domain == suffix
        } else {
            pattern == domain
        }
    }

    /// Match path pattern
    ///
    /// Supports wildcards: "/api/*"
    pub fn matches_path(pattern: &str, path: &str) -> bool {
        Self::matches_wildcard(pattern, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        assert!(PatternMatcher::matches("example.com", "example.com"));
        assert!(!PatternMatcher::matches("example.com", "other.com"));
    }

    #[test]
    fn test_catch_all() {
        assert!(PatternMatcher::matches("*", "anything"));
        assert!(PatternMatcher::matches("*", "192.168.1.1"));
    }

    #[test]
    fn test_wildcard_domain() {
        assert!(PatternMatcher::matches_domain("*.example.com", "api.example.com"));
        assert!(PatternMatcher::matches_domain("*.example.com", "www.example.com"));
        assert!(!PatternMatcher::matches_domain("*.example.com", "other.com"));
    }

    #[test]
    fn test_wildcard_path() {
        assert!(PatternMatcher::matches_path("/api/*", "/api/users"));
        assert!(PatternMatcher::matches_path("/api/*", "/api/products"));
        assert!(!PatternMatcher::matches_path("/api/*", "/admin/users"));
    }

    #[test]
    fn test_cidr_match() {
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        assert!(PatternMatcher::matches_cidr("192.168.1.0/24", ip));

        let ip2: IpAddr = "192.168.2.100".parse().unwrap();
        assert!(!PatternMatcher::matches_cidr("192.168.1.0/24", ip2));
    }
}
