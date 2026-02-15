# 01 - Hello World

The simplest Lithair server. Zero models, zero auth — just a running server with admin panel.

## Run

```bash
cargo run -p hello-world
# Open http://localhost:8080
```

## What you learn

- `LithairServer::new()` builder pattern
- `.with_port()`, `.with_host()`, `.with_cors()`
- Gzip compression config
- Admin panel (`/admin`)
- Metrics endpoint (`/metrics`)
- Development logging

## Code highlights

```rust
LithairServer::new()
    .with_port(8080)
    .with_host("127.0.0.1")
    .with_cors(true)
    .with_logging_config(LoggingConfig::development())
    .with_gzip_config(GzipConfig { enabled: true, min_bytes: 1024 })
    .with_admin_panel(true)
    .with_metrics(true)
    .serve()
    .await?;
```

## Next

Add static files → [02-static-site](../02-static-site/)
