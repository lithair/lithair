# SCC2 Server Demo (Hyper + SCC2 Max Performance)

High-performance demo server showcasing:

- Stateless HTTP performance endpoints built on Hyper 1.7
- SCC2 lock-free HashMap for ultra-fast KV operations
- Optional gzip compression based on `Accept-Encoding`
- Bulk JSON endpoints to stress concurrent SCC2 access

## Quickstart

Using Taskfile (recommended):

```bash
# Full demo (build + server + benchmarks)
task scc2:demo

# Start server only (debug build)
task scc2:serve PORT=18321 HOST=127.0.0.1

# Run stateless JSON benchmark
task loadgen:json LEADER=http://127.0.0.1:18321 BYTES=1024 TOTAL=20000 CONC=512

# Gzip comparison demo
task scc2:gzip
```

Manual startup (alternative):

```bash
# Start server (default: 127.0.0.1:18321)
cargo run -p scc2_server_demo -- --port 18321

# Run stateless benchmarks via loadgen
cargo run -p raft_replication_demo --bin http_loadgen_demo -- \
  --leader http://127.0.0.1:18321 --total 20000 --concurrency 512 --mode perf-status --perf-path /status
```

## Endpoints

- Stateless perf
  - `GET /status` → `OK`
  - `GET /perf/json?bytes=N` → JSON payload with `N` data bytes
  - `GET /perf/bytes?n=N` → raw bytes of size `N`
  - `POST /perf/echo` → echoes request body
- SCC2 KV
  - `POST /scc2/put?key=K&n=N` → store key with `N` bytes
  - `GET /scc2/get?key=K` → returns `LEN=<size>` or `404`
- SCC2 Bulk (JSON)
  - `POST /scc2/put_bulk` with body:
    ```json
    [
      {"key":"k1","n":4096},
      {"key":"k2","value":"custom"}
    ]
    ```
  - `POST /scc2/get_bulk` with body:
    ```json
    ["k1","k2","missing"]
    ```

## Gzip Compression

This server automatically gzips responses when the client sends `Accept-Encoding: gzip`.

Run comparison benchmarks with:

```bash
task scc2:gzip
```

The script will:
- Start the server (if not already running)
- Measure `/perf/json?bytes=1024` without gzip using the loadgen tool
- Measure the same path with gzip using `curl` with `-H 'Accept-Encoding: gzip'`
- Print timings and effective request rates

## Tuning Tips

- Use higher concurrency (e.g., `--concurrency 512` or `1024`) to saturate cores.
- Prefer release builds for realistic numbers:
  ```bash
  cargo run --release -p scc2_server_demo -- --port 18321
  cargo run --release -p raft_replication_demo --bin http_loadgen_demo -- --leader http://127.0.0.1:18321 --total 50000 --concurrency 1024 --mode perf-json --perf-path /perf/json --perf-bytes 1024
  ```
- When testing gzip, larger payloads and/or higher concurrency show clearer benefits.

## Notes

- SCC2 operations use async methods (`insert_async`, `get_async`) for maximum scalability.
- Responses include `Vary: accept-encoding` and `Content-Encoding: gzip` when compressed.
- This demo is minimal—no persistence; it focuses on HTTP + in-memory SCC2 throughput.
