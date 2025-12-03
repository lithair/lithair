# Lithair Admin Protection System

Lithair provides a comprehensive, zero-configuration admin protection system that combines **automatic endpoint generation** with **IP-based firewall protection**. This system provides secure administrative access with minimal setup.

## Overview

The admin protection system consists of three main components:

1. **Automatic Admin Endpoints** - Zero-config `/status`, `/health`, `/info` endpoints
2. **IP-Based Firewall** - Configurable IP allowlists with logging
3. **Generic Admin Handler** - Combines automatic + custom endpoints with unified protection

## Quick Start

### Basic Setup (No Protection)

```rust
use lithair_core::http::{ServerMetrics, AutoAdminConfig, handle_admin_with_custom};

// 1. Implement ServerMetrics for your server
impl ServerMetrics for MyServer {
    fn get_uptime_seconds(&self) -> i64 { /* implementation */ }
    fn get_server_mode(&self) -> &str { /* implementation */ }
    fn get_last_reload_at(&self) -> Option<String> { /* implementation */ }
    fn get_server_start_time(&self) -> String { /* implementation */ }
}

// 2. Configure automatic endpoints
let config = AutoAdminConfig::default(); // Enables /status, /health, /info

// 3. Use in your request handler
if let Some(response) = handle_admin_with_custom(
    &method, &path, &req, &server, &config, None, None
).await {
    return response; // Automatic endpoint handled
}
```

### Secure Setup (With Firewall)

```rust
use lithair_core::http::firewall::{Firewall, FirewallConfig};

// Create firewall with IP restrictions
let firewall_config = FirewallConfig {
    enabled: true,
    allow: ["127.0.0.1", "10.0.0.0/8", "192.168.1.100"].into(),
    deny: HashSet::new(),
    protected_prefixes: vec!["/admin".to_string()],
    // ... other config
};
let firewall = Some(Firewall::new(firewall_config));

// Use with firewall protection
if let Some(response) = handle_admin_with_custom(
    &method, &path, &req, &server, &config, firewall.as_ref(), custom_handler
).await {
    return response;
}
```

## Automatic Admin Endpoints

### Available Endpoints

| Endpoint | Method | Purpose | Example Response |
|----------|--------|---------|------------------|
| `/admin/status` | GET | Server status with metrics | Uptime, mode, reload info |
| `/admin/health` | GET | Health check | Simple OK/FAIL status |
| `/admin/info` | GET | Server information | Version, framework info |

### Configuration

```rust
use lithair_core::http::AutoAdminConfig;

let config = AutoAdminConfig {
    enable_status: true,        // Enable /admin/status
    enable_health: true,        // Enable /admin/health
    enable_info: true,          // Enable /admin/info
    admin_prefix: "/admin".to_string(), // Custom prefix
};
```

### Response Examples

#### Status Endpoint (`GET /admin/status`)

```json
{
  "status": "running",
  "mode": "hybrid",
  "uptime": "2h15m30s",
  "uptime_seconds": 8130,
  "server_start_time": "2025-09-25T10:00:00Z",
  "last_reload_at": "2025-09-25T12:00:00Z",
  "timestamp": "2025-09-25T12:15:30Z",
  "architecture": "clean_separation",
  "assets_count": 42
}
```

#### Health Endpoint (`GET /admin/health`)

```json
{
  "health": "ok",
  "status": "running",
  "mode": "production",
  "uptime_seconds": 8130,
  "timestamp": "2025-09-25T12:15:30Z",
  "checks": {
    "server": "pass",
    "uptime": "pass"
  }
}
```

#### Info Endpoint (`GET /admin/info`)

```json
{
  "name": "Lithair Server",
  "version": "0.1.0",
  "mode": "production",
  "server_start_time": "2025-09-25T10:00:00Z",
  "uptime": "2h15m30s",
  "framework": "lithair-core",
  "timestamp": "2025-09-25T12:15:30Z"
}
```

## ServerMetrics Trait

The `ServerMetrics` trait is the **core interface** that powers all automatic endpoints:

```rust
pub trait ServerMetrics {
    /// Server uptime in seconds (required)
    fn get_uptime_seconds(&self) -> i64;

    /// Server mode: "dev", "hybrid", "prod" (required)
    fn get_server_mode(&self) -> &str;

    /// Optional last reload timestamp in RFC3339 format
    fn get_last_reload_at(&self) -> Option<String>;

    /// Server start time in RFC3339 format (required)
    fn get_server_start_time(&self) -> String;

    /// Additional custom metrics (optional)
    fn get_additional_metrics(&self) -> serde_json::Value {
        serde_json::json!({})
    }
}
```

### Implementation Example

```rust
use chrono::{DateTime, Utc};

struct MyServer {
    start_time: DateTime<Utc>,
    last_reload: Option<DateTime<Utc>>,
    dev_mode: bool,
    hybrid_mode: bool,
}

impl ServerMetrics for MyServer {
    fn get_uptime_seconds(&self) -> i64 {
        Utc::now()
            .signed_duration_since(self.start_time)
            .num_seconds()
    }

    fn get_server_mode(&self) -> &str {
        if self.hybrid_mode {
            "hybrid"
        } else if self.dev_mode {
            "dev"
        } else {
            "prod"
        }
    }

    fn get_last_reload_at(&self) -> Option<String> {
        self.last_reload.as_ref().map(|dt| dt.to_rfc3339())
    }

    fn get_server_start_time(&self) -> String {
        self.start_time.to_rfc3339()
    }

    fn get_additional_metrics(&self) -> serde_json::Value {
        serde_json::json!({
            "architecture": "microservices",
            "custom_metric": 42,
            "feature_flags": {
                "feature_a": true,
                "feature_b": false
            }
        })
    }
}
```

## IP-Based Firewall Protection

### Firewall Configuration

```rust
use lithair_core::http::firewall::{Firewall, FirewallConfig};
use std::collections::HashSet;

let config = FirewallConfig {
    enabled: true,

    // IP allowlist (CIDR notation supported)
    allow: [
        "127.0.0.1",        // Localhost
        "10.0.0.0/8",       // Private network
        "192.168.1.100",    // Specific IP
        "203.0.113.0/24"    // Public subnet
    ].iter().map(|s| s.to_string()).collect(),

    // IP denylist (takes precedence over allow)
    deny: ["192.168.1.50"].iter().map(|s| s.to_string()).collect(),

    // Protected path prefixes
    protected_prefixes: vec![
        "/admin".to_string(),
        "/internal".to_string()
    ],

    // Exempt path prefixes (bypass firewall)
    exempt_prefixes: vec![
        "/admin/health".to_string()  // Health checks bypass firewall
    ],

    // Rate limiting (optional)
    global_qps: Some(100),     // 100 requests/sec globally
    per_ip_qps: Some(10),      // 10 requests/sec per IP
};

let firewall = Firewall::new(config);
```

### Security Levels

#### Level 1: Open Access (Development)

```rust
// No firewall - all admin endpoints are public
let firewall = None;
```

#### Level 2: Basic IP Filtering

```rust
let config = FirewallConfig {
    enabled: true,
    allow: ["127.0.0.1", "192.168.1.0/24"].into(),
    protected_prefixes: vec!["/admin".to_string()],
    ..Default::default()
};
```

#### Level 3: Strict Corporate Security

```rust
let config = FirewallConfig {
    enabled: true,
    allow: ["10.0.0.0/8"].into(),                    // Corporate network only
    deny: ["10.0.1.50", "10.0.1.51"].into(),       // Blocked specific IPs
    protected_prefixes: vec!["/admin".to_string()],
    exempt_prefixes: vec![],                         // No exemptions
    per_ip_qps: Some(5),                            // Rate limiting
    ..Default::default()
};
```

### Firewall Logging

The firewall automatically logs all access attempts:

```text
✅ Admin access allowed from IP: 192.168.1.100 for path: /admin/status
🚫 Admin access denied from IP: 203.0.113.50 for path: /admin/status
⚠️ Admin access denied: Could not determine client IP for path: /admin/reload
```

## Custom Admin Endpoints

### Adding Custom Endpoints

```rust
// Define your custom admin handler
async fn handle_custom_admin(
    method: &hyper::Method,
    path: &str,
    req: &hyper::Request<hyper::body::Incoming>,
) -> Option<hyper::Response<hyper::body::Boxed>> {
    match (method, path) {
        (&Method::GET, "/admin/custom") => {
            Some(hyper::Response::builder()
                .status(200)
                .header("Content-Type", "application/json")
                .body(body_from(r#"{"custom": "endpoint"}"#))
                .unwrap())
        }
        (&Method::POST, path) if path.ends_with("/restart") => {
            // Handle restart request
            Some(restart_server().await)
        }
        _ => None  // Not handled by custom logic
    }
}

// Use in your main handler
if let Some(response) = handle_admin_with_custom(
    &method, &path, &req, &server, &config, firewall.as_ref(),
    Some(handle_custom_admin)  // Pass custom handler
).await {
    return response;
}
```

### Request Flow

```text
Request → Is admin path? → Check firewall → Try automatic endpoints → Try custom handler → 404
```

## Production Deployment Patterns

### Docker Configuration

```dockerfile
# Dockerfile
ENV ADMIN_FIREWALL_ENABLED=true
ENV ADMIN_ALLOW_IPS="10.0.0.0/8,127.0.0.1"
EXPOSE 8080
```

```rust
// main.rs
let admin_firewall_enabled = env::var("ADMIN_FIREWALL_ENABLED")
    .unwrap_or("false".to_string())
    .parse::<bool>()
    .unwrap_or(false);

let admin_allow_ips = env::var("ADMIN_ALLOW_IPS")
    .unwrap_or("127.0.0.1".to_string());
```

### Kubernetes Deployment

```yaml
# deployment.yaml
apiVersion: apps/v1
kind: Deployment
spec:
  template:
    spec:
      containers:
      - name: lithair-app
        env:
        - name: ADMIN_FIREWALL_ENABLED
          value: "true"
        - name: ADMIN_ALLOW_IPS
          value: "10.244.0.0/16,127.0.0.1"  # Pod network
```

### Load Balancer Configuration

```nginx
# nginx.conf
upstream lithair_admin {
    server 127.0.0.1:8080;
}

server {
    listen 443 ssl;
    server_name admin.example.com;

    location /admin {
        allow 10.0.0.0/8;      # Corporate network
        allow 127.0.0.1;       # Localhost
        deny all;              # Deny everyone else

        proxy_pass http://lithair_admin;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    }
}
```

### Monitoring Integration

```rust
impl ServerMetrics for MyServer {
    fn get_additional_metrics(&self) -> serde_json::Value {
        serde_json::json!({
            "requests_per_second": self.get_current_rps(),
            "memory_usage_mb": self.get_memory_usage(),
            "active_connections": self.get_active_connections(),
            "error_rate": self.get_error_rate(),
            "database_connections": self.get_db_pool_status()
        })
    }
}
```

## Security Best Practices

### Network Security

1. **Always use HTTPS** in production
2. **Restrict IP ranges** to minimum necessary
3. **Use VPN or private networks** for admin access
4. **Consider SSH tunneling** for ultra-secure access

### Application Security

1. **Enable firewall** in production environments
2. **Use strong admin prefixes** (not just `/admin`)
3. **Implement rate limiting** for admin endpoints
4. **Log all admin access** for audit trails

### Operational Security

1. **Monitor admin endpoints** for unusual activity
2. **Rotate allowed IPs** regularly
3. **Use separate admin ports** if needed
4. **Implement alerting** on admin access patterns

## Troubleshooting

### Common Issues

#### Admin Endpoints Return 404

- Check `AutoAdminConfig.admin_prefix` matches your request path
- Ensure `handle_admin_with_custom` is called in your request handler
- Verify the endpoint is enabled in config (`enable_status: true`)

#### Firewall Blocking Legitimate Requests

- Check server logs for "Admin access denied" messages
- Verify client IP is in the allow list
- Ensure CIDR notation is correct (`192.168.1.0/24` not `192.168.1.0-255`)
- Consider client IP detection through proxies/load balancers

#### Custom Endpoints Not Working

- Ensure custom handler returns `Some(response)` for handled requests
- Return `None` for unhandled requests to continue processing
- Check that firewall allows access before custom handler is called

### Debugging Tools

```bash
# Test automatic endpoints
curl -v http://127.0.0.1:8080/admin/status
curl -v http://127.0.0.1:8080/admin/health
curl -v http://127.0.0.1:8080/admin/info

# Test firewall (should be denied)
curl -v -H "X-Forwarded-For: 203.0.113.1" http://127.0.0.1:8080/admin/status

# Check logs
tail -f /var/log/lithair.log | grep -i admin
```

This admin protection system provides enterprise-grade security with zero configuration overhead while remaining flexible for custom requirements.
