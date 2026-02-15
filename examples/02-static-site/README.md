# 02 - Static Site

Serve a static website from SCC2 memory. Zero disk I/O at runtime.

## Run

```bash
cargo run -p static-site
# Open http://localhost:8080
```

## What you learn

- `.with_frontend("path/to/public")` loads files into SCC2
- All assets served from lock-free concurrent memory
- Gzip compression on responses
- No file system reads after startup

## How it works

At startup, Lithair reads `public/` into SCC2 (lock-free HashMap).
Every HTTP request is served directly from memory — no disk, no cache invalidation, no complexity.

## Code highlights

```rust
LithairServer::new()
    .with_port(8080)
    .with_gzip_config(GzipConfig { enabled: true, min_bytes: 512 })
    .with_frontend("examples/02-static-site/public")
    .serve()
    .await?;
```

## Next

Add a REST API → [03-rest-api](../03-rest-api/)
