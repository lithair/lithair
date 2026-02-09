use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::{Method, Response, StatusCode};
use ipnet::IpNet;
use std::convert::Infallible;

#[derive(Clone, Debug)]
pub struct FirewallConfig {
    pub enabled: bool,
    pub allow: HashSet<String>,
    pub deny: HashSet<String>,
    pub global_qps: Option<u64>,
    pub per_ip_qps: Option<u64>,
    // Route scoping
    pub protected_prefixes: Vec<String>,
    pub exempt_prefixes: Vec<String>,
}

#[derive(Debug, Default, Clone, Copy)]
struct Window {
    second: u64,
    count: u64,
}

#[derive(Debug)]
pub struct Firewall {
    cfg: FirewallConfig,
    global_win: Mutex<Window>,
    per_ip_win: Mutex<HashMap<String, Window>>,
    compiled_allow: Vec<IpMatcher>,
    compiled_deny: Vec<IpMatcher>,
}

impl FirewallConfig {
    pub fn from_env() -> Self {
        let enabled = std::env::var("RS_FW_ENABLE")
            .ok()
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let allow = parse_csv_ipset(std::env::var("RS_FW_IP_ALLOW").unwrap_or_default());
        let deny = parse_csv_ipset(std::env::var("RS_FW_IP_DENY").unwrap_or_default());
        let global_qps =
            std::env::var("RS_FW_RATE_GLOBAL_QPS").ok().and_then(|v| v.parse::<u64>().ok());
        let per_ip_qps =
            std::env::var("RS_FW_RATE_PERIP_QPS").ok().and_then(|v| v.parse::<u64>().ok());
        Self {
            enabled,
            allow,
            deny,
            global_qps,
            per_ip_qps,
            protected_prefixes: vec![],
            exempt_prefixes: vec![],
        }
    }
}

#[derive(Debug, Clone)]
enum IpMatcher {
    Exact(IpAddr),
    Net(IpNet),
}

fn matches_ip(m: &IpMatcher, ip: &IpAddr) -> bool {
    match m {
        IpMatcher::Exact(x) => x == ip,
        IpMatcher::Net(n) => n.contains(ip),
    }
}

fn macro_nets(name: &str) -> Vec<IpNet> {
    // Supported macros (case-insensitive):
    // "private_v4", "private", "internal", "internal_private", "internal_private_ip",
    // "loopback", "link_local"
    let n = name.trim().to_ascii_lowercase();
    let mut out = Vec::new();
    match n.as_str() {
        "private_v4" => {
            out.push("10.0.0.0/8".parse().expect("valid CIDR literal"));
            out.push("172.16.0.0/12".parse().expect("valid CIDR literal"));
            out.push("192.168.0.0/16".parse().expect("valid CIDR literal"));
        }
        "private" | "internal" | "internal_private" | "internal_private_ip" => {
            out.push("10.0.0.0/8".parse().expect("valid CIDR literal"));
            out.push("172.16.0.0/12".parse().expect("valid CIDR literal"));
            out.push("192.168.0.0/16".parse().expect("valid CIDR literal"));
            out.push("fc00::/7".parse().expect("valid CIDR literal")); // IPv6 ULA
        }
        "loopback" => {
            out.push("127.0.0.0/8".parse().expect("valid CIDR literal"));
            out.push("::1/128".parse().expect("valid CIDR literal"));
        }
        "link_local" => {
            out.push("169.254.0.0/16".parse().expect("valid CIDR literal"));
            out.push("fe80::/10".parse().expect("valid CIDR literal"));
        }
        _ => {}
    }
    out
}

fn compile_ip_matchers(list: &HashSet<String>) -> Vec<IpMatcher> {
    let mut out = Vec::new();
    for raw in list.iter() {
        let s = raw.trim();
        // Try exact IP
        if let Ok(ip) = s.parse::<IpAddr>() {
            out.push(IpMatcher::Exact(ip));
            continue;
        }
        // Try CIDR
        if let Ok(net) = s.parse::<IpNet>() {
            out.push(IpMatcher::Net(net));
            continue;
        }
        // Try macro(s)
        for net in macro_nets(s) {
            out.push(IpMatcher::Net(net));
        }
    }
    out
}

/// Resolve the effective firewall configuration from multiple sources.
/// Precedence: builder > model > env
pub fn resolve_firewall_config(
    builder: Option<FirewallConfig>,
    model: Option<FirewallConfig>,
) -> FirewallConfig {
    if let Some(b) = builder {
        return b;
    }
    if let Some(m) = model {
        return m;
    }
    FirewallConfig::from_env()
}

fn parse_csv_ipset(csv: String) -> HashSet<String> {
    csv.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

// Module-level response type aliases for Hyper 1.x
type RespBody = BoxBody<Bytes, Infallible>;
type Resp = Response<RespBody>;
type RespErr = Box<Resp>;

#[inline]
fn body_from<T: Into<Bytes>>(data: T) -> RespBody {
    Full::new(data.into()).boxed()
}

impl Firewall {
    pub fn new(cfg: FirewallConfig) -> Self {
        let compiled_allow = compile_ip_matchers(&cfg.allow);
        let compiled_deny = compile_ip_matchers(&cfg.deny);
        Self {
            cfg,
            global_win: Mutex::new(Window::default()),
            per_ip_win: Mutex::new(HashMap::new()),
            compiled_allow,
            compiled_deny,
        }
    }

    #[inline]
    fn now_sec() -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
    }

    fn ip_to_key(addr: Option<SocketAddr>) -> Option<String> {
        addr.map(|sa| match sa.ip() {
            IpAddr::V4(v4) => v4.to_string(),
            IpAddr::V6(v6) => v6.to_string(),
        })
    }

    fn json_error(status: StatusCode, code: &str, message: &str) -> Resp {
        let body = format!(r#"{{"error":"{}","message":"{}"}}"#, code, message);
        Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(body_from(body))
            .expect("valid HTTP response")
    }

    fn check_ip_lists(&self, ip: Option<&str>) -> Result<(), RespErr> {
        if let Some(ip_str) = ip {
            if let Ok(addr) = ip_str.parse::<IpAddr>() {
                // Deny takes priority
                if !self.compiled_deny.is_empty()
                    && self.compiled_deny.iter().any(|m| matches_ip(m, &addr))
                {
                    let resp = Self::json_error(
                        StatusCode::FORBIDDEN,
                        "forbidden",
                        &format!("IP {} is denied", ip_str),
                    );
                    return Err(Box::new(resp));
                }
                // Allow list: if configured, must match
                if !self.compiled_allow.is_empty()
                    && !self.compiled_allow.iter().any(|m| matches_ip(m, &addr))
                {
                    return Err(Box::new(Self::json_error(
                        StatusCode::FORBIDDEN,
                        "forbidden",
                        "IP not in allow list",
                    )));
                }
            } else {
                // If we cannot parse, be conservative: deny when allow list set, else pass
                if !self.compiled_allow.is_empty() {
                    return Err(Box::new(Self::json_error(
                        StatusCode::FORBIDDEN,
                        "forbidden",
                        "Unrecognized IP format",
                    )));
                }
            }
        } else {
            // No IP (e.g., UNIX socket or unknown); if allow list is present, deny by default
            if !self.compiled_allow.is_empty() {
                return Err(Box::new(Self::json_error(
                    StatusCode::FORBIDDEN,
                    "forbidden",
                    "Unknown IP not allowed",
                )));
            }
        }
        Ok(())
    }

    fn check_global_qps(&self) -> Result<(), RespErr> {
        if let Some(limit) = self.cfg.global_qps {
            let now = Self::now_sec();
            let mut win = self.global_win.lock().expect("global rate limit lock poisoned");
            if win.second != now {
                win.second = now;
                win.count = 0;
            }
            if win.count >= limit {
                return Err(Box::new(Self::json_error(
                    StatusCode::TOO_MANY_REQUESTS,
                    "rate_limited",
                    "Global QPS limit exceeded",
                )));
            }
            win.count += 1;
        }
        Ok(())
    }

    fn check_per_ip_qps(&self, ip: Option<&str>) -> Result<(), RespErr> {
        if let (Some(limit), Some(ip)) = (self.cfg.per_ip_qps, ip) {
            let now = Self::now_sec();
            let mut map = self.per_ip_win.lock().expect("per-ip rate limit lock poisoned");
            let win = map.entry(ip.to_string()).or_insert(Window { second: now, count: 0 });
            if win.second != now {
                win.second = now;
                win.count = 0;
            }
            if win.count >= limit {
                return Err(Box::new(Self::json_error(
                    StatusCode::TOO_MANY_REQUESTS,
                    "ip_rate_limited",
                    "Per-IP QPS limit exceeded",
                )));
            }
            win.count += 1;
        }
        Ok(())
    }

    pub fn check(
        &self,
        remote: Option<SocketAddr>,
        method: &Method,
        path: &str,
    ) -> Result<(), RespErr> {
        if !self.cfg.enabled {
            return Ok(());
        }
        // Exempt OPTIONS preflight from rate limiting/filters
        if *method == Method::OPTIONS {
            return Ok(());
        }

        // Route scoping: bypass firewall entirely for exempt prefixes
        if self.cfg.exempt_prefixes.iter().any(|p| path.starts_with(p)) {
            return Ok(());
        }

        // If protected_prefixes is non-empty, apply firewall only to those prefixes
        if !self.cfg.protected_prefixes.is_empty()
            && !self.cfg.protected_prefixes.iter().any(|p| path.starts_with(p))
        {
            return Ok(());
        }
        let ip_key = Self::ip_to_key(remote);
        self.check_ip_lists(ip_key.as_deref())?;
        self.check_global_qps()?;
        self.check_per_ip_qps(ip_key.as_deref())?;
        // Future: per-endpoint rules can be added using `path` and `method`.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sa127() -> SocketAddr {
        "127.0.0.1:12345".parse().unwrap()
    }

    fn sa(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    #[test]
    fn exempt_prefix_bypasses_all_checks() {
        let cfg = FirewallConfig {
            enabled: true,
            allow: std::collections::HashSet::new(),
            deny: ["127.0.0.1"].into_iter().map(|s| s.to_string()).collect(),
            global_qps: Some(0),
            per_ip_qps: Some(0),
            protected_prefixes: vec!["/api/products".to_string()],
            exempt_prefixes: vec!["/status".to_string()],
        };
        let fw = Firewall::new(cfg);
        // Even though IP would be denied and limits are zero, exempt path should succeed
        assert!(fw.check(Some(sa127()), &Method::GET, "/status").is_ok());
    }

    #[test]
    fn protected_scoping_applies_only_to_listed_prefixes() {
        let cfg = FirewallConfig {
            enabled: true,
            // Allow list excludes 127.0.0.1 on purpose
            allow: ["127.0.0.2"].into_iter().map(|s| s.to_string()).collect(),
            deny: std::collections::HashSet::new(),
            global_qps: None,
            per_ip_qps: None,
            protected_prefixes: vec!["/api/products".to_string()],
            exempt_prefixes: vec![],
        };
        let fw = Firewall::new(cfg);
        // Request to a non-protected path should bypass checks (OK)
        assert!(fw.check(Some(sa127()), &Method::GET, "/api/other").is_ok());
        // Request to a protected path should enforce allow-list and thus be forbidden
        assert!(fw.check(Some(sa127()), &Method::GET, "/api/products").is_err());
    }

    #[test]
    fn global_rate_limit_zero_hits_immediately() {
        let cfg = FirewallConfig {
            enabled: true,
            allow: ["127.0.0.1"].into_iter().map(|s| s.to_string()).collect(),
            deny: std::collections::HashSet::new(),
            global_qps: Some(0),
            per_ip_qps: None,
            protected_prefixes: vec!["/api/products".to_string()],
            exempt_prefixes: vec![],
        };
        let fw = Firewall::new(cfg);
        let res = fw.check(Some(sa127()), &Method::GET, "/api/products");
        assert!(res.is_err());
        let resp = res.err().unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[test]
    fn precedence_builder_over_model() {
        let builder = FirewallConfig {
            enabled: true,
            allow: ["127.0.0.1"].into_iter().map(|s| s.to_string()).collect(),
            deny: std::collections::HashSet::new(),
            global_qps: Some(10),
            per_ip_qps: Some(5),
            protected_prefixes: vec!["/api/builder".to_string()],
            exempt_prefixes: vec!["/status".to_string()],
        };
        let model = FirewallConfig {
            enabled: false,
            allow: std::collections::HashSet::new(),
            deny: ["8.8.8.8"].into_iter().map(|s| s.to_string()).collect(),
            global_qps: None,
            per_ip_qps: None,
            protected_prefixes: vec!["/api/model".to_string()],
            exempt_prefixes: vec![],
        };
        let eff = resolve_firewall_config(Some(builder.clone()), Some(model));
        assert_eq!(eff.enabled, builder.enabled);
        assert_eq!(eff.global_qps, builder.global_qps);
        assert_eq!(eff.protected_prefixes, builder.protected_prefixes);
    }

    #[test]
    fn precedence_model_over_env() {
        // Set env to something different
        std::env::set_var("RS_FW_ENABLE", "0");
        std::env::set_var("RS_FW_IP_ALLOW", "");
        std::env::set_var("RS_FW_RATE_GLOBAL_QPS", "100");
        let model = FirewallConfig {
            enabled: true,
            allow: ["127.0.0.1"].into_iter().map(|s| s.to_string()).collect(),
            deny: std::collections::HashSet::new(),
            global_qps: Some(3),
            per_ip_qps: Some(2),
            protected_prefixes: vec!["/api/products".to_string()],
            exempt_prefixes: vec!["/health".to_string()],
        };
        let eff = resolve_firewall_config(None, Some(model.clone()));
        assert_eq!(eff.enabled, model.enabled);
        assert_eq!(eff.global_qps, model.global_qps);
        assert_eq!(eff.exempt_prefixes, model.exempt_prefixes);
    }

    #[test]
    fn env_only_is_selected_when_no_builder_or_model() {
        std::env::set_var("RS_FW_ENABLE", "1");
        std::env::set_var("RS_FW_IP_ALLOW", "127.0.0.1");
        std::env::set_var("RS_FW_IP_DENY", "");
        std::env::set_var("RS_FW_RATE_GLOBAL_QPS", "7");
        std::env::set_var("RS_FW_RATE_PERIP_QPS", "4");
        let eff = resolve_firewall_config(None, None);
        assert!(eff.enabled);
        assert_eq!(eff.global_qps, Some(7));
        assert_eq!(eff.per_ip_qps, Some(4));
        assert!(eff.allow.contains("127.0.0.1"));
    }

    #[test]
    fn allow_cidr_v4_matches_private_addr() {
        let cfg = FirewallConfig {
            enabled: true,
            allow: ["192.168.0.0/16".to_string()].into_iter().collect(),
            deny: HashSet::new(),
            global_qps: None,
            per_ip_qps: None,
            protected_prefixes: vec!["/perf".to_string()],
            exempt_prefixes: vec![],
        };
        let fw = Firewall::new(cfg);
        // Inside subnet
        assert!(fw.check(Some(sa("192.168.1.10:1111")), &Method::GET, "/perf").is_ok());
        // Outside subnet should be forbidden because allowlist present
        assert!(fw.check(Some(sa("10.0.0.5:1111")), &Method::GET, "/perf").is_err());
    }

    #[test]
    fn internal_macro_allows_private_v4_and_ipv6_ula() {
        let cfg = FirewallConfig {
            enabled: true,
            allow: ["internal".to_string()].into_iter().collect(),
            deny: HashSet::new(),
            global_qps: None,
            per_ip_qps: None,
            protected_prefixes: vec!["/metrics".to_string()],
            exempt_prefixes: vec![],
        };
        let fw = Firewall::new(cfg);
        // Private v4 should pass
        assert!(fw.check(Some(sa("10.12.1.2:80")), &Method::GET, "/metrics").is_ok());
        assert!(fw.check(Some(sa("172.16.0.1:80")), &Method::GET, "/metrics").is_ok());
        assert!(fw.check(Some(sa("192.168.10.5:80")), &Method::GET, "/metrics").is_ok());
        // Public v4 should be rejected
        assert!(fw.check(Some(sa("8.8.8.8:80")), &Method::GET, "/metrics").is_err());
        // IPv6 ULA fc00::/7 should pass
        assert!(fw.check(Some(sa("[fc00::1]:80")), &Method::GET, "/metrics").is_ok());
    }

    #[test]
    fn loopback_macro_allows_ipv4_and_ipv6_loopback() {
        let cfg = FirewallConfig {
            enabled: true,
            allow: ["loopback".to_string()].into_iter().collect(),
            deny: HashSet::new(),
            global_qps: None,
            per_ip_qps: None,
            protected_prefixes: vec!["/perf".to_string()],
            exempt_prefixes: vec![],
        };
        let fw = Firewall::new(cfg);
        assert!(fw.check(Some(sa("127.0.0.1:1")), &Method::GET, "/perf").is_ok());
        assert!(fw.check(Some(sa("127.1.2.3:1")), &Method::GET, "/perf").is_ok());
        assert!(fw.check(Some(sa("[::1]:1")), &Method::GET, "/perf").is_ok());
        assert!(fw.check(Some(sa("1.2.3.4:1")), &Method::GET, "/perf").is_err());
    }

    #[test]
    fn deny_priority_over_allow() {
        let allow: HashSet<String> = ["internal".to_string()].into_iter().collect();
        let deny: HashSet<String> = ["10.0.0.5".to_string()].into_iter().collect();
        let cfg = FirewallConfig {
            enabled: true,
            allow,
            deny,
            global_qps: None,
            per_ip_qps: None,
            protected_prefixes: vec!["/metrics".to_string()],
            exempt_prefixes: vec![],
        };
        let fw = Firewall::new(cfg);
        // 10.0.0.5 matches internal but is explicitly denied
        assert!(fw.check(Some(sa("10.0.0.5:80")), &Method::GET, "/metrics").is_err());
        // Another internal IP is allowed
        assert!(fw.check(Some(sa("10.0.0.6:80")), &Method::GET, "/metrics").is_ok());
    }

    #[test]
    fn private_v4_macro_allows_only_ipv4_private_ranges() {
        let cfg = FirewallConfig {
            enabled: true,
            allow: ["private_v4".to_string()].into_iter().collect(),
            deny: HashSet::new(),
            global_qps: None,
            per_ip_qps: None,
            protected_prefixes: vec!["/metrics".to_string()],
            exempt_prefixes: vec![],
        };
        let fw = Firewall::new(cfg);
        // Private IPv4 ranges should pass
        assert!(fw.check(Some(sa("10.1.2.3:80")), &Method::GET, "/metrics").is_ok());
        assert!(fw.check(Some(sa("172.16.5.6:80")), &Method::GET, "/metrics").is_ok());
        assert!(fw.check(Some(sa("192.168.1.1:80")), &Method::GET, "/metrics").is_ok());
        // Public IPv4 should be rejected
        assert!(fw.check(Some(sa("8.8.8.8:80")), &Method::GET, "/metrics").is_err());
        // IPv6 ULA should be rejected by private_v4 (not included)
        assert!(fw.check(Some(sa("[fc00::1]:80")), &Method::GET, "/metrics").is_err());
    }

    #[test]
    fn link_local_macro_allows_both_ipv4_and_ipv6_link_local() {
        let cfg = FirewallConfig {
            enabled: true,
            allow: ["link_local".to_string()].into_iter().collect(),
            deny: HashSet::new(),
            global_qps: None,
            per_ip_qps: None,
            protected_prefixes: vec!["/perf".to_string()],
            exempt_prefixes: vec![],
        };
        let fw = Firewall::new(cfg);
        // IPv4 link-local
        assert!(fw.check(Some(sa("169.254.1.2:80")), &Method::GET, "/perf").is_ok());
        // IPv6 link-local
        assert!(fw.check(Some(sa("[fe80::1]:80")), &Method::GET, "/perf").is_ok());
        // Non link-local should be rejected when allow-list is present
        assert!(fw.check(Some(sa("10.0.0.1:80")), &Method::GET, "/perf").is_err());
    }
}
