# Frontend Serving Modes

Lithair provides three serving modes that balance performance and developer experience.

## Development (`--dev`)

- Assets served directly from disk on each request
- Minimal memory usage, instant updates
- Headers: `Cache-Control: no-cache`, `X-Served-From: Disk-Dev-Mode`
- Best for local development and rapid iteration

  Example:
  
  ```bash
  cargo run -- --port 3000 --dev
  ```

## Production (default)

- Assets preloaded into memory (SCC2) at startup
- Highest performance and lowest latency
- Standard caching headers for browsers
- Best for production deployments

Example:

```bash
cargo run -- --port 3000
# or explicitly
cargo run -- --port 3000 --prod
```

## Hybrid (`--hybrid`)

- Serves from in-memory SCC2 like production
- Hot reload via API: `POST /admin/sites/reload`
- Atomic swap of frontend assets (zero downtime)
- Best for active dev with production-like performance

Example:

```bash
# Development-friendly reload (token bypass)
LT_DEV_RELOAD_TOKEN=dev123 cargo run -- --port 3000 --hybrid
curl -X POST http://localhost:3000/admin/sites/reload -H "X-Reload-Token: dev123"

# Production-safe reload (auth required)
TOKEN=$(curl -s -X POST http://localhost:3000/auth/login -H 'Content-Type: application/json' -d '{"username":"admin","password":"password123"}' | jq -r '.session_token')
curl -X POST http://localhost:3000/admin/sites/reload -H "Authorization: Bearer $TOKEN"
```

## See also

- Full guide: `../../guides/serving-modes.md`
- Env vars: `../../reference/env-vars.md`
