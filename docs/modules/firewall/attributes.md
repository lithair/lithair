# Declarative HTTP Firewall on Models

This document explains how to configure the Lithair HTTP firewall directly on your data model using a struct-level attribute, without relying on environment variables or CLI flags.

The attribute is designed for route-scoped enforcement (protect only the endpoints you want) while keeping other endpoints open.

## Attribute

Place the attribute on the model struct that derives `DeclarativeModel`.

Important: put the attribute after the `derive` line to avoid the legacy helper warning.

Example:

```rust
use lithair_macros::DeclarativeModel;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
#[firewall(
  enabled = true,
  allow = "127.0.0.1",
  deny = "",
  protected = "/api/products",
  exempt = "/status,/health",
  global_qps = 3,
  per_ip_qps = 2
)]
pub struct Product {
    // fields ...
}
```

## Keys

- enabled: `true | false`
- allow: CSV string of IPs to allow (exact match). If non-empty, only these IPs are allowed
- deny: CSV string of IPs to deny (exact match). Takes precedence over allow
- global_qps: global requests-per-second limit (u64). Omit to disable
- per_ip_qps: per-IP requests-per-second limit (u64). Omit to disable
- protected: CSV string of URL prefixes where firewall applies. When non-empty, the firewall is enforced only on these prefixes
- exempt: CSV string of URL prefixes that bypass the firewall entirely (e.g., `/status`, `/health`)

## Precedence Rules

1. Builder config from code: `DeclarativeServer::with_firewall_config(...)`
2. Model-level config: `#[firewall(...)]` on the model
3. Environment variables: `LT_FW_*`

The builder config is the highest priority and will override the model attribute. The model attribute overrides environment variables.

## Route Scoping

The firewall supports route scoping:

- If `exempt` contains a prefix that matches the requested path, the firewall is bypassed completely for that request
- If `protected` is non-empty, the firewall is applied only when the requested path matches one of the protected prefixes
- If `protected` is empty, the firewall applies globally (except on `exempt` prefixes)

## Interaction with the Server

`DeclarativeServer` automatically consults the sources using the precedence above:

```rust
let fw_cfg = self
    .firewall_config               // builder
    .clone()
    .or_else(|| <T as HttpExposable>::firewall_config())  // model
    .unwrap_or_else(FirewallConfig::from_env);            // env
```

## Demo and CLI

The example demo (`examples/http_firewall_demo/run_demo.sh`) uses CLI flags to make scenarios explicit and testable:

- `--fw-enable true|false`
- `--fw-allow <CSV>`
- `--fw-deny <CSV>`
- `--fw-global-qps <u64>`
- `--fw-perip-qps <u64>`
- `--fw-protected-prefixes <CSV>`
- `--fw-exempt-prefixes <CSV>`

This is a demonstration choice. In your application, prefer the model attribute for simplicity and self-contained configuration.

## Known Notes

- Keep the attribute after the derive line, e.g.:

```rust
#[derive(DeclarativeModel)]
#[firewall(...)]
struct MyModel { ... }
```

- In the demo crate, we keep the CLI builder path active to avoid a current proc-macro type-inference quirk when combining example code, macro expansion, and certain compiler lints. The attribute is stable for application code; the builder remains a reliable override.

## Troubleshooting

- `403` when using allow-list: ensure your client IP (often `127.0.0.1`) is present in `allow` if it is non-empty
- Rate limit tests: expect mixtures of `200` and `429` under bursts; adjust `global_qps` and `per_ip_qps` accordingly
- Prefix matching is prefix-based (not regex). Ensure you provide the exact API base path, e.g. `/api/products`

## What you can run now (Quickstart)

Fully declarative (model attribute only):

```bash
bash examples/http_firewall_demo/run_declarative_demo.sh
```

Or manual:

```bash
cargo run -p raft_replication_demo --bin http_firewall_declarative -- --port 8081
curl http://127.0.0.1:8081/status
curl http://127.0.0.1:8081/api/products
```

CLI-configurable demo (flags):

```bash
bash examples/http_firewall_demo/run_demo.sh
```

Demonstrates deny/allow and rate limiting with route scoping.
