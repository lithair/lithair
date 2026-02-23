# TLS Configuration Guide

Lithair supports native TLS termination via [rustls](https://github.com/rustls/rustls), eliminating the need for a reverse proxy in simple deployments.

## Quick Start

### 1. Generate a self-signed certificate (development)

```bash
openssl req -x509 -newkey rsa:2048 \
  -keyout key.pem -out cert.pem \
  -days 365 -nodes -subj "/CN=localhost"
```

### 2. Start the server with TLS

```bash
LT_TLS_CERT=cert.pem LT_TLS_KEY=key.pem cargo run
```

### 3. Verify

```bash
curl -kI https://localhost:8080/
```

You should see `strict-transport-security: max-age=31536000; includeSubDomains` in the response headers.

## Configuration

### Environment Variables

| Variable | Description |
|----------|-------------|
| `LT_TLS_CERT` | Path to PEM-encoded certificate file (or chain) |
| `LT_TLS_KEY` | Path to PEM-encoded private key file |

Both variables must be set together. If neither is set, the server starts in plain HTTP mode.

### Builder API

```rust
LithairServer::new()
    .with_tls("cert.pem", "key.pem")
    .serve()
    .await?;
```

The builder method and environment variables can be combined; env vars take priority when `apply_env_vars()` runs.

## Certificate Fingerprint

When TLS is enabled, the server logs the SHA-256 fingerprint of the leaf certificate at startup:

```
[INFO] TLS certificate SHA-256: ab:cd:ef:12:34:...
```

Use this to verify the correct certificate is loaded, especially in automated deployments.

## HSTS (HTTP Strict Transport Security)

When TLS is active, the server automatically adds the following header to every response:

```
strict-transport-security: max-age=31536000; includeSubDomains
```

This instructs browsers to always use HTTPS for subsequent requests. The header is only sent when TLS is active (never on plain HTTP).

## Production Setup

### Using Let's Encrypt certificates

```bash
# After certbot generates your certs:
LT_TLS_CERT=/etc/letsencrypt/live/example.com/fullchain.pem \
LT_TLS_KEY=/etc/letsencrypt/live/example.com/privkey.pem \
cargo run --release
```

### Certificate chain

`LT_TLS_CERT` should point to the full chain (leaf + intermediates). The PEM file can contain multiple certificates; the first is used as the leaf certificate.

### File permissions

Ensure the key file is readable only by the server process:

```bash
chmod 600 key.pem
```

## Validation

The server validates TLS configuration at startup:

- Both `LT_TLS_CERT` and `LT_TLS_KEY` must be set together (setting only one is an error)
- Both files must exist on disk
- The certificate and key must form a valid pair (rustls validates this)

If validation fails, the server exits with a descriptive error message.

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `LT_TLS_KEY is missing` | Only cert is set | Set both `LT_TLS_CERT` and `LT_TLS_KEY` |
| `TLS certificate file not found` | Wrong path | Check file path and permissions |
| `Invalid TLS certificate/key pair` | Mismatched cert/key | Regenerate or use matching pair |
| `curl: (60) SSL certificate problem` | Self-signed cert | Use `curl -k` or add cert to trust store |

## See Also

- `docs/reference/env-vars.md` for all environment variables
- `docs/internal/HTTP_HARDENING.md` for security headers
