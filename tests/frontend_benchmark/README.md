# Frontend Benchmark (Phase A)

A minimal, neutral frontend (vanilla JS) used as a common benchmark UI across stacks.

This phase serves the frontend directly from Lithair to validate that Lithair acts as both the API server and a static web server.

## Directory structure

- `tests/frontend_benchmark/dist/`
  - `index.html` — main page (lists products, simple create form)
  - `assets/`
    - `main.js` — calls `/status` and `/api/products` with `fetch`, records performance metrics via the Web Performance API
    - `styles.css` — minimal UI styles
- `tests/frontend_benchmark/run_frontend_demo.sh` — runner that:
  - Builds and starts the `http_firewall_declarative` demo binary
  - Serves static files via `RS_STATIC_DIR`
  - Verifies `/`, `/assets/main.js`, `/status`, `/api/products` and a basic POST/GET flow

## How it works

The Lithair declarative server will serve static files when the `RS_STATIC_DIR` environment variable is set to a directory containing `index.html` and an `assets/` directory.

In `tests/frontend_benchmark/run_frontend_demo.sh`, we set:

```bash
RS_STATIC_DIR="tests/frontend_benchmark/dist"
```

The server’s router in `lithair-core/src/http/declarative_server.rs` then:
- Serves `index.html` for `/` and `/index.html`
- Serves files under `/assets/*`
- Delegates `/api/products` to the declarative CRUD API

## Run the demo (no Docker)

```bash
bash tests/frontend_benchmark/run_frontend_demo.sh
```

What it does:
- Starts the server on `PORT=18090` (override with `PORT=...`)
- Checks static endpoints and API endpoints
- Creates a product and re-lists (`POST` then `GET`)
- Prints a PASS/FAIL summary

Example:

```
✅ Frontend demo PASS (6 scenarios)
```

## Using Docker (optional, future phases)

This phase does not require Docker. In Phase B/C we will introduce a Docker-based environment for comparative stacks (e.g., PHP/MySQL) while keeping a non-Docker path for environments without Docker.

## Notes

- The demo uses the model-level firewall from `examples/raft_replication_demo/http_firewall_declarative.rs`. The runner waits briefly after a `POST` to avoid hitting the QPS window before listing again.
- The frontend is intentionally vanilla JS to stay neutral and minimize perf bias.
- The API is assumed to be at `/api/products`. The frontend derives `API_BASE_URL` from `location.origin` by default.
