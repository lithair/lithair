# Observability: logs, traces, and correlation

This guide covers Lithair's logging/tracing stack: how log output is
produced and filtered, how requests are correlated via `X-Request-ID`,
and how to export distributed traces to an OpenTelemetry collector
(Jaeger, Tempo, any OTLP/gRPC endpoint).

Scope note: **this document is about traces and logs.** Numeric metrics
are a separate, always-on surface — Prometheus-compatible gauges at
`/metrics` (see `docs/operations/deployment-systemd-k8s.md` for the
ServiceMonitor wiring and the README's Monitoring & Health Checks
section). Traces are **not** a replacement for `/metrics`; they answer
"where did this one slow request spend its time", not "what is p99".

## Mental model

```text
~550 log::* call sites ──► tracing-log bridge ──► tracing subscriber
                                                   ├─ EnvFilter (RUST_LOG)
                                                   ├─ fmt layer (stderr)
                                                   └─ otel layer (opt-in)
```

- **`log::*` is still the in-crate convention.** The ~550 existing call
  sites were not mass-migrated; a `tracing_log::LogTracer` bridge feeds
  them into the tracing subscriber (`init_default_tracing()` in
  `lithair-core/src/app/mod.rs`).
- **The subscriber is installed first-wins** at `serve()` start. If you
  installed your own `log` backend or tracing subscriber before calling
  `serve()`, yours is kept untouched.
- **Spans exist on five critical paths** (issue #107 phase 1):

  | Span | Where | Fields |
  |------|-------|--------|
  | `http_request` | per-request dispatch, `app/mod.rs` | `method`, `path`, `request_id` |
  | `event_append` | event store, `engine/events.rs` | — |
  | `snapshot_save` / `snapshot_load` | `engine/snapshot.rs` | — |
  | `retention_evict` | `http/declarative.rs` | — |
  | `event_replay` | startup replay, `http/declarative.rs` | event count |

  Every log line and span event emitted while handling a request happens
  inside `http_request`, so it carries the `request_id` field.

## Request correlation: X-Request-ID

Every response carries an `X-Request-ID` header, attached at a single
top-level point in the serve loop so no branch (firewall 403/429,
rate-limit 429, body-size 413, handler 500, success) can miss it.

Sanitization rules (`request_id_from_headers`): an inbound
`X-Request-ID` is echoed back **only** when it is 1–128 bytes of visible
ASCII (0x21–0x7E). Anything else — empty, oversized, whitespace, control
bytes, non-ASCII — is replaced with a freshly generated UUID v4, so
hostile bytes are never reflected into a response header or log output.

```bash
curl -si http://localhost:8080/health -H 'X-Request-ID: deploy-checkout-42' | grep -i x-request-id
# x-request-id: deploy-checkout-42
```

## Log filtering: RUST_LOG

The filter is a standard `tracing_subscriber::EnvFilter`, honoring
`RUST_LOG` with the same directive syntax `env_logger` used:

```bash
RUST_LOG=info ./my-app                      # everything at info+
RUST_LOG=warn,lithair_core=debug ./my-app   # per-crate override
```

When `RUST_LOG` is unset the fallback is `error` — **except** when
`LT_OTEL_ENDPOINT` is set, where the fallback is raised to `info`. The
five spans are emitted at INFO; under an `error` filter they would be
discarded before reaching the exporter and an operator who explicitly
opted into tracing would see zero spans. A set `RUST_LOG` always wins —
if you export traces with a custom filter, make sure it admits INFO for
`lithair_core` or your collector stays empty.

## Log-level policy

What each level means in Lithair — the contract for reading production logs,
and the rule contributors follow when adding a call site:

| Level | Meaning | Frequency guarantee |
|---|---|---|
| `error` | Actionable failure — an operator may need to react | rare |
| `warn`  | Degraded/misconfigured but still serving (empty role list, env var without its feature, best-effort persist failed) | rare |
| `info`  | **Lifecycle events only**: boot, config summary, model registration, shutdown, snapshot install/ship, resync, schema migrations, frontend reload | *never* per-request |
| `debug` | Per-request / per-write flow (dispatch decisions, consensus apply, replication acks, audited-field traces) | hot paths |
| `trace` | Per-item detail inside loops | hottest |

The performance rationale: a *disabled* level costs one atomic load per call
site — negligible. An *enabled* `info` on a per-write path costs formatting +
I/O on every request, which is why per-request lines live at `debug`: running
production at `RUST_LOG=info` gives quiet, lifecycle-only logs at full speed,
and `lithair_core=debug` turns the flow on when needed. There are deliberately
**no compile-time level caps** (`release_max_level_*`): an operator can raise
verbosity on a live production binary with an env var and a restart — no
rebuild.

## Exporting traces (OpenTelemetry)

Trace export is **off by default twice over**: it requires the `otel`
Cargo feature at build time AND `LT_OTEL_ENDPOINT` at run time.

```toml
# Cargo.toml of your application
lithair-core = { version = "0.12", features = ["otel"] }
```

| Variable | Effect | Default |
|----------|--------|---------|
| `LT_OTEL_ENDPOINT` | OTLP/gRPC collector endpoint; enables export | unset = no export |
| `LT_OTEL_SERVICE_NAME` | OTel `service.name` resource attribute | `lithair` |

```bash
LT_OTEL_ENDPOINT=http://otel-collector:4317 \
LT_OTEL_SERVICE_NAME=checkout-api \
./my-app
```

Failure behavior is fail-open in both directions:

- **Binary built without the feature, env var set:** one warning at
  startup — `LT_OTEL_ENDPOINT is set but this build lacks the 'otel'
  feature — traces will not be exported` — then normal operation.
- **Feature built, endpoint malformed:** the init error is logged and
  the server starts without export. An unreachable (but well-formed)
  endpoint does not fail startup at all: the gRPC channel is lazy and
  per-batch export errors never disturb request handling.

### Local Jaeger in one command

```yaml
# docker-compose.yml
services:
  jaeger:
    image: jaegertracing/all-in-one:latest
    ports:
      - "4317:4317"     # OTLP gRPC ingest
      - "16686:16686"   # Jaeger UI
```

```bash
docker compose up -d jaeger
LT_OTEL_ENDPOINT=http://localhost:4317 RUST_LOG=info ./my-app
curl -s http://localhost:8080/health   # returns {"status":"healthy"}
# open http://localhost:16686, service "lithair" → http_request spans
```

## Shutdown and span flushing

The exporter batches spans and sends them on an interval. On graceful
shutdown (`serve_with_graceful_shutdown`), after the accept loop stops
and the 5s connection-drain window elapses, the tracer provider is shut
down and force-flushed so the spans of the final requests are exported
rather than lost. The flush is bounded (the SDK caps shutdown at 5s, and
Lithair wraps it in a further 6s timeout on a blocking thread), so a
dead collector cannot hang shutdown. A plain `serve()` never returns, so
processes killed without the graceful path may lose the last batch —
wire the shutdown hook in production (see
`docs/operations/deployment-systemd-k8s.md`).

## Current limitations

Be aware of what this stack does **not** do today:

- **Spans are coarse.** Five instrumentation sites (listed above). There
  are no per-handler, per-RBAC-check, or consensus-round spans yet, and
  no inbound `traceparent` propagation — each request starts a new
  trace, correlated by `request_id` rather than W3C trace context.
- **No metrics over OTel.** Metrics remain Prometheus-only at
  `/metrics`; the `otel` feature exports traces exclusively.
- **No dynamic level reload.** The `log`→`tracing` bridge freezes its
  max-level hint at init time (documented at the cap site in
  `init_default_tracing()`); changing verbosity requires a restart.
- **First-wins init.** If your application installs its own subscriber
  before `serve()`, Lithair's stack — including the otel layer — steps
  aside entirely; you own export wiring in that case.
