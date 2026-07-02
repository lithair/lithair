//! Lithair Server - Unified multi-model server with RBAC and Sessions
//!
//! The LithairServer provides a complete HTTP server with:
//! - Multiple models on a single server
//! - Global RBAC and session management
//! - Automatic configuration loading
//! - Hot-reload support
//! - Admin panel and metrics
//!
//! # Example
//!
//! ```no_run
//! use lithair_core::LithairServer;
//! use lithair_core::session::{SessionManager, MemorySessionStore};
//!
//! # async fn example() -> anyhow::Result<()> {
//! LithairServer::new()
//!     .with_port(8080)
//!     .with_sessions(SessionManager::new(MemorySessionStore::new()))
//!     .with_admin_panel(true)
//!     .serve()
//!     .await?;
//! # Ok(())
//! # }
//! ```

use crate::cluster::RaftLeadershipState;
use crate::config::LithairConfig;
use anyhow::{Context, Result};
use bytes::Bytes;
#[cfg(feature = "tls")]
use sha2::Digest;
use std::sync::Arc;
// `Future::instrument(span)` for the per-request `http_request` span in the
// serve loop (issue #107). Imported anonymously: only the method matters.
use tracing::Instrument as _;

pub mod builder;
mod data_admin;
mod frontend_admin;
mod model_dispatch;
pub mod model_handler;
mod ops_endpoints;
mod replication;
pub mod request;
pub mod response;
pub mod router;
mod schema_handlers;

pub use builder::LithairServerBuilder;
pub use model_handler::{DeclarativeModelHandler, ModelHandler, ModelStats};

// ============================================================================
// Public route-handler type aliases (issue #59)
// ============================================================================
//
// Consumers registering custom routes via `LithairServerBuilder::with_route`
// only need to spell out the request and response types in their handler
// signatures. Exposing them as aliases — together with the underlying
// `http::Method` and `http::StatusCode` re-exports — lets downstream crates
// drop direct dependencies on `bytes`, `http`, `http-body-util`, and `hyper`
// from their `Cargo.toml` when they don't otherwise interact with those
// crates. See `lithair/lithair#59` for the motivating use case (kovre's
// custom dashboard routes).

/// Request passed to handlers registered via
/// [`LithairServerBuilder::with_route`], [`LithairServerBuilder::with_route_async`],
/// and [`LithairServerBuilder::with_not_found_handler`].
///
/// This is a type alias for `hyper::Request<hyper::body::Incoming>` — no
/// wrapping, no overhead. The alias exists so consumers can type their
/// handlers without depending on `hyper` directly.
pub type RouteRequest = hyper::Request<hyper::body::Incoming>;

/// Response returned by handlers registered via
/// [`LithairServerBuilder::with_route`], [`LithairServerBuilder::with_route_async`],
/// and [`LithairServerBuilder::with_not_found_handler`], and by every helper
/// in [`response`] (`json`, `json_value`, `json_serialize`, `text`, `html`,
/// `redirect`, `empty`).
///
/// This is a type alias for
/// `hyper::Response<http_body_util::combinators::BoxBody<bytes::Bytes, std::convert::Infallible>>`.
/// The `BoxBody` wrapper allows handlers to return either buffered
/// (`Full<Bytes>`) or streaming (`StreamBody`) response bodies. This is
/// critical for SSE endpoints (`/api/{model}/stream`) where the body must
/// be sent incrementally as events arrive (issue #93).
///
/// The alias exists so consumers can type their handlers and return values
/// without depending on `hyper`, `http-body-util`, or `bytes` directly.
pub type RouteResponse =
    hyper::Response<http_body_util::combinators::BoxBody<bytes::Bytes, std::convert::Infallible>>;

/// Convenience constructor: wrap a buffered payload in `BoxBody` for use as
/// a [`RouteResponse`] body.
///
/// This is the `RouteResponse`-level equivalent of `Full::new(data.into()).boxed()`
/// — a one-liner that keeps call sites readable after the body type migration
/// from `Full<Bytes>` to `BoxBody<Bytes, Infallible>` (issue #93).
#[inline]
fn boxed_full(
    data: impl Into<bytes::Bytes>,
) -> http_body_util::combinators::BoxBody<bytes::Bytes, std::convert::Infallible> {
    use http_body_util::BodyExt;
    http_body_util::Full::new(data.into()).boxed()
}

/// Install the default tracing stack used by `serve()` /
/// `serve_with_graceful_shutdown` (issue #107, phase 1).
///
/// Two independent try-style steps, in the same order
/// `tracing_subscriber::util::SubscriberInitExt::try_init` uses internally
/// (dispatcher first, so the `log` max-level hint below sees the installed
/// filter):
///
/// 1. A `tracing_subscriber` registry — `EnvFilter` honoring `RUST_LOG`
///    plus a compact fmt layer on stderr — becomes the global tracing
///    subscriber via `set_global_default`.
/// 2. `tracing_log::LogTracer` becomes the global `log` backend so the
///    ~550 existing `log::*` call sites in this crate flow into tracing
///    untouched (bridge strategy — deliberately NOT a mass migration).
///
/// Both steps ignore "already initialized" errors, preserving the
/// first-wins contract the previous `env_logger::try_init()` had: users
/// who installed their own `log` backend first — e.g. the opt-in
/// `RaftstoneLogger` from `crate::logging` — keep it, and their `log::*`
/// records keep flowing to it unchanged (step 2 is then a no-op).
/// Likewise a user-installed tracing subscriber wins over step 1.
///
/// We deliberately do NOT use `SubscriberInitExt::try_init()`: with
/// tracing-subscriber's default `tracing-log` feature it bails with `Err`
/// after the dispatcher install when a `log` backend already exists, which
/// makes the two installs impossible to reason about independently. The
/// manual sequence keeps each step's try-semantics explicit (verified
/// against tracing-subscriber 0.3.23 / tracing-log 0.2.0 sources).
///
/// # OpenTelemetry export (issue #107, phase 2 — `otel` feature)
///
/// When the crate is compiled with the `otel` feature AND `LT_OTEL_ENDPOINT`
/// is set, an OTLP/gRPC span exporter layer is added to the SAME registry
/// composition (fmt layer + EnvFilter + otel layer coexist). When the env
/// var is unset, the layer is `None` and behavior is exactly the phase-1
/// stack. When the env var is set but the feature was NOT compiled, a
/// one-time warning is emitted so operators aren't silently confused.
/// An otel init failure (bad endpoint syntax, tonic error) never aborts
/// startup — it is logged and the server continues without export.
fn init_default_tracing() {
    use tracing_log::AsLog;
    use tracing_subscriber::layer::SubscriberExt;

    // RUST_LOG semantics: `env_logger::Builder::from_default_env()` (the
    // previous default) fell back to `error` when RUST_LOG is unset.
    // `EnvFilter::new("error")` preserves that exact out-of-the-box
    // verbosity; a set RUST_LOG is honored with the same directive syntax.
    //
    // One documented exception (issue #107 phase 2): when LT_OTEL_ENDPOINT
    // is set and RUST_LOG is NOT, the fallback is raised from `error` to
    // `info`. The five instrumentation spans are emitted at INFO — under
    // the `error` fallback the registry-level filter would discard them
    // and an operator who explicitly opted into tracing would see zero
    // spans in their collector (and none of the otel init notices below).
    // A set RUST_LOG always wins unchanged. This check is compiled in ALL
    // builds (not cfg-gated) so the filter computation — and therefore the
    // visibility of the missing-feature warning below — is identical
    // whether or not the `otel` feature is present.
    let fallback = if otel_endpoint_requested() { "info" } else { "error" };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(fallback));

    // Match the old env_logger output as closely as `fmt` allows:
    // millisecond UTC timestamps (`format_timestamp_millis()`), stderr
    // (env_logger's default target), ANSI color only on a tty (env_logger
    // auto-detected; fmt would otherwise default to always-on), and no
    // target/module path (the old init used `format_module_path(false)`).
    let fmt_layer = tracing_subscriber::fmt::layer()
        .compact()
        .with_timer(tracing_subscriber::fmt::time::ChronoUtc::new(
            "%Y-%m-%dT%H:%M:%S%.3fZ".to_owned(),
        ))
        .with_target(false)
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .with_writer(std::io::stderr);

    // Build the (optional) otel layer BEFORE installing the subscriber: any
    // init outcome must be reported through the subscriber we are about to
    // install, so the notice is stashed and emitted after the install below.
    // `Option<Layer>` implements `Layer` (None = pass-through), so the
    // composition type stays uniform whether export is active or not.
    #[cfg(feature = "otel")]
    let (otel_layer, otel_provider, otel_notice) = build_otel_layer_from_env();

    let subscriber = tracing_subscriber::registry().with(filter).with(fmt_layer);
    #[cfg(feature = "otel")]
    let subscriber = subscriber.with(otel_layer);
    // Capture the install outcome (otel builds need it to decide the
    // provider's fate below); default builds keep the historical
    // ignore-the-result try-semantics.
    #[cfg(not(feature = "otel"))]
    let _ = tracing::subscriber::set_global_default(subscriber);
    #[cfg(feature = "otel")]
    let subscriber_installed = tracing::subscriber::set_global_default(subscriber).is_ok();

    // Provider lifecycle is bound to the install outcome (PR #119 review):
    // only a layer that actually routes spans may keep its provider alive.
    // If a custom subscriber won the first-wins race, our layer is orphaned
    // — shut the provider down here rather than leak its batch-export
    // worker thread for the process lifetime. `shutdown()` on a provider
    // whose lazy tonic channel never connected returns quickly; the SDK
    // caps it at 5s internally in the worst case (acceptable on this
    // once-per-process edge path).
    #[cfg(feature = "otel")]
    let otel_orphaned = if subscriber_installed {
        if let Some(provider) = otel_provider {
            let _ = OTEL_TRACER_PROVIDER.set(provider);
        }
        false
    } else if let Some(provider) = otel_provider {
        let _ = provider.shutdown();
        true
    } else {
        false
    };

    // Bridge `log` → `tracing`. The max-level hint mirrors what the
    // installed subscriber will actually accept (ERROR by default), so
    // filtered-out `log::*` calls stay as cheap as they were under
    // env_logger (which sets the same `log::set_max_level` hint from its
    // filter) instead of paying the bridge dispatch on every call.
    //
    // Deliberate trade-off (PR #118 review): this freezes the bridge's
    // ceiling at init time, so a hypothetical *runtime* level change in the
    // subscriber would not reach `log::*` call sites. Lithair has no dynamic
    // level reload today; if one lands (e.g. a tracing-subscriber `reload`
    // layer), drop this hint — or re-issue `log::set_max_level` from the
    // reload hook — as part of that change.
    let _ = tracing_log::LogTracer::builder()
        .with_max_level(tracing::level_filters::LevelFilter::current().as_log())
        .init();

    // Deferred otel init notices: emitted only now that a subscriber is
    // installed (ours or a pre-existing one), otherwise they would be
    // silently dropped. If our layer ended up orphaned (custom subscriber
    // won), the "export enabled" notice would be a lie — report the
    // orphaning instead so the operator knows export is NOT active.
    #[cfg(feature = "otel")]
    match otel_notice {
        Some(OtelInitNotice::Enabled { endpoint, service_name }) => {
            if otel_orphaned {
                tracing::warn!(
                    %endpoint,
                    "LT_OTEL_ENDPOINT is set but a tracing subscriber was already \
                     installed before Lithair's — the OTLP layer is not wired in, \
                     export is disabled and the provider was shut down"
                );
            } else {
                tracing::info!(
                    %endpoint,
                    service_name = %service_name,
                    "OpenTelemetry OTLP trace export enabled"
                );
            }
        }
        Some(OtelInitNotice::Failed { endpoint, error }) => {
            tracing::error!(
                %endpoint,
                %error,
                "OpenTelemetry OTLP exporter init failed; continuing without trace export"
            );
        }
        None => {}
    }
    // The operator asked for OTLP export but this binary cannot provide it.
    // This path must exist in default builds (cfg(not) + env check) so the
    // misconfiguration is never silent. Visible out of the box: the same
    // env var raised the unset-RUST_LOG fallback to `info` above.
    #[cfg(not(feature = "otel"))]
    if otel_endpoint_requested() {
        tracing::warn!(
            "LT_OTEL_ENDPOINT is set but this build lacks the 'otel' feature — \
             traces will not be exported (rebuild with `--features otel`)"
        );
    }
}

/// `true` when the operator requested OTLP export via `LT_OTEL_ENDPOINT`
/// (issue #107 phase 2). Compiled in ALL builds: default builds use it to
/// emit the missing-feature warning and to compute the same EnvFilter
/// fallback as otel builds; otel builds use it to decide whether to build
/// the exporter layer.
fn otel_endpoint_requested() -> bool {
    std::env::var("LT_OTEL_ENDPOINT").map(|v| !v.trim().is_empty()).unwrap_or(false)
}

/// Global handle to the OTLP tracer provider, set once by
/// [`build_otel_layer_from_env`] and read by [`shutdown_otel_provider`]
/// (issue #107 phase 2).
///
/// Why a `static OnceLock` rather than a field on `LithairServer`: the otel
/// layer is installed into the process-global tracing subscriber by
/// `init_default_tracing()`, whose whole contract is global, first-wins,
/// once-per-process (same as the `log` backend install). The provider that
/// backs that global layer has exactly the same lifetime — it outlives any
/// one server value (`serve_with_graceful_shutdown` consumes `self` into an
/// `Arc` and tests spin several servers per process), so a per-server field
/// would be a lie about ownership. `OnceLock` mirrors the first-wins
/// semantics: only the init call that actually installed the layer stores
/// its provider.
#[cfg(feature = "otel")]
static OTEL_TRACER_PROVIDER: std::sync::OnceLock<opentelemetry_sdk::trace::SdkTracerProvider> =
    std::sync::OnceLock::new();

/// Deferred outcome of the otel layer build, logged by
/// [`init_default_tracing`] once a subscriber is installed.
#[cfg(feature = "otel")]
enum OtelInitNotice {
    Enabled { endpoint: String, service_name: String },
    Failed { endpoint: String, error: String },
}

/// Build the OTLP/gRPC export layer from `LT_OTEL_ENDPOINT` /
/// `LT_OTEL_SERVICE_NAME` (issue #107 phase 2).
///
/// Returns `(None, None)` when `LT_OTEL_ENDPOINT` is unset — the registry
/// composition is then byte-identical in behavior to the phase-1 stack.
/// Returns `(None, Some(Failed))` when the exporter cannot be built (e.g.
/// invalid endpoint URI): startup continues without export, per the
/// fail-open contract of `init_default_tracing`.
///
/// Version pairing (bump together): opentelemetry 0.32 / opentelemetry_sdk
/// 0.32 / opentelemetry-otlp 0.32 (grpc-tonic) / tracing-opentelemetry 0.33.
///
/// Runtime requirements: this is called from `serve_with_graceful_shutdown`,
/// i.e. inside a live tokio runtime. That matters — `connect_lazy()` inside
/// the tonic exporter builder spawns the channel's I/O worker onto the
/// *current* runtime, and the SDK's batch span processor later drives
/// exports from its own dedicated thread through that channel. Building the
/// exporter outside a runtime would panic in tonic (upstream-documented
/// constraint, opentelemetry-otlp 0.32 crate docs).
#[cfg(feature = "otel")]
fn build_otel_layer_from_env<S>() -> (
    Option<tracing_opentelemetry::OpenTelemetryLayer<S, opentelemetry_sdk::trace::SdkTracer>>,
    Option<opentelemetry_sdk::trace::SdkTracerProvider>,
    Option<OtelInitNotice>,
)
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry_otlp::WithExportConfig;

    if !otel_endpoint_requested() {
        return (None, None, None);
    }
    // Non-empty by the check above; trim to tolerate stray whitespace from
    // env files.
    let endpoint = std::env::var("LT_OTEL_ENDPOINT")
        .map(|v| v.trim().to_owned())
        .unwrap_or_default();
    let service_name = std::env::var("LT_OTEL_SERVICE_NAME")
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "lithair".to_owned());

    // Endpoint-syntax and tonic transport errors surface here; an
    // unreachable-but-well-formed endpoint does NOT (the channel is lazy,
    // export failures are reported per-batch by the SDK's internal logging).
    let exporter = match opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint.clone())
        .build()
    {
        Ok(exporter) => exporter,
        Err(e) => {
            return (None, None, Some(OtelInitNotice::Failed { endpoint, error: e.to_string() }));
        }
    };

    let resource = opentelemetry_sdk::Resource::builder()
        .with_service_name(service_name.clone())
        .build();
    let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        // Default batch processor: buffers spans and exports from a
        // dedicated background thread. This is the batching the
        // shutdown-flush in `shutdown_otel_provider` exists for.
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build();
    let tracer = provider.tracer("lithair");

    // The provider is RETURNED, not registered in OTEL_TRACER_PROVIDER here
    // (PR #119 review): registration must be conditional on the global
    // subscriber install actually succeeding. If a custom subscriber was
    // installed first (the supported coexistence case), the layer built
    // below is orphaned — the caller then shuts this provider down instead
    // of leaking its batch-export worker thread.
    (
        Some(tracing_opentelemetry::layer().with_tracer(tracer)),
        Some(provider),
        Some(OtelInitNotice::Enabled { endpoint, service_name }),
    )
}

/// Flush and shut down the OTLP tracer provider at the end of a graceful
/// shutdown (issue #107 phase 2).
///
/// The batch processor exports on an interval; without this, spans recorded
/// after the last batch export — typically the final requests before the
/// shutdown signal — would be lost. No-op when export was never enabled.
///
/// Bounded: `SdkTracerProvider::shutdown()` blocks the calling thread (the
/// SDK caps it at 5s internally — verified against opentelemetry_sdk 0.32.1
/// sources), so it runs on `spawn_blocking` to keep the runtime responsive,
/// with a slightly larger outer timeout as a belt-and-braces bound in case
/// the SDK's own cap regresses.
#[cfg(feature = "otel")]
async fn shutdown_otel_provider() {
    const OTEL_SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(6);

    let Some(provider) = OTEL_TRACER_PROVIDER.get() else {
        return;
    };
    // `SdkTracerProvider` is a cheap Arc-backed clone; shutting down the
    // clone shuts down the shared inner provider.
    let provider = provider.clone();
    log::info!("Flushing OpenTelemetry spans before shutdown");
    match tokio::time::timeout(
        OTEL_SHUTDOWN_TIMEOUT,
        tokio::task::spawn_blocking(move || provider.shutdown()),
    )
    .await
    {
        Ok(Ok(Ok(()))) => log::info!("OpenTelemetry tracer provider shut down"),
        // Includes `AlreadyShutdown` when several servers in one process
        // shut down in sequence — harmless, hence warn not error.
        Ok(Ok(Err(e))) => log::warn!("OpenTelemetry shutdown reported: {}", e),
        Ok(Err(join_err)) => log::warn!("OpenTelemetry shutdown task panicked: {}", join_err),
        Err(_) => {
            log::warn!("OpenTelemetry shutdown timed out after {:?}", OTEL_SHUTDOWN_TIMEOUT)
        }
    }
}

/// Extract a usable correlation ID from the inbound `X-Request-ID` header,
/// or mint a fresh UUID v4 (issue #107).
///
/// Inbound values are accepted only when they are 1..=128 bytes of visible
/// ASCII (0x21..=0x7E). Anything else — empty, oversized, whitespace,
/// control bytes, non-ASCII — is replaced with a generated ID so we never
/// reflect hostile bytes back into a response header or into log output
/// (header-injection hygiene).
fn request_id_from_headers(headers: &hyper::HeaderMap) -> String {
    if let Some(value) = headers.get("x-request-id") {
        // Validate the raw bytes directly (single scan — PR #118 review);
        // `to_str()` would re-scan for UTF-8 first. Visible ASCII is valid
        // UTF-8 by construction, so the lossy conversion below is lossless.
        let bytes = value.as_bytes();
        if (1..=128).contains(&bytes.len()) && bytes.iter().all(|b| (0x21..=0x7e).contains(b)) {
            return String::from_utf8_lossy(bytes).into_owned();
        }
    }
    uuid::Uuid::new_v4().to_string()
}

/// HTTP method re-exported from the `http` crate.
///
/// `LithairServerBuilder::with_route` accepts a `http::Method`; re-exporting
/// it lets consumers write `use lithair_core::app::Method;` instead of pulling
/// in the `http` crate as a direct dependency.
pub use http::Method;

/// HTTP status code re-exported from the `http` crate.
///
/// All response helpers in [`response`] accept a `http::StatusCode`;
/// re-exporting it lets consumers write `use lithair_core::app::StatusCode;`
/// instead of pulling in the `http` crate as a direct dependency.
///
/// Note: this is the `http` crate's `StatusCode`, **not** the Lithair custom
/// `lithair_core::http::StatusCode` enum (which is used by the lithair-native
/// `HttpServer` / `Route` abstraction). The two types are distinct — this
/// alias matches the one used by `with_route` and the `response::*` helpers.
pub use http::StatusCode;

// ============================================================================
// TLS support types
// ============================================================================

/// A TCP stream that may or may not be wrapped in TLS.
#[cfg(feature = "tls")]
enum MaybeTlsStream {
    Plain(tokio::net::TcpStream),
    Tls(Box<tokio_rustls::server::TlsStream<tokio::net::TcpStream>>),
}

#[cfg(feature = "tls")]
impl tokio::io::AsyncRead for MaybeTlsStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            MaybeTlsStream::Plain(s) => std::pin::Pin::new(s).poll_read(cx, buf),
            MaybeTlsStream::Tls(s) => std::pin::Pin::new(s).poll_read(cx, buf),
        }
    }
}

#[cfg(feature = "tls")]
impl tokio::io::AsyncWrite for MaybeTlsStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match self.get_mut() {
            MaybeTlsStream::Plain(s) => std::pin::Pin::new(s).poll_write(cx, buf),
            MaybeTlsStream::Tls(s) => std::pin::Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            MaybeTlsStream::Plain(s) => std::pin::Pin::new(s).poll_flush(cx),
            MaybeTlsStream::Tls(s) => std::pin::Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            MaybeTlsStream::Plain(s) => std::pin::Pin::new(s).poll_shutdown(cx),
            MaybeTlsStream::Tls(s) => std::pin::Pin::new(s).poll_shutdown(cx),
        }
    }
}

/// Load TLS certificates from a PEM file.
#[cfg(feature = "tls")]
fn load_tls_certs(path: &str) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open TLS certificate file: {}", path))?;
    let mut reader = std::io::BufReader::new(file);
    let certs: Vec<_> = rustls_pemfile::certs(&mut reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("Failed to parse TLS certificates from: {}", path))?;
    if certs.is_empty() {
        anyhow::bail!("No certificates found in {}", path);
    }
    Ok(certs)
}

/// Load a TLS private key from a PEM file.
#[cfg(feature = "tls")]
fn load_tls_key(path: &str) -> Result<rustls::pki_types::PrivateKeyDer<'static>> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open TLS key file: {}", path))?;
    let mut reader = std::io::BufReader::new(file);
    rustls_pemfile::private_key(&mut reader)
        .with_context(|| format!("Failed to parse TLS key from: {}", path))?
        .ok_or_else(|| anyhow::anyhow!("No private key found in {}", path))
}

/// Model registration with handler
pub struct ModelRegistration {
    pub name: String,
    pub base_path: String,
    pub data_path: String,
    pub handler: Arc<dyn ModelHandler>,
    pub schema_extractor: Option<SchemaSpecExtractor>,
}

/// Deferred session-gate applier closure. Registered by `with_handler` paths
/// that bypass the `model_infos` pipeline; invoked at `serve()` time so the
/// `models_require_session` flag is propagated to externally-constructed
/// handlers through interior mutability on `DeclarativeHttpHandler`.
pub(crate) type ExternalHandlerGate = Box<dyn Fn(bool) + Send + Sync>;

/// Deferred SSE-broadcaster wiring closure. Registered by `with_handler` paths
/// (and therefore by `with_model_ref`, which delegates to `with_handler`) that
/// bypass the `model_infos` pipeline; invoked at `serve()` time to install the
/// builder-level SSE broadcaster onto externally-constructed handlers through
/// `OnceLock` interior mutability on `DeclarativeHttpHandler`. Issue #91.
pub(crate) type ExternalHandlerSseWiring =
    Box<dyn Fn(Arc<crate::http::sse::SseEventBroadcaster>) + Send + Sync>;

/// Lithair multi-model server
pub struct LithairServer {
    config: LithairConfig,
    session_manager: Option<Arc<dyn std::any::Any + Send + Sync>>,
    custom_routes: Vec<CustomRoute>,
    not_found_handler: Option<RouteHandler>,
    route_guards: Vec<crate::http::RouteGuardMatcher>,
    model_infos: Vec<ModelRegistrationInfo>,
    /// Issue #78: when true, every auto-generated `/api/{model}` endpoint
    /// must carry a valid session, otherwise the request is rejected with
    /// HTTP 401. Wired from
    /// `LithairServerBuilder::with_models_require_session(...)` and applied
    /// uniformly to every model handler in `serve()` after each model's
    /// factory has produced its handler.
    models_require_session: bool,

    /// Issue #86: deferred session-gate appliers for models registered via
    /// `LithairServerBuilder::with_handler(...)`. Each closure captures an
    /// `Arc<DeclarativeHttpHandler<T>>` clone and calls `set_require_session`
    /// on it. Executed in `serve()` after the boot-time session-store shape
    /// check, only when `models_require_session` is `true`. Without this, the
    /// `with_handler` path silently bypassed the gate even when the operator
    /// asked for it via `with_models_require_session(true)`.
    external_handler_gates: Vec<ExternalHandlerGate>,

    /// Issue #91: deferred SSE-broadcaster wirings for models registered via
    /// `LithairServerBuilder::with_handler(...)` (including `with_model_ref`,
    /// which delegates to it). Each closure captures an
    /// `Arc<DeclarativeHttpHandler<T>>` clone and calls
    /// `set_sse_broadcaster_shared` on it. Executed in `serve()` only when a
    /// builder-level broadcaster is configured (i.e. the user called
    /// `.with_sse(true)`). Without this, the `with_handler` path silently
    /// started with no broadcaster wired, so `apply_replicated_*` broadcasts
    /// from issue #89 became no-ops and `/api/{model}/stream` returned 404.
    external_handler_sse_wirings: Vec<ExternalHandlerSseWiring>,

    models: Arc<tokio::sync::RwLock<Vec<ModelRegistration>>>,

    // Frontend configurations to load (path_prefix -> static_dir)
    frontend_configs: Vec<(String, String)>,

    // Frontend serving (SCC2 memory-first) - Multiple frontends with path prefixes
    // Key: route_prefix (e.g., "/", "/admin"), Value: FrontendEngine
    frontend_engines: std::collections::HashMap<String, Arc<crate::frontend::FrontendEngine>>,

    // Per-vhost frontend configurations declared via
    // `LithairServerBuilder::with_vhost` / `with_default_vhost`. Loaded
    // into `vhost_frontend_router` at startup.
    vhost_frontend_configs: Vec<(crate::app::builder::VhostScope, String, String)>,

    // Host-header based frontend router built at startup from
    // `vhost_frontend_configs`. Each value is a map of route_prefix ->
    // FrontendEngine for that vhost, mirroring `frontend_engines` but
    // scoped to a specific `Host:` header.
    //
    // When empty (no vhosts declared) the server falls back entirely to
    // `frontend_engines`, preserving the pre-feature behaviour.
    vhost_frontend_router: crate::http::HostRouter<
        std::collections::HashMap<String, Arc<crate::frontend::FrontendEngine>>,
    >,

    // Host-to-host 301 redirects declared via
    // [`crate::app::LithairServerBuilder::with_redirect`]. Looked up by
    // the request's normalized `Host:` header *before* any vhost frontend
    // dispatch — see `handle_request`. The value stored is the canonical
    // target host (already normalized).
    //
    // When empty (no redirects declared) the server behaves as before.
    host_redirects: crate::http::HostRouter<String>,

    // HTTP Features
    firewall_config: Option<crate::http::FirewallConfig>,
    anti_ddos_config: Option<crate::security::anti_ddos::AntiDDoSConfig>,
    access_log: bool,
    access_log_capacity: usize,
    openapi_enabled: bool,
    openapi_spec_cache: std::sync::OnceLock<serde_json::Value>,

    // Raft cluster (distributed consensus)
    cluster_peers: Vec<String>,
    node_id: Option<u64>,
    raft_state: Option<Arc<RaftLeadershipState>>,

    // Raft CRUD consensus channel - for submitting CRUD operations through Raft
    // When Some and cluster_peers is non-empty, all writes go through Raft consensus
    #[allow(dead_code)]
    raft_crud_sender: Option<tokio::sync::mpsc::Sender<RaftCrudOperation>>,

    // Consensus log for ordered CRUD operations
    consensus_log: Option<Arc<crate::cluster::ConsensusLog>>,

    // Write-Ahead Log for durability (WAL ensures operations survive crashes)
    wal: Option<Arc<crate::cluster::WriteAheadLog>>,

    // Replication batcher for intelligent batching and follower health tracking
    replication_batcher: Option<Arc<crate::cluster::ReplicationBatcher>>,

    // Snapshot manager for full state snapshots (resync of desynced followers)
    snapshot_manager: Option<Arc<tokio::sync::RwLock<crate::cluster::SnapshotManager>>>,

    // Migration manager for rolling upgrades
    migration_manager: Option<Arc<crate::cluster::MigrationManager>>,

    // Resync statistics for observability
    resync_stats: Arc<crate::cluster::ResyncStats>,

    // Schema synchronization state for cluster-wide schema consensus
    schema_sync_state: Arc<tokio::sync::RwLock<crate::schema::SchemaSyncState>>,

    // SSE real-time subscriptions broadcaster (shared across all model handlers)
    sse_broadcaster: Option<Arc<crate::http::sse::SseEventBroadcaster>>,

    // Issue #69: builder-driven auto-compaction config. When `Some`,
    // `serve()` spawns one tokio task per registered model that
    // periodically inspects the model's `EventStore::event_count()` and
    // triggers `EventStore::truncate_events()` once the count crosses
    // `events_threshold`. The spawned tasks are fire-and-forget — runtime
    // shutdown aborts them, matching the existing background-flusher
    // lifecycle in `DeclarativeHttpHandler::new`.
    //
    // `None` = feature off (default, no observable behavior change).
    auto_compaction: Option<crate::engine::AutoCompactionConfig>,
}

/// A CRUD operation to be submitted through Raft consensus
#[derive(Debug)]
pub struct RaftCrudOperation {
    pub operation: crate::cluster::CrudOperation,
    pub response_tx: tokio::sync::oneshot::Sender<Result<serde_json::Value, String>>,
}

/// Type alias for async route handlers, as stored internally after a call to
/// [`LithairServerBuilder::with_route`] or
/// [`LithairServerBuilder::with_route_async`].
///
/// The handler input/output types are exposed as [`RouteRequest`] and
/// [`RouteResponse`] for consumers; this alias just bundles the
/// `Arc<dyn Fn(...) -> Pin<Box<dyn Future>>>` machinery used by the dispatcher.
pub type RouteHandler = Arc<
    dyn Fn(
            RouteRequest,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<RouteResponse>> + Send>>
        + Send
        + Sync,
>;

/// Custom route registration
pub struct CustomRoute {
    pub method: http::Method,
    pub path: String,
    pub handler: RouteHandler,
}

/// Type for async model handler factory
pub type ModelFactory = Arc<
    dyn Fn(
            String,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Arc<dyn ModelHandler>>> + Send>,
        > + Send
        + Sync,
>;

/// Type for schema spec extractor function
pub type SchemaSpecExtractor = Arc<dyn Fn() -> crate::schema::ModelSpec + Send + Sync>;

/// Model registration info with factory
pub struct ModelRegistrationInfo {
    pub name: String,
    pub base_path: String,
    pub data_path: String,
    pub factory: ModelFactory,
    /// Optional schema spec extractor for migration detection
    pub schema_extractor: Option<SchemaSpecExtractor>,
    /// Whether the builder-level `with_models_require_session(true)` switch
    /// should apply to this registration (issue #78). Set to `true` for
    /// models registered via `with_model(...)` / `with_declarative_model(...)`,
    /// `false` for `with_model_full(...)` — that path already supports RBAC
    /// via `PermissionChecker` and the issue #78 flag is intentionally
    /// scoped to the simple-CRUD path.
    pub require_session_applies: bool,
}

impl LithairServer {
    /// Create a new Lithair server with default configuration
    ///
    /// Configuration is loaded with full supersedence:
    /// 1. Defaults
    /// 2. Config file (config.toml)
    /// 3. Environment variables
    /// 4. Code (builder methods)
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> LithairServerBuilder {
        LithairServerBuilder::new()
    }

    /// Create a new server with custom configuration
    pub fn with_config(config: LithairConfig) -> LithairServerBuilder {
        LithairServerBuilder::with_config(config)
    }

    /// Validate schemas for all registered models with schema extractors
    ///
    /// This compares stored schema specs with current specs and handles
    /// differences based on the configured migration mode.
    async fn validate_schemas(&self) -> Result<()> {
        use crate::config::SchemaMigrationMode;
        use crate::schema::{
            load_schema_spec, save_schema_spec, AppliedSchemaChange, PendingSchemaChange,
            SchemaChangeDetector,
        };
        use std::path::Path;

        let base_path = Path::new(&self.config.storage.data_dir);
        let mode = self.config.storage.schema_migration_mode;
        let is_cluster = !self.cluster_peers.is_empty();
        let node_id = self.node_id.unwrap_or(0);

        log::info!("Validating model schemas...");
        if is_cluster {
            log::info!("   Cluster mode: schema changes will be synchronized");
        }

        let mut has_breaking_changes = false;

        for info in &self.model_infos {
            // Skip models without schema extractors
            let extractor = match &info.schema_extractor {
                Some(e) => e,
                None => {
                    log::debug!("   {} - no schema extractor, skipping", info.name);
                    continue;
                }
            };

            // Extract current schema
            let current_spec = extractor();

            // Load stored schema (if exists)
            let stored_spec = match load_schema_spec(&info.name, base_path) {
                Ok(spec) => spec,
                Err(e) => {
                    log::warn!("   {} - failed to load stored schema: {}", info.name, e);
                    None
                }
            };

            match stored_spec {
                Some(stored) => {
                    // Compare schemas
                    let changes = SchemaChangeDetector::detect_changes(&stored, &current_spec);

                    if changes.is_empty() {
                        log::info!(
                            "   {} - schema unchanged (v{})",
                            info.name,
                            current_spec.version
                        );

                        // In cluster mode, update local sync state
                        if is_cluster {
                            let mut state = self.schema_sync_state.write().await;
                            state.schemas.insert(info.name.clone(), current_spec.clone());
                        }
                    } else {
                        // Check if schema migrations are locked
                        {
                            let state = self.schema_sync_state.read().await;
                            if state.lock_status.is_locked() {
                                log::error!(
                                    "   {} - schema changes BLOCKED (migrations locked)",
                                    info.name
                                );
                                log::error!(
                                    "      Reason: {}",
                                    state.lock_status.reason.as_deref().unwrap_or("none")
                                );
                                log::error!("      Unlock via: POST /_admin/schema/unlock");
                                has_breaking_changes = true; // Will cause failure in strict mode
                                continue; // Skip this model, check next
                            }
                        }

                        log::warn!(
                            "   {} - {} schema change(s) detected:",
                            info.name,
                            changes.len()
                        );

                        for change in &changes {
                            let field = change.field_name.as_deref().unwrap_or("model");
                            log::warn!(
                                "      - {:?} on '{}' ({:?})",
                                change.change_type,
                                field,
                                change.migration_strategy
                            );

                            if change.requires_consensus {
                                has_breaking_changes = true;
                            }
                        }

                        // Handle based on mode and cluster status
                        if is_cluster {
                            // In cluster mode: create pending change for consensus
                            let pending = PendingSchemaChange::new(
                                info.name.clone(),
                                node_id,
                                changes.clone(),
                                current_spec.clone(),
                                Some(stored.clone()),
                            );

                            let mut state = self.schema_sync_state.write().await;
                            let policy = state.policy.clone();
                            let strategy = policy.strategy_for(&pending.overall_strategy);

                            match strategy {
                                crate::schema::VoteStrategy::AutoAccept => {
                                    log::info!(
                                        "      Cluster: auto-accepting {:?} change",
                                        pending.overall_strategy
                                    );
                                    state.schemas.insert(info.name.clone(), current_spec.clone());
                                    // Record in history
                                    let applied = AppliedSchemaChange {
                                        id: uuid::Uuid::new_v4(),
                                        model_name: info.name.clone(),
                                        changes: changes.clone(),
                                        applied_at: std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_millis()
                                            as u64,
                                        applied_by_node: node_id,
                                    };
                                    state.change_history.push(applied.clone());
                                    // Persist history to disk
                                    if let Err(e) =
                                        crate::schema::append_schema_history(&applied, base_path)
                                    {
                                        log::error!(
                                            "      Failed to persist schema history: {}",
                                            e
                                        );
                                    }
                                    // Also save locally
                                    if let Err(e) = save_schema_spec(&current_spec, base_path) {
                                        log::error!("      Failed to save updated schema: {}", e);
                                    }
                                }
                                crate::schema::VoteStrategy::Reject => {
                                    log::error!(
                                        "      Cluster: rejecting {:?} change (policy)",
                                        pending.overall_strategy
                                    );
                                    has_breaking_changes = true;
                                }
                                _ => {
                                    // Consensus or ManualApproval required
                                    log::info!("      Cluster: change requires {:?}", strategy);
                                    state.add_pending(pending);
                                    // Node should wait or be blocked until approval
                                    // Note: Blocking on pending approval is not yet supported
                                    log::warn!("      ⏳ Schema change pending approval - check /_admin/schema/pending");
                                }
                            }
                        } else {
                            // Non-cluster mode: behavior depends on migration mode
                            match mode {
                                SchemaMigrationMode::Strict => {
                                    // Will fail after logging all changes
                                }
                                SchemaMigrationMode::Auto => {
                                    // Save new schema (actual data migration not implemented yet)
                                    if let Err(e) = save_schema_spec(&current_spec, base_path) {
                                        log::error!("      Failed to save updated schema: {}", e);
                                    } else {
                                        log::info!(
                                            "      Schema updated to v{}",
                                            current_spec.version
                                        );
                                        // Record in history (non-cluster mode)
                                        let applied = AppliedSchemaChange {
                                            id: uuid::Uuid::new_v4(),
                                            model_name: info.name.clone(),
                                            changes: changes.clone(),
                                            applied_at: std::time::SystemTime::now()
                                                .duration_since(std::time::UNIX_EPOCH)
                                                .unwrap_or_default()
                                                .as_millis()
                                                as u64,
                                            applied_by_node: node_id,
                                        };
                                        let mut state = self.schema_sync_state.write().await;
                                        state.change_history.push(applied.clone());
                                        // Persist history to disk
                                        if let Err(e) = crate::schema::append_schema_history(
                                            &applied, base_path,
                                        ) {
                                            log::error!(
                                                "      Failed to persist schema history: {}",
                                                e
                                            );
                                        }
                                    }
                                }
                                SchemaMigrationMode::Manual => {
                                    // Create pending change requiring manual approval (even in standalone)
                                    let pending = PendingSchemaChange::new(
                                        info.name.clone(),
                                        node_id,
                                        changes.clone(),
                                        current_spec.clone(),
                                        Some(stored.clone()),
                                    );
                                    log::info!(
                                        "      Manual mode: change pending approval (id: {})",
                                        pending.id
                                    );
                                    log::warn!(
                                        "      ⏳ Approve via: POST /_admin/schema/approve/{}",
                                        pending.id
                                    );

                                    let mut state = self.schema_sync_state.write().await;
                                    state.add_pending(pending);
                                }
                                SchemaMigrationMode::Warn => {
                                    // Just log, already done
                                }
                            }
                        }
                    }
                }
                None => {
                    // First run - save initial schema
                    log::info!(
                        "   {} - first run, saving schema v{}",
                        info.name,
                        current_spec.version
                    );
                    if let Err(e) = save_schema_spec(&current_spec, base_path) {
                        log::error!("      Failed to save initial schema: {}", e);
                    }

                    // In cluster mode, update local sync state
                    if is_cluster {
                        let mut state = self.schema_sync_state.write().await;
                        state.schemas.insert(info.name.clone(), current_spec.clone());
                    }
                }
            }
        }

        // Fail in strict mode if breaking changes detected
        if mode == SchemaMigrationMode::Strict && has_breaking_changes {
            anyhow::bail!("Schema validation failed: breaking changes detected in strict mode");
        }

        log::info!("Schema validation complete");
        Ok(())
    }

    /// Start the server, running until `accept()` errors.
    ///
    /// Thin delegate over [`serve_with_graceful_shutdown`] using a
    /// `std::future::pending()` shutdown future, which never resolves — so the
    /// accept loop runs forever, byte-for-byte identical to the historical
    /// behavior. Existing callers are unaffected.
    ///
    /// [`serve_with_graceful_shutdown`]: Self::serve_with_graceful_shutdown
    pub async fn serve(self) -> Result<()> {
        self.serve_with_graceful_shutdown(std::future::pending::<()>()).await
    }

    /// Start the server, stopping the accept loop when `shutdown` resolves.
    ///
    /// Mirrors `axum::serve(...).with_graceful_shutdown(f)` and hyper's
    /// pattern. When `shutdown` completes, the server stops accepting new
    /// connections, gives already-accepted connections a bounded grace window
    /// to drain (see [`GRACEFUL_DRAIN_GRACE`]), then returns `Ok(())`.
    ///
    /// Downstream apps can pass a `watch`/`oneshot`/`CancellationToken`-driven
    /// future flipped on `ctrl_c`/SIGTERM, then join their own background
    /// workers after `.await` returns.
    ///
    /// [`GRACEFUL_DRAIN_GRACE`]: Self::GRACEFUL_DRAIN_GRACE
    pub async fn serve_with_graceful_shutdown<F>(mut self, shutdown: F) -> Result<()>
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        // Install the default tracing subscriber + log bridge FIRST, before
        // any boot-time `log::*` call (schema history load, validation,
        // session fail-fast, handler creation below). A boot failure in
        // those paths must be observable — with no subscriber installed,
        // those records would be silently dropped (PR #118 review). Same
        // try-semantics as the historical env_logger init: a logger the
        // caller installed earlier (e.g. RaftstoneLogger) wins untouched.
        init_default_tracing();

        // Load persisted schema history and lock status
        {
            use crate::schema::{load_lock_status, load_schema_history};
            use std::path::Path;

            let base_path = Path::new(&self.config.storage.data_dir);

            // Load history
            match load_schema_history(base_path) {
                Ok(history) => {
                    let mut state = self.schema_sync_state.write().await;
                    state.change_history = history.changes;
                    if !state.change_history.is_empty() {
                        log::info!(
                            "Loaded {} schema change(s) from history",
                            state.change_history.len()
                        );
                    }
                }
                Err(e) => {
                    log::warn!("Failed to load schema history: {}", e);
                }
            }

            // Load lock status
            match load_lock_status(base_path) {
                Ok(lock) => {
                    let mut state = self.schema_sync_state.write().await;
                    state.lock_status = lock;
                    if state.lock_status.is_locked() {
                        log::warn!("Schema migrations are LOCKED (persisted state)");
                    }
                }
                Err(e) => {
                    log::warn!("Failed to load schema lock status: {}", e);
                }
            }
        }

        // Schema validation for models with schema extractors
        if self.config.storage.schema_validation_enabled {
            self.validate_schemas().await?;
        }

        // Issue #80 — fail-fast at boot when the operator opted into the
        // `with_models_require_session(true)` gate but the registered
        // session manager has a shape the gate can't recognize at request
        // time (`has_valid_session` in `http/declarative.rs` only handles
        // `Arc<PersistentSessionStore>` and
        // `Arc<SessionManager<PersistentSessionStore>>`).
        //
        // The classic mis-wire is `SessionManager::new(arc_store)` where
        // `arc_store: Arc<PersistentSessionStore>` — the generic resolves
        // to `S = Arc<PersistentSessionStore>` and the stored type
        // becomes `Arc<SessionManager<Arc<PersistentSessionStore>>>` (a
        // double-Arc shape the downcast misses). Pre-fix, every request
        // 401'd silently. Now we refuse to start, point the operator at
        // the right constructor (`SessionManager::from_arc`), and never
        // bind the port.
        //
        // Only fires when the gate is actually opt-in (`models_require_session`)
        // AND at least one model registration is opted-in via the
        // simple-CRUD path. `with_model_full` registrations carry their
        // own RBAC story and the gate doesn't cover them, so a mismatched
        // shape doesn't cause a silent auth-bypass for them.
        if self.models_require_session && self.model_infos.iter().any(|i| i.require_session_applies)
        {
            if let Some(ref store_any) = self.session_manager {
                // Single source of truth for which shapes the gate
                // recognizes. Defined in `lithair-core/src/session/mod.rs`
                // and consumed both here and in
                // `http/declarative.rs::has_valid_session`. Adding a new
                // supported shape in one place automatically extends the
                // other — no more drift between constructor surface and
                // runtime downcast (the original cause of issue #80).
                if crate::session::RecognizedSessionStore::recognize(store_any).is_none() {
                    // The stored `Arc<dyn Any>` doesn't carry its concrete
                    // type name as a string. We at least preserve the
                    // `TypeId` so an operator with access to a debug
                    // build can correlate it.
                    let actual_type_id = (**store_any).type_id();
                    anyhow::bail!(
                        "Refusing to start: `with_models_require_session(true)` is set \
                         but the registered session store has an unrecognized shape \
                         (TypeId = {:?}). The gate recognizes the built-in stores \
                         (`PersistentSessionStore`, `MemorySessionStore`), each either \
                         raw or wrapped in a `SessionManager`. An unrecognized shape \
                         usually means `SessionManager::new(arc_store)` was called with \
                         an already-`Arc`-wrapped store, producing a double-`Arc` shape \
                         that silently 401s every request. Use \
                         `SessionManager::from_arc(arc_store)` instead, or pass the \
                         store by value to `SessionManager::new`. See issue #80.",
                        actual_type_id
                    );
                }
            } else {
                // Flag is on but no session store was ever wired. This is
                // already a noisy 401-on-everything situation
                // (`has_valid_session` returns false unconditionally),
                // but pre-issue-#80 we shipped it silently. Failing fast
                // here matches the operator's intent: they asked for
                // gating, the framework cannot honor it, refuse to start.
                anyhow::bail!(
                    "Refusing to start: `with_models_require_session(true)` is set \
                     but no session store was registered. Add `.with_sessions(...)` \
                     before `.serve()`, or remove the require-session flag."
                );
            }
        }

        // Create model handlers from factories
        for info in &self.model_infos {
            log::info!("Creating handler for model: {}", info.name);
            match (info.factory)(info.data_path.clone()).await {
                Ok(mut handler) => {
                    // Wire SSE broadcaster into each model handler. Works
                    // through `&self` since #91 — `DeclarativeHttpHandler`
                    // stores the broadcaster in a `OnceLock` and the trait
                    // method takes `&self`. The previous `Arc::get_mut`
                    // dance + warn-on-shared-Arc is no longer needed; any
                    // shared Arc still receives the broadcaster cleanly.
                    if let Some(ref broadcaster) = self.sse_broadcaster {
                        handler.set_sse_broadcaster(Arc::clone(broadcaster));
                    }

                    // Wire the session store into every model handler.
                    //
                    // Pre-issue-#78, `with_model(...)` never threaded the
                    // session store through (only `with_model_full` did), so
                    // even when sessions were configured the simple-CRUD
                    // path had no way to look one up. We now plumb it here,
                    // uniformly, after the factory has produced the handler.
                    // This makes builder-method ordering irrelevant.
                    //
                    // `with_model_full` may have already attached its own
                    // store via the owned setter; this call overwrites it
                    // with the builder-level store. That is intentional —
                    // the builder-level configuration is authoritative
                    // (any RBAC checker provided via `with_model_full`
                    // continues to use the same shared store).
                    //
                    // We fail-fast (bail) rather than warn here: if a
                    // factory hands back a shared `Arc`, the session store
                    // wiring would silently drop and RBAC / the #78 gate
                    // would both become no-ops. That's a security-relevant
                    // silent failure — better to refuse to start than
                    // serve unauthenticated traffic that the operator
                    // believed to be gated. (Built-in factories return a
                    // fresh `Arc::new(handler)` so this never triggers in
                    // practice; the bail is a guard against external
                    // factory implementations that misbehave.)
                    if let Some(ref store) = self.session_manager {
                        if let Some(h) = Arc::get_mut(&mut handler) {
                            h.set_session_store_any(Arc::clone(store));
                        } else {
                            anyhow::bail!(
                                "Could not wire session store for model '{}': handler Arc has multiple strong references — refusing to start to avoid a silent auth-bypass",
                                info.name
                            );
                        }
                    }

                    // Apply the issue #78 require-session flag — but only
                    // to registrations that opted in via the simple-CRUD
                    // path. `with_model_full(...)` registrations carry
                    // their own RBAC story (see PermissionChecker) and the
                    // flag is intentionally scoped to NOT cover them.
                    if self.models_require_session && info.require_session_applies {
                        if let Some(h) = Arc::get_mut(&mut handler) {
                            h.set_require_session(true);
                        } else {
                            anyhow::bail!(
                                "Could not enable require-session for model '{}': handler Arc has multiple strong references — refusing to start because the operator-requested gate would silently not engage",
                                info.name
                            );
                        }
                    }
                    let mut models = self.models.write().await;
                    models.push(ModelRegistration {
                        name: info.name.clone(),
                        base_path: info.base_path.clone(),
                        data_path: info.data_path.clone(),
                        handler,
                        schema_extractor: info.schema_extractor.clone(),
                    });
                    log::info!("Handler created for {}", info.name);
                }
                Err(e) => {
                    log::error!("Failed to create handler for {}: {}", info.name, e);
                    return Err(e.context(format!("Failed to create handler for {}", info.name)));
                }
            }
        }
        self.model_infos.clear(); // Clear infos, we have the models now

        // Issue #86: apply the session-presence gate to every model
        // registered via `LithairServerBuilder::with_handler(...)`. Each
        // closure was pushed at registration time with a captured
        // `Arc::clone(&handler)`, so flipping the flag here propagates to
        // every CRUD route closure that was registered through `with_route`
        // — they all dispatch into `handler.handle_request(...)`, which
        // reads `require_session` via `AtomicBool::load`.
        //
        // Pre-fix, this path silently never gated, so a consumer who
        // switched from `.with_model::<T>(...)` to `.with_handler(...)`
        // (to gain a programmatic handle on the handler) lost the gate
        // without warning. See issue #86 for the original repro.
        //
        // Note: we do NOT bail on a missing session store here. The
        // boot-time fail-fast check above (line ~707) already covers the
        // mis-wire case for the `with_model` path; for `with_handler`,
        // the responsibility for wiring the session store onto the
        // handler currently sits with the caller (they constructed the
        // handler externally). Without a store, `has_valid_session`
        // returns `false` on every request, which combined with the gate
        // means the route returns 401 unconditionally — failing closed
        // is the safe direction here.
        let gates = std::mem::take(&mut self.external_handler_gates);
        if self.models_require_session {
            for gate in &gates {
                gate(true);
            }
        }

        // Issue #91: install the builder-level SSE broadcaster onto every
        // handler registered via `with_handler(...)` (including via
        // `with_model_ref`, which delegates to it). Mirrors the wiring loop
        // above for the factory `with_model` path at line ~774 — same
        // production semantics (one broadcaster per server, shared across
        // all model handlers), different installation surface (interior
        // mutability via `OnceLock` instead of `Arc::get_mut`, because
        // `with_handler` consumers already hold an Arc clone).
        //
        // The closure is only invoked when the builder has a broadcaster,
        // i.e. when `.with_sse(true)` was called on the builder. Without
        // a broadcaster, every wiring is a no-op and the handlers stay
        // with their default empty `OnceLock` — backward-compat for every
        // `with_handler` consumer who never opted into SSE.
        let sse_wirings = std::mem::take(&mut self.external_handler_sse_wirings);
        if let Some(ref broadcaster) = self.sse_broadcaster {
            for wire in &sse_wirings {
                wire(Arc::clone(broadcaster));
            }
        }

        // Issue #69: spawn one auto-compaction task per registered model
        // when the feature is enabled. The task body mirrors the one
        // covered in `tests/auto_compaction_test.rs::spawn_auto_compaction`
        // — keep them in sync if either is touched.
        //
        // Lifecycle matches the existing background-flusher pattern in
        // `DeclarativeHttpHandler::new` (line ~180): spawned and forgotten,
        // runtime shutdown aborts. We deliberately do NOT hold JoinHandles
        // on `LithairServer` — that would diverge from the flusher's
        // lifecycle and touch async writers across several files.
        //
        // A graceful shutdown signal now exists
        // (`serve_with_graceful_shutdown`, issue #112): the accept loop stops
        // and in-flight HTTP connections get a grace window. Draining these
        // *internal* tasks (auto-compaction, the WAL flusher, async writers)
        // via tracked JoinHandles is still deferred — it spans 3+ files and
        // is tracked as a follow-up. Both code paths should grow JoinHandle
        // tracking together when that lands.
        //
        // Tracing init happens at the very top of this function (PR #118
        // review) so logs emitted by the tasks spawned below — and by every
        // boot step above — are always observable (issue #69 follow-up
        // originally pinned the init before this spawn loop; moving it
        // earlier preserves that property a fortiori).

        if let Some(cfg) = self.auto_compaction {
            let models = self.models.read().await;
            for reg in models.iter() {
                // Skip handlers that don't event-source — their `compact()`
                // is a no-op but we'd still pay the per-tick lock acquire.
                // The event-store handle is also what we read `event_count()`
                // from on each tick; without it, no threshold check is
                // meaningful.
                let Some(event_store) = reg.handler.event_store_arc() else {
                    log::debug!(
                        "Auto-compaction: model '{}' has no EventStore, skipping",
                        reg.name
                    );
                    continue;
                };
                let handler = Arc::clone(&reg.handler);
                let model_name = reg.name.clone();
                log::info!(
                    "Auto-compaction enabled for model '{}': threshold={}, interval={:?}",
                    model_name,
                    cfg.events_threshold,
                    cfg.check_interval
                );
                tokio::spawn(async move {
                    let mut ticker = tokio::time::interval(cfg.check_interval);
                    // `MissedTickBehavior::Skip` — if the system is under
                    // load and several check intervals elapse between
                    // `.tick()` resolutions, fire once and align to the
                    // next interval (don't burst-fire a compaction check
                    // multiple times in a row). Default `Burst` would
                    // hammer a maintenance task that should be paced.
                    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                    // Skip the immediate first tick — `tokio::time::interval`
                    // fires immediately on the first `.tick()` which would
                    // be a spurious read of an empty just-started store.
                    ticker.tick().await;
                    loop {
                        ticker.tick().await;
                        let needs_compaction = {
                            let store = event_store.read().await;
                            store.event_count() > cfg.events_threshold
                        };
                        if !needs_compaction {
                            continue;
                        }
                        log::info!(
                            "Auto-compaction: model '{}' crossed threshold {}, compacting (snapshot + truncate)",
                            model_name,
                            cfg.events_threshold
                        );
                        // Delegate to the handler's `compact()` — it owns
                        // the snapshot+truncate atomicity guarantee. The
                        // server-side loop intentionally does NOT touch
                        // `EventStore::truncate_events()` directly: that
                        // path caused the data-loss bug Gemini flagged on
                        // PR #84 (truncate without snapshot = next replay
                        // sees an empty log = state lost).
                        if let Err(e) = handler.compact().await {
                            log::warn!(
                                "Auto-compaction: model '{}' compact() failed: {}",
                                model_name,
                                e
                            );
                        }
                    }
                });
            }
        }

        // Validate configuration
        self.config.validate()?;

        log::info!("Starting Lithair Server");
        log::info!("   Port: {}", self.config.server.port);
        log::info!("   Host: {}", self.config.server.host);
        log::info!(
            "   Sessions: {}",
            if self.config.sessions.enabled { "enabled" } else { "disabled" }
        );
        log::info!("   RBAC: {}", if self.config.rbac.enabled { "enabled" } else { "disabled" });
        log::info!("   Admin: {}", if self.config.admin.enabled { "enabled" } else { "disabled" });
        log::info!("   Models: {}", self.models.read().await.len());
        log::info!("   Custom routes: {}", self.custom_routes.len());

        // Load frontend assets - support both old config and new multi-frontend approach
        let mut frontends_to_load = Vec::new();

        // Add legacy frontend config if enabled
        if self.config.frontend.enabled {
            if let Some(ref static_dir) = self.config.frontend.static_dir {
                frontends_to_load.push(("/".to_string(), static_dir.clone()));
            }
        }

        // Add new multi-frontend configs
        frontends_to_load.extend(self.frontend_configs.clone());

        // Load each frontend
        for (route_prefix, static_dir) in frontends_to_load {
            log::info!("Loading frontend at '{}' from {}...", route_prefix, static_dir);

            // Create unique host_id from route_prefix
            let host_id = if route_prefix == "/" {
                "default".to_string()
            } else {
                route_prefix.trim_matches('/').replace('/', "_")
            };

            // Create FrontendEngine (SCC2 lock-free with event sourcing)
            match crate::frontend::FrontendEngine::new(&host_id, "./data/frontend").await {
                Ok(engine) => {
                    log::info!("   FrontendEngine created (host_id: {})", host_id);

                    // Load assets into memory
                    match engine.load_directory(&static_dir).await {
                        Ok(count) => {
                            log::info!("   {} assets loaded (40M+ ops/sec)", count);
                            self.frontend_engines.insert(route_prefix.clone(), Arc::new(engine));
                        }
                        Err(e) => {
                            log::warn!("   Could not load frontend assets: {}", e);
                        }
                    }
                }
                Err(e) => {
                    log::warn!("   Could not create frontend engine: {}", e);
                }
            }
        }

        // Load per-vhost frontends (host-header based routing, see
        // `lithair/lithair#30`). Each vhost gets its own set of
        // FrontendEngines keyed by route prefix, exactly like the
        // host-agnostic `frontend_engines` above. The resulting map is
        // stored under the normalized host in `vhost_frontend_router`.
        if !self.vhost_frontend_configs.is_empty() {
            use crate::app::builder::VhostScope;

            // Group configs by scope so we iterate each vhost once.
            // `serve` owns `mut self` and `vhost_frontend_configs` is not
            // used again after startup, so take it instead of cloning.
            let mut by_scope: std::collections::HashMap<VhostScope, Vec<(String, String)>> =
                std::collections::HashMap::new();
            for (scope, prefix, dir) in std::mem::take(&mut self.vhost_frontend_configs) {
                by_scope.entry(scope).or_default().push((prefix, dir));
            }

            for (scope, entries) in by_scope {
                let scope_label = match &scope {
                    VhostScope::Host(h) => h.clone(),
                    VhostScope::Default => "<default>".to_string(),
                };
                log::info!("Loading vhost '{}' ({} frontends)", scope_label, entries.len());

                let mut engines: std::collections::HashMap<
                    String,
                    Arc<crate::frontend::FrontendEngine>,
                > = std::collections::HashMap::new();

                // Injective byte-level encoder used for host_id segments:
                // ASCII alphanumerics and '-' pass through, everything else
                // becomes `_<hex>`. This makes the (vhost, prefix) → host_id
                // mapping collision-free — otherwise pairs like ("foo.bar",
                // "/") and ("foo_bar", "/"), or "/a/b" and "/a_b", would
                // collapse onto the same on-disk SCC2 store.
                let stable_host_id_segment = |input: &str| -> String {
                    let mut out = String::with_capacity(input.len() * 3);
                    for b in input.bytes() {
                        match b {
                            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' => out.push(b as char),
                            _ => {
                                use std::fmt::Write as _;
                                let _ = write!(&mut out, "_{b:02x}");
                            }
                        }
                    }
                    out
                };

                for (route_prefix, static_dir) in entries {
                    // Compose a stable host_id unique to (vhost, prefix)
                    // so the two frontends don't clobber each other's
                    // SCC2 event store.
                    let prefix_segment = stable_host_id_segment(&route_prefix);
                    let scope_segment = match &scope {
                        VhostScope::Host(h) => stable_host_id_segment(h),
                        VhostScope::Default => "_default".to_string(),
                    };
                    let host_id = format!("vhost_{}__{}", scope_segment, prefix_segment);

                    match crate::frontend::FrontendEngine::new(&host_id, "./data/frontend").await {
                        Ok(engine) => match engine.load_directory(&static_dir).await {
                            Ok(count) => {
                                log::info!(
                                    "   [{}] {} → {} ({} assets)",
                                    scope_label,
                                    route_prefix,
                                    static_dir,
                                    count
                                );
                                engines.insert(route_prefix, Arc::new(engine));
                            }
                            Err(e) => {
                                log::warn!(
                                    "   [{}] could not load {} -> {}: {}",
                                    scope_label,
                                    route_prefix,
                                    static_dir,
                                    e
                                );
                            }
                        },
                        Err(e) => {
                            log::warn!(
                                "   [{}] could not create frontend engine for {}: {}",
                                scope_label,
                                route_prefix,
                                e
                            );
                        }
                    }
                }

                match scope {
                    VhostScope::Host(h) => {
                        self.vhost_frontend_router.insert(h, engines);
                    }
                    VhostScope::Default => {
                        self.vhost_frontend_router.set_default(engines);
                    }
                }
            }
        }

        // Log HTTP features
        if self.firewall_config.is_some() {
            log::info!("   Firewall enabled");
        }
        if self.anti_ddos_config.is_some() {
            log::info!("   Anti-DDoS protection enabled");
        }

        // Initialize Raft cluster if configured
        if self.config.raft.enabled && !self.cluster_peers.is_empty() {
            if let Some(node_id) = self.node_id {
                let port = self.config.server.port;

                log::info!("Initializing Raft cluster...");
                log::info!("   Node ID: {}", node_id);
                log::info!("   Peers: {:?}", self.cluster_peers);
                log::info!("   Raft path: {}", self.config.raft.path);
                log::info!(
                    "   Raft auth: {}",
                    if self.config.raft.auth_required { "enabled" } else { "disabled" }
                );

                let raft_state =
                    Arc::new(RaftLeadershipState::new(node_id, port, self.cluster_peers.clone()));

                if raft_state.is_leader() {
                    log::info!("THIS NODE IS THE LEADER");
                } else {
                    log::info!(
                        "This node is a FOLLOWER (leader port: {})",
                        raft_state.get_leader_port()
                    );
                }

                self.raft_state = Some(raft_state);

                // Initialize replication batcher with peers
                if let Some(ref batcher) = self.replication_batcher {
                    batcher.initialize(&self.cluster_peers).await;
                    log::info!(
                        "Replication batcher initialized with {} peers",
                        self.cluster_peers.len()
                    );
                }

                // ── WAL REPLAY ──────────────────────────────────────────────
                // On restart, the WAL contains all committed operations from
                // before the crash. We replay them into the ConsensusLog to
                // restore the node's state without losing data.
                if let (Some(ref wal), Some(ref consensus_log)) = (&self.wal, &self.consensus_log) {
                    match wal.read_all() {
                        Ok(entries) if !entries.is_empty() => {
                            let count = consensus_log.replay_from_wal_entries(entries).await;
                            log::info!(
                                "WAL replay: restored {} entries (term={}, commit_index={})",
                                count,
                                consensus_log.current_term(),
                                consensus_log.commit_index(),
                            );
                        }
                        Ok(_) => {
                            log::info!("WAL replay: no entries to restore (fresh node)");
                        }
                        Err(e) => {
                            log::warn!("WAL replay failed: {} — starting with empty log", e);
                        }
                    }
                }

                // Start WAL background flush task (group commit)
                if let Some(ref wal) = self.wal {
                    let _flush_handle = wal.spawn_flush_task();
                    log::info!("WAL group commit enabled (flush interval: 5ms)");
                }

                // Log snapshot status
                if self.snapshot_manager.is_some() {
                    log::info!("Snapshot manager enabled for resync");
                }

                // ── BACKGROUND REPLICATION TASK ────────────────────────────
                //
                // This long-running task handles three responsibilities:
                //
                // 1. CATCH-UP: Periodically checks each follower's last_replicated_index
                //    and sends only the missing entries. This is how lagging followers
                //    eventually converge with the leader — the write path only sends
                //    the current entry, this task fills in any gaps.
                //
                // 2. HEARTBEAT: Every ~1.7s (election_timeout/3), sends an empty
                //    AppendEntriesRequest to all followers. This prevents followers
                //    from starting unnecessary elections during idle periods.
                //
                // 3. RESYNC: When a follower is detected as desynced (>1000 entries
                //    behind, or unresponsive for >30s with pending work), triggers
                //    a full snapshot transfer instead of incremental catch-up.
                //
                // Runs on a 100ms tick interval.
                if let Some(ref batcher) = self.replication_batcher {
                    let batcher_clone = Arc::clone(batcher);
                    let peers = self.cluster_peers.clone();
                    let consensus_log = self.consensus_log.clone();
                    let node_id = self.node_id.unwrap_or(0);
                    let self_port = self.config.server.port;
                    let raft_state = self.raft_state.clone();
                    let snapshot_manager = self.snapshot_manager.clone();
                    let models = Arc::clone(&self.models);
                    let replication_config = self.config.replication.clone();
                    let resync_stats = Arc::clone(&self.resync_stats);
                    let wal_for_resync = self.wal.clone();

                    tokio::spawn(async move {
                        use std::collections::HashMap;
                        use std::time::Duration;
                        use tokio::time::interval;

                        let mut ticker = interval(Duration::from_millis(100)); // Check every 100ms
                        let mut _catchup_counter = 0u64; // Reserved for future use
                        let mut resync_counter = 0u64; // For periodic snapshot resync
                        let mut heartbeat_counter = 0u64; // For leader heartbeat

                        // Heartbeat interval: election_timeout / 3, derived from config
                        let heartbeat_ticks = raft_state
                            .as_ref()
                            .map(|s| (s.election_timeout.as_millis() as u64 / 3 + 50) / 100)
                            .unwrap_or(17);

                        // Shared HTTP client for background tasks (reuses connection pool)
                        let bg_client = reqwest::Client::builder()
                            .timeout(Duration::from_secs(5))
                            .build()
                            .unwrap_or_else(|_| reqwest::Client::new());

                        // Track last resync time per follower for cooldown
                        let mut last_resync: HashMap<String, std::time::Instant> = HashMap::new();

                        // Calculate resync check ticks from config (100ms base interval)
                        let resync_check_ticks = replication_config.resync_check_interval_ms / 100;

                        loop {
                            ticker.tick().await;

                            // Only leader should do background replication
                            if let Some(ref state) = raft_state {
                                if !state.is_leader() {
                                    continue;
                                }
                            }

                            let consensus_log_ref = match &consensus_log {
                                Some(log) => log,
                                None => continue,
                            };

                            let term = consensus_log_ref.current_term();
                            let commit_index = consensus_log_ref.commit_index();

                            // Increment counters
                            _catchup_counter += 1;
                            resync_counter += 1;

                            // === SNAPSHOT-BASED RESYNC FOR DESYNCED FOLLOWERS ===
                            // Check based on configurable interval (default: 1 second = 10 ticks)
                            if resync_counter >= resync_check_ticks {
                                resync_counter = 0;

                                // Get list of desynced followers
                                let desynced =
                                    batcher_clone.get_desynced_followers(commit_index).await;

                                if !desynced.is_empty() {
                                    if let Some(ref snap_mgr_inner) = snapshot_manager {
                                        // Filter out followers that are in cooldown
                                        let cooldown_duration = Duration::from_secs(
                                            replication_config.resync_cooldown_secs,
                                        );
                                        let now = std::time::Instant::now();
                                        let eligible_for_resync: Vec<_> = desynced
                                            .into_iter()
                                            .filter(|peer| {
                                                match last_resync.get(peer) {
                                                    Some(last_time) => {
                                                        now.duration_since(*last_time)
                                                            >= cooldown_duration
                                                    }
                                                    None => true, // Never resynced, eligible
                                                }
                                            })
                                            .collect();

                                        if !eligible_for_resync.is_empty() {
                                            log::info!(
                                                "Found {} desynced followers eligible for resync",
                                                eligible_for_resync.len()
                                            );

                                            let snapshot_mgr = snap_mgr_inner.clone();
                                            let models_clone = Arc::clone(&models);
                                            let batcher_for_resync = Arc::clone(&batcher_clone);

                                            // Create snapshot if needed (only once per resync cycle)
                                            if let Err(e) = Self::create_snapshot_from_models(
                                                &models_clone,
                                                &snapshot_mgr,
                                                term,
                                                commit_index,
                                            )
                                            .await
                                            {
                                                log::warn!(
                                                    "Failed to create snapshot for resync: {}",
                                                    e
                                                );
                                            } else {
                                                // Track snapshot creation
                                                resync_stats.record_snapshot_created();

                                                // ── WAL COMPACTION ─────────────────────────
                                                // Now that the snapshot captures state up to
                                                // commit_index, we can safely remove older WAL
                                                // entries. This prevents the WAL from growing
                                                // unbounded over time.
                                                if let Some(ref wal_for_compact) = wal_for_resync {
                                                    match wal_for_compact.compact(commit_index).await {
                                                        Ok(0) => {}
                                                        Ok(n) => log::info!(
                                                            "WAL compacted: removed {} entries (snapshot covers up to index {})",
                                                            n, commit_index
                                                        ),
                                                        Err(e) => log::warn!("WAL compaction failed: {}", e),
                                                    }
                                                }

                                                // Send snapshot to each desynced follower (in parallel, with configurable rate limit)
                                                let max_concurrent =
                                                    replication_config.max_concurrent_resyncs;
                                                let snapshot_timeout_secs =
                                                    replication_config.snapshot_send_timeout_secs;

                                                for peer in eligible_for_resync
                                                    .into_iter()
                                                    .take(max_concurrent)
                                                {
                                                    // Mark as resyncing with current timestamp
                                                    last_resync.insert(peer.clone(), now);

                                                    let peer_clone = peer.clone();
                                                    let snapshot_mgr_clone = snapshot_mgr.clone();
                                                    let batcher_resync =
                                                        Arc::clone(&batcher_for_resync);
                                                    let stats_clone = Arc::clone(&resync_stats);

                                                    // Track send attempt
                                                    resync_stats.record_send_attempt(commit_index);

                                                    tokio::spawn(async move {
                                                        log::info!(
                                                    "Sending snapshot to desynced follower: {}",
                                                    peer_clone
                                                );

                                                        match Self::send_snapshot_to_follower_with_timeout(
                                                    &peer_clone,
                                                    &snapshot_mgr_clone,
                                                    snapshot_timeout_secs,
                                                )
                                                .await
                                                {
                                                    Ok(()) => {
                                                        log::info!(
                                                            "Snapshot installed on {}",
                                                            peer_clone
                                                        );
                                                        // Track success
                                                        stats_clone.record_send_success();
                                                        // Reset follower health after successful resync
                                                        if let Some(follower) = batcher_resync
                                                            .get_follower(&peer_clone)
                                                            .await
                                                        {
                                                            follower.record_success(0, 0).await;
                                                            // Reset to healthy
                                                        }
                                                    }
                                                    Err(e) => {
                                                        log::error!(
                                                            "Snapshot send to {} failed: {}",
                                                            peer_clone,
                                                            e
                                                        );
                                                        // Track failure
                                                        stats_clone.record_send_failure();
                                                    }
                                                }
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // === INCREMENTAL CATCH-UP FOR LAGGING FOLLOWERS ===
                            // Only send entries that followers are actually missing
                            if commit_index > 0 {
                                // Get health status to skip desynced followers
                                let health_summary = batcher_clone.get_health_summary().await;

                                for peer in &peers {
                                    // Skip desynced followers - they'll get snapshots instead
                                    if let Some(health) = health_summary.get(peer) {
                                        if *health == crate::cluster::replication_batcher::FollowerHealth::Desynced {
                                        log::debug!("Skipping desynced follower {} (will use snapshot)", peer);
                                        continue;
                                    }
                                    }

                                    // Get follower's last replicated index
                                    let follower_index = if let Some(follower) =
                                        batcher_clone.get_follower(peer).await
                                    {
                                        follower
                                            .last_replicated_index
                                            .load(std::sync::atomic::Ordering::SeqCst)
                                    } else {
                                        0
                                    };

                                    // Skip if follower is already in sync
                                    if follower_index >= commit_index {
                                        continue;
                                    }

                                    // Only get entries the follower is missing
                                    let missing_entries = consensus_log_ref
                                        .get_entries_from(follower_index + 1)
                                        .await;
                                    if missing_entries.is_empty() {
                                        continue;
                                    }

                                    let peer = peer.clone();
                                    let entries = missing_entries;
                                    let batcher = Arc::clone(&batcher_clone);
                                    let commit = commit_index;

                                    tokio::spawn(async move {
                                        let client = reqwest::Client::builder()
                                            .timeout(Duration::from_secs(5))
                                            .build()
                                            .unwrap_or_else(|_| reqwest::Client::new());

                                        let request =
                                            crate::cluster::consensus_log::AppendEntriesRequest {
                                                term,
                                                leader_id: node_id,
                                                leader_port: self_port,
                                                prev_log_index: 0,
                                                prev_log_term: 0,
                                                entries: entries.clone(),
                                                leader_commit: commit,
                                            };

                                        let start = std::time::Instant::now();
                                        let url = format!("http://{}/_raft/append", peer);

                                        match client.post(&url).json(&request).send().await {
                                            Ok(resp) if resp.status().is_success() => {
                                                let latency = start.elapsed().as_millis() as u64;
                                                let last_index = entries
                                                    .last()
                                                    .map(|e| e.log_id.index)
                                                    .unwrap_or(0);
                                                batcher
                                                    .record_success(&peer, last_index, latency)
                                                    .await;
                                                log::debug!(
                                                    "Background catch-up: {} entries to {} ({}ms)",
                                                    entries.len(),
                                                    peer,
                                                    latency
                                                );
                                            }
                                            Ok(resp) => {
                                                log::debug!(
                                                    "Background catch-up to {} failed: {}",
                                                    peer,
                                                    resp.status()
                                                );
                                                batcher.record_failure(&peer).await;
                                            }
                                            Err(e) => {
                                                log::debug!(
                                                    "Background catch-up to {} error: {}",
                                                    peer,
                                                    e
                                                );
                                                batcher.record_failure(&peer).await;
                                            }
                                        }
                                    });
                                }
                            }

                            // === LEADER HEARTBEAT ===
                            // Send empty AppendEntriesRequest to all followers periodically
                            // to prevent them from starting unnecessary elections.
                            // Includes the leader's last log index/term so followers can
                            // detect log divergence (Raft safety property).
                            heartbeat_counter += 1;
                            if heartbeat_counter >= heartbeat_ticks {
                                heartbeat_counter = 0;

                                let last_log_index = consensus_log_ref.last_index().await;
                                let last_log_term = consensus_log_ref
                                    .last_entry()
                                    .await
                                    .map_or(0, |e| e.log_id.term);

                                for peer in &peers {
                                    let peer = peer.clone();
                                    let client = bg_client.clone();
                                    let heartbeat_request =
                                        crate::cluster::consensus_log::AppendEntriesRequest {
                                            term,
                                            leader_id: node_id,
                                            leader_port: self_port,
                                            prev_log_index: last_log_index,
                                            prev_log_term: last_log_term,
                                            entries: vec![], // Empty = heartbeat
                                            leader_commit: commit_index,
                                        };

                                    tokio::spawn(async move {
                                        let url = format!("http://{}/_raft/append", peer);
                                        let _ =
                                            client.post(&url).json(&heartbeat_request).send().await;
                                    });
                                }
                            }

                            // Normal batch processing for new entries
                            if !batcher_clone.should_send_batch().await {
                                continue;
                            }

                            let batch = batcher_clone.take_batch().await;
                            if batch.is_empty() {
                                continue;
                            }

                            // Send batch to all peers
                            for peer in &peers {
                                let peer = peer.clone();
                                let entries = batch.clone();
                                let batcher = Arc::clone(&batcher_clone);
                                let max_entry_index =
                                    entries.iter().map(|e| e.log_id.index).max().unwrap_or(0);

                                tokio::spawn(async move {
                                    let client = reqwest::Client::builder()
                                        .timeout(Duration::from_secs(5))
                                        .build()
                                        .unwrap_or_else(|_| reqwest::Client::new());

                                    let request =
                                        crate::cluster::consensus_log::AppendEntriesRequest {
                                            term,
                                            leader_id: node_id,
                                            leader_port: self_port,
                                            prev_log_index: 0,
                                            prev_log_term: 0,
                                            entries: entries.clone(),
                                            leader_commit: max_entry_index,
                                        };

                                    let start = std::time::Instant::now();
                                    let url = format!("http://{}/_raft/append", peer);

                                    match client.post(&url).json(&request).send().await {
                                        Ok(resp) if resp.status().is_success() => {
                                            let latency = start.elapsed().as_millis() as u64;
                                            let last_index =
                                                entries.last().map(|e| e.log_id.index).unwrap_or(0);
                                            batcher
                                                .record_success(&peer, last_index, latency)
                                                .await;
                                            log::debug!(
                                                "Background replicated {} entries to {} ({}ms)",
                                                entries.len(),
                                                peer,
                                                latency
                                            );
                                        }
                                        Ok(resp) => {
                                            log::warn!(
                                                "Background replication to {} failed: {}",
                                                peer,
                                                resp.status()
                                            );
                                            batcher.record_failure(&peer).await;
                                        }
                                        Err(e) => {
                                            log::warn!(
                                                "Background replication to {} error: {}",
                                                peer,
                                                e
                                            );
                                            batcher.record_failure(&peer).await;
                                        }
                                    }
                                });
                            }
                        }
                    });
                    log::info!("Background replication task started");
                }
            }
        }

        // Build server address
        let addr = format!("{}:{}", self.config.server.host, self.config.server.port);

        // Build optional TLS acceptor
        #[cfg(feature = "tls")]
        let tls_acceptor =
            match (&self.config.server.tls_cert_path, &self.config.server.tls_key_path) {
                (Some(cert_path), Some(key_path)) => {
                    let certs = load_tls_certs(cert_path)?;
                    let key = load_tls_key(key_path)?;

                    // Log certificate fingerprint for verification
                    if let Some(leaf_cert) = certs.first() {
                        let hash = sha2::Sha256::digest(leaf_cert.as_ref());
                        log::info!("TLS certificate SHA-256: {:x}", hash);
                    }

                    let tls_config = rustls::ServerConfig::builder()
                        .with_no_client_auth()
                        .with_single_cert(certs, key)
                        .context("Invalid TLS certificate/key pair")?;
                    Some(tokio_rustls::TlsAcceptor::from(Arc::new(tls_config)))
                }
                _ => None,
            };
        #[cfg(not(feature = "tls"))]
        let tls_acceptor: Option<()> = {
            if self.config.server.tls_cert_path.is_some()
                || self.config.server.tls_key_path.is_some()
            {
                log::warn!("TLS certificate/key configured but the 'tls' feature is not enabled");
                log::warn!("   Add `features = [\"tls\"]` to your lithair-core dependency");
            }
            None
        };
        let tls_active = tls_acceptor.is_some();

        // Start server
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .with_context(|| format!("Failed to bind to {}", addr))?;

        let scheme = if tls_active { "https" } else { "http" };
        log::info!("Server listening on {}://{}", scheme, addr);

        // Start Raft background tasks if cluster mode enabled
        if let Some(ref raft_state) = self.raft_state {
            let state_clone = Arc::clone(raft_state);
            let peers = self.cluster_peers.clone();
            let raft_config = self.config.raft.clone();

            if raft_state.is_leader() {
                // Leader: send heartbeats to followers
                tokio::spawn(async move {
                    use reqwest::Client as HttpClient;
                    use std::time::Duration;
                    use tokio::time::sleep;

                    let client = HttpClient::builder()
                        .timeout(Duration::from_secs(2))
                        .build()
                        .unwrap_or_else(|_| HttpClient::new());

                    let heartbeat_interval =
                        Duration::from_secs(raft_config.heartbeat_interval_secs);

                    loop {
                        sleep(heartbeat_interval).await;

                        if !state_clone.is_leader() {
                            log::info!("No longer leader, stopping heartbeat sender");
                            break;
                        }

                        let heartbeat_msg = serde_json::json!({
                            "leader_id": state_clone.node_id,
                            "leader_port": state_clone.self_port,
                            "term": 1
                        });

                        for peer in &peers {
                            let url = format!("http://{}{}/heartbeat", peer, raft_config.path);
                            let mut req = client.post(&url).json(&heartbeat_msg);

                            if let Some(ref token) = raft_config.auth_token {
                                req = req.header("X-Raft-Token", token);
                            }

                            let _ = req.send().await;
                        }
                    }
                });
            } else {
                // Follower: monitor heartbeats and trigger election if timeout
                tokio::spawn(async move {
                    use std::time::Duration;
                    use tokio::time::sleep;

                    loop {
                        sleep(Duration::from_secs(1)).await;

                        if state_clone.should_start_election() {
                            log::info!("⏰ Heartbeat timeout detected! Starting election...");

                            let (should_become_leader, new_leader_id, new_leader_port) =
                                state_clone.start_election().await;

                            if should_become_leader {
                                state_clone.become_leader();
                            } else {
                                state_clone.become_follower(new_leader_id, new_leader_port);
                            }
                        }
                    }
                });
            }
        }

        // Extract config values before moving self into Arc
        let request_timeout = self.config.server.request_timeout;
        let max_body_size = self.config.server.max_body_size;

        // Materialize firewall from config (builder > env)
        let firewall = Arc::new(crate::http::Firewall::new(
            crate::http::firewall::resolve_firewall_config(self.firewall_config.clone(), None),
        ));

        // Materialize anti-DDoS protection if configured
        let anti_ddos: Option<Arc<crate::security::anti_ddos::AntiDDoSProtection>> = self
            .anti_ddos_config
            .as_ref()
            .map(|cfg| Arc::new(crate::security::anti_ddos::AntiDDoSProtection::new(cfg.clone())));

        // Resolve access log: builder flag OR env var
        let access_log = self.access_log
            || std::env::var("LT_HTTP_ACCESS_LOG")
                .ok()
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);

        if access_log {
            crate::http::init_access_log_buffer(self.access_log_capacity);
        }

        // Initialize system metrics collector (CPU, RAM, load, RSS)
        crate::system::init_system_metrics();

        // Share server state
        let server = Arc::new(self);

        // Pin the shutdown future once so we can poll it across loop
        // iterations via `&mut`. For the `serve()` delegate this is
        // `std::future::pending()`, which never resolves — the `select!`
        // below then behaves identically to the historical unconditional
        // `loop { listener.accept().await? }`.
        tokio::pin!(shutdown);

        // Accept connections until shutdown is signaled.
        loop {
            let (stream, remote_addr) = tokio::select! {
                // `biased`: poll the shutdown branch first so a signal wins
                // deterministically over a connection already sitting in the
                // accept backlog. Without it, `select!`'s random fairness
                // could accept one more connection after shutdown is ready.
                biased;
                _ = &mut shutdown => {
                    log::info!(
                        "Graceful shutdown signal received; stopping accept loop"
                    );
                    break;
                }
                accepted = listener.accept() => accepted?,
            };

            // Connection-level anti-DDoS check
            if let Some(ref protection) = anti_ddos {
                if !protection.is_connection_allowed(remote_addr.ip()).await {
                    log::warn!("Anti-DDoS: rejected connection from {}", remote_addr.ip());
                    drop(stream);
                    continue;
                }
            }

            let server = server.clone();
            let firewall = firewall.clone();
            let anti_ddos = anti_ddos.clone();
            #[cfg(feature = "tls")]
            let tls_acceptor = tls_acceptor.clone();

            tokio::spawn(async move {
                // TLS handshake (if configured) or plain TCP
                #[cfg(feature = "tls")]
                let io = {
                    let maybe_tls = if let Some(acceptor) = tls_acceptor {
                        match tokio::time::timeout(
                            std::time::Duration::from_secs(10),
                            acceptor.accept(stream),
                        )
                        .await
                        {
                            Ok(Ok(tls_stream)) => MaybeTlsStream::Tls(Box::new(tls_stream)),
                            Ok(Err(e)) => {
                                log::debug!("TLS handshake failed from {}: {}", remote_addr, e);
                                return;
                            }
                            Err(_) => {
                                log::debug!("TLS handshake timeout from {}", remote_addr);
                                return;
                            }
                        }
                    } else {
                        MaybeTlsStream::Plain(stream)
                    };
                    hyper_util::rt::TokioIo::new(maybe_tls)
                };
                #[cfg(not(feature = "tls"))]
                let io = hyper_util::rt::TokioIo::new(stream);

                let service = hyper::service::service_fn(
                    move |req: hyper::Request<hyper::body::Incoming>| {
                        let server = server.clone();
                        let firewall = firewall.clone();
                        let anti_ddos = anti_ddos.clone();
                        async move {
                            let start = std::time::Instant::now();
                            let req_method = req.method().to_string();
                            let req_path = req.uri().path().to_string();
                            // Correlation ID (issue #107): respect a sane
                            // inbound X-Request-ID, otherwise mint a UUID v4.
                            // Captured as a span field so every log/span event
                            // emitted while handling this request carries it,
                            // and echoed on the response below.
                            let request_id = request_id_from_headers(req.headers());
                            // Resolve real client IP (trusts proxy headers only from loopback/private)
                            let client_ip = crate::http::resolve_client_ip(&req, remote_addr);

                            let http_span = tracing::info_span!(
                                "http_request",
                                method = %req_method,
                                path = %req_path,
                                request_id = %request_id,
                            );

                            let mut result = (async move {
                                // Firewall check (preserves 403 vs 429 distinction)
                                if let Err(denied) = firewall.check(
                                    Some(remote_addr),
                                    req.method(),
                                    req.uri().path(),
                                ) {
                                    let (parts, boxed_body) = denied.into_parts();
                                    let body_bytes = http_body_util::BodyExt::collect(boxed_body)
                                        .await
                                        .map(|c| c.to_bytes())
                                        .unwrap_or_else(|_| {
                                            bytes::Bytes::from(r#"{"error":"Forbidden"}"#)
                                        });
                                    return Ok::<_, std::convert::Infallible>(
                                        Self::add_security_headers(
                                            hyper::Response::builder()
                                                .status(parts.status)
                                                .header("Content-Type", "application/json")
                                                .body(boxed_full(body_bytes))
                                                .expect("valid HTTP response"),
                                            tls_active,
                                        ),
                                    );
                                }

                                // Anti-DDoS request rate check
                                if let Some(ref protection) = anti_ddos {
                                    if !protection.is_request_allowed(remote_addr.ip()).await {
                                        return Ok(Self::add_security_headers(
                                            hyper::Response::builder()
                                                .status(429)
                                                .header("Content-Type", "application/json")
                                                .header("Retry-After", "60")
                                                .body(boxed_full(bytes::Bytes::from(
                                                    r#"{"error":"Rate limit exceeded"}"#,
                                                )))
                                                .expect("valid HTTP response"),
                                            tls_active,
                                        ));
                                    }
                                }

                                // Body size enforcement via Content-Length
                                if let Some(cl) = req.headers().get(hyper::header::CONTENT_LENGTH) {
                                    if let Ok(len) = cl.to_str().unwrap_or("0").parse::<usize>() {
                                        if len > max_body_size {
                                            return Ok(Self::add_security_headers(
                                                hyper::Response::builder()
                                                    .status(413)
                                                    .header("Content-Type", "application/json")
                                                    .body(boxed_full(bytes::Bytes::from(
                                                        r#"{"error":"Request body too large"}"#,
                                                    )))
                                                    .expect("valid HTTP response"),
                                                tls_active,
                                            ));
                                        }
                                    }
                                }

                                match server.handle_request(req).await {
                                    Ok(resp) => Ok::<_, std::convert::Infallible>(
                                        Self::add_security_headers(resp, tls_active),
                                    ),
                                    Err(e) => {
                                        log::error!("Request handler error: {}", e);
                                        Ok(Self::add_security_headers(
                                            response::json(
                                                StatusCode::INTERNAL_SERVER_ERROR,
                                                r#"{"error":"Internal server error"}"#,
                                            ),
                                            tls_active,
                                        ))
                                    }
                                }
                            })
                            .instrument(http_span)
                            .await;

                            // Echo the correlation ID on every response. This
                            // is the single top-level attach point that wraps
                            // ALL branches above — firewall 403/429, anti-DDoS
                            // 429, body-size 413, handler 500, and success —
                            // so no response-construction path can miss it.
                            // `request_id` is either a validated visible-ASCII
                            // inbound value or a generated UUID, so the
                            // HeaderValue conversion cannot fail in practice;
                            // we still go through `from_str` defensively
                            // rather than panic.
                            // (`Err` is `Infallible`, so the pattern is
                            // irrefutable — same style as the access-log
                            // destructuring below.)
                            let Ok(ref mut resp) = result;
                            if let Ok(hv) = hyper::header::HeaderValue::from_str(&request_id) {
                                resp.headers_mut().insert("x-request-id", hv);
                            }

                            if access_log {
                                let Ok(ref resp) = result;
                                crate::http::log_access_ip(
                                    &client_ip,
                                    &req_method,
                                    &req_path,
                                    resp,
                                    start,
                                );
                            }

                            result
                        }
                    },
                );

                if let Err(err) = hyper::server::conn::http1::Builder::new()
                    .timer(hyper_util::rt::TokioTimer::new())
                    .header_read_timeout(std::time::Duration::from_secs(request_timeout))
                    .keep_alive(true)
                    .serve_connection(io, service)
                    .await
                {
                    // `header_read_timeout` reaps idle keep-alive connections
                    // with a "read header from client timeout" error
                    // (`hyper::Error::is_timeout()`). That is routine
                    // housekeeping, not a fault — under sustained load it
                    // fired thousands of times at ERROR (issue #104 stress
                    // run), drowning real errors. Keep genuine connection
                    // errors at ERROR.
                    if err.is_timeout() {
                        log::debug!("Idle connection reaped from {}: {}", remote_addr, err);
                    } else {
                        log::error!("Connection error from {}: {}", remote_addr, err);
                    }
                }
            });
        }

        // In-flight connection drain (approach (b) from issue #112).
        //
        // Per-connection handlers above are `tokio::spawn`ed and forgotten —
        // we do not currently track their `JoinHandle`s, so we cannot join
        // them precisely. Instead, once the accept loop has stopped, we give
        // already-accepted connections a fixed grace window to finish before
        // returning (after which the runtime drops any stragglers).
        //
        // This is the zero-new-dependency option: `tokio-util` is not a
        // declared dependency of this crate, so a `CancellationToken` +
        // JoinHandle join-with-timeout (approach (a)) would mean pulling it in
        // just for this. Precise join-based draining is left as a follow-up.
        //
        // For the `serve()` delegate this code is unreachable: its shutdown
        // future is `std::future::pending()`, so the loop above never breaks.
        //
        // Close the listening socket before the grace window so new TCP
        // handshakes fail fast instead of completing into the accept backlog
        // (where they'd never be serviced). This tightens the shutdown
        // boundary: no connection accepted after the signal. Note this still
        // does NOT signal the already-spawned per-connection tasks — a
        // keep-alive connection accepted before shutdown can keep serving
        // within the grace window; precise per-connection draining is the
        // deferred approach-(a) follow-up.
        drop(listener);
        log::info!("Draining in-flight connections for up to {:?}", Self::GRACEFUL_DRAIN_GRACE);
        tokio::time::sleep(Self::GRACEFUL_DRAIN_GRACE).await;

        // OTLP exporters batch spans; flush AFTER the drain window so the
        // spans of the last in-flight requests are exported too (issue #107
        // phase 2). Bounded — see `shutdown_otel_provider`. No-op when
        // export was never enabled.
        #[cfg(feature = "otel")]
        shutdown_otel_provider().await;

        log::info!("Graceful shutdown complete");
        Ok(())
    }

    /// Grace period granted to already-accepted connections to drain after a
    /// graceful shutdown signal, before [`serve_with_graceful_shutdown`]
    /// returns. Currently a fixed constant; making it configurable and
    /// switching to precise join-based draining is a follow-up to issue #112.
    ///
    /// [`serve_with_graceful_shutdown`]: Self::serve_with_graceful_shutdown
    const GRACEFUL_DRAIN_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

    /// Add security headers to a response.
    /// Uses `entry().or_insert()` so handlers that explicitly set a header are not overridden.
    /// When `tls_active` is true, adds HSTS header.
    fn add_security_headers(resp: RouteResponse, tls_active: bool) -> RouteResponse {
        let (mut parts, body) = resp.into_parts();
        let h = &mut parts.headers;
        h.entry("x-content-type-options")
            .or_insert("nosniff".parse().expect("valid header value"));
        h.entry("x-frame-options")
            .or_insert("DENY".parse().expect("valid header value"));
        h.entry("referrer-policy")
            .or_insert("strict-origin-when-cross-origin".parse().expect("valid header value"));
        h.entry("x-xss-protection")
            .or_insert("1; mode=block".parse().expect("valid header value"));
        if tls_active {
            h.entry("strict-transport-security").or_insert(
                "max-age=31536000; includeSubDomains".parse().expect("valid header value"),
            );
        }
        hyper::Response::from_parts(parts, body)
    }

    /// Build the canonical "leader port unknown" 503 response used by every
    /// follower-side write redirect path.
    ///
    /// A follower learns the leader's port from the first AppendEntries it
    /// receives. Until then `raft_state.get_leader_port()` returns 0, and
    /// blindly redirecting to `http://127.0.0.1:0` would point clients at the
    /// kernel's "ephemeral port" sentinel. Callers must short-circuit to this
    /// 503 when `leader_port == 0`. The body and headers are kept identical
    /// across call sites so clients can recognize and back off uniformly.
    ///
    /// Note: only the 503 fallback is centralized here. The 307 redirect
    /// branches in `handle_request`, `handle_model_request`, and
    /// `handle_migrate_operation` differ in body shape, headers
    /// (`X-Raft-Leader`), and the path/query forwarded in `Location`, so
    /// folding them would change wire behavior. They remain inlined.
    fn leader_port_unknown_503() -> RouteResponse {
        hyper::Response::builder()
            .status(hyper::StatusCode::SERVICE_UNAVAILABLE)
            .header("Content-Type", "application/json")
            .header("Retry-After", "1")
            .body(boxed_full(Bytes::from(
                r#"{"error":"Leader port not yet discovered, retry after heartbeat"}"#,
            )))
            .expect("valid HTTP response")
    }

    /// Match a path against a pattern with wildcard support
    ///
    /// Supports:
    /// - Exact match: `/api/products`
    /// - Single segment wildcard: `/api/*` matches `/api/products` but not `/api/products/123`
    /// - Multi-segment wildcard: `/api/**` matches `/api/products`, `/api/products/123`, etc.
    /// - Suffix wildcard: `/static/*` matches any path starting with `/static/`
    /// - Middle wildcard: `/api/consumers/*/orders` matches `/api/consumers/{id}/orders`
    fn path_matches(pattern: &str, path: &str) -> bool {
        // Exact match
        if pattern == path {
            return true;
        }

        // Wildcard matching
        if pattern.contains('*') {
            // Handle `**` (multi-segment wildcard) - matches everything after
            if let Some(prefix) = pattern.strip_suffix("/**") {
                return path.starts_with(prefix);
            }

            // Handle `/*` (any single path after prefix) - but only if it's at the end
            if pattern.ends_with("/*") && !pattern.contains("/*/") {
                let prefix = &pattern[..pattern.len() - 2];
                return path.starts_with(prefix);
            }

            // Handle exact wildcard `/` + `*`
            if pattern == "/*" {
                return true; // Matches any path
            }

            // Handle middle wildcard: `/api/consumers/*/orders`
            // Split both pattern and path by '/' and match segment by segment
            let pattern_segments: Vec<&str> = pattern.split('/').collect();
            let path_segments: Vec<&str> = path.split('/').collect();

            // Must have same number of segments for exact middle wildcard matching
            if pattern_segments.len() != path_segments.len() {
                return false;
            }

            // Match segment by segment
            for (p_seg, path_seg) in pattern_segments.iter().zip(path_segments.iter()) {
                if *p_seg == "*" {
                    // Wildcard matches any single segment
                    continue;
                }
                if p_seg != path_seg {
                    return false;
                }
            }
            return true;
        }

        false
    }

    /// Handle incoming HTTP request
    async fn handle_request(
        &self,
        req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<RouteResponse> {
        use bytes::Bytes;

        let method = req.method().clone();
        let path = req.uri().path().to_string();

        // Resolve the request host once and reuse it for both the host
        // redirect (below) and the vhost lookup (further down). Both
        // call paths previously called `host_from_request` independently;
        // hoisting it avoids duplicate header parsing and keeps the two
        // dispatch decisions consistent on a single source of truth.
        // `host_from_request` returns `None` only when neither URI
        // authority nor `Host:` header is present — we treat that as
        // "" (matches no entry).
        let req_host = crate::http::host_from_request(&req).unwrap_or("").to_string();

        // Host-to-host 301 redirect (canonical URL hygiene).
        //
        // Declared via `LithairServerBuilder::with_redirect`. Runs before
        // any other dispatch logic so a redirected host never reaches the
        // vhost router, frontends, or custom routes — the only thing the
        // client gets back is a 301 with `Location:` pointing at the
        // canonical host, preserving path + query string. Applies to ALL
        // HTTP methods (clients may follow 301 on any verb).
        //
        // Zero-cost when no redirects are configured.
        if self.host_redirects.has_entries() {
            if let Some(target_host) = self.host_redirects.lookup(&req_host) {
                let path_and_query =
                    req.uri().path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
                let location = format!("https://{}{}", target_host, path_and_query);

                // Build the `Location:` header value defensively.
                // `target_host` was normalized at config time and
                // `path_and_query` comes from a parsed `hyper::Uri`, so
                // the bytes should already be header-safe. We still go
                // through `HeaderValue::try_from` rather than panicking:
                // an unexpected failure here (e.g. a future change to the
                // URI parser) should produce a clean 500 to the client,
                // not crash the worker.
                match hyper::header::HeaderValue::try_from(location.as_str()) {
                    Ok(loc_hv) => {
                        log::debug!("301 host redirect: {} -> {}", req_host, location);
                        return Ok(hyper::Response::builder()
                            .status(hyper::StatusCode::MOVED_PERMANENTLY)
                            .header(hyper::header::LOCATION, loc_hv)
                            .header("Content-Type", "text/plain; charset=utf-8")
                            .body(boxed_full(Bytes::from_static(b"Moved Permanently")))
                            .expect("static body + valid status never fails"));
                    }
                    Err(e) => {
                        log::error!(
                            "host redirect: refusing to emit invalid Location='{}' ({})",
                            location,
                            e
                        );
                        return Ok(hyper::Response::builder()
                            .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                            .header("Content-Type", "text/plain; charset=utf-8")
                            .body(boxed_full(Bytes::from_static(b"Internal Server Error")))
                            .expect("static body + valid status never fails"));
                    }
                }
            }
        }

        // Resolve the vhost-scoped frontend engine map (if any) before we
        // consume `req` later in the pipeline. If no vhosts are
        // declared, `vhost_engines` is `None` and the server behaves as
        // before (host-agnostic path routing).
        let vhost_engines: Option<
            &std::collections::HashMap<String, Arc<crate::frontend::FrontendEngine>>,
        > = if self.vhost_frontend_router.has_entries() {
            self.vhost_frontend_router.lookup(&req_host)
        } else {
            None
        };

        log::debug!("{} {}", method, path);

        // Raft Cluster: Check for write redirection and Raft endpoints
        if let Some(ref raft_state) = self.raft_state {
            let heartbeat_path = self.config.raft.heartbeat_path();
            let leader_path = self.config.raft.leader_path();

            // Raft heartbeat endpoint
            if path == heartbeat_path && method == hyper::Method::POST {
                let provided_token =
                    req.headers().get("X-Raft-Token").and_then(|v| v.to_str().ok());

                if !self.config.raft.validate_token(provided_token) {
                    return Ok(response::json(
                        StatusCode::UNAUTHORIZED,
                        r#"{"error":"Invalid Raft token"}"#,
                    ));
                }

                // Update heartbeat timestamp
                raft_state.update_heartbeat();

                // Parse heartbeat to update leader info if needed
                use http_body_util::BodyExt;
                let body_bytes =
                    req.into_body().collect().await.map(|c| c.to_bytes()).unwrap_or_default();
                if let Ok(heartbeat) = serde_json::from_slice::<serde_json::Value>(&body_bytes) {
                    let leader_id =
                        heartbeat.get("leader_id").and_then(|v| v.as_u64()).unwrap_or(0);
                    let leader_port =
                        heartbeat.get("leader_port").and_then(|v| v.as_u64()).unwrap_or(0) as u16;

                    if !raft_state.is_leader()
                        && leader_id
                            != raft_state
                                .current_leader_id
                                .load(std::sync::atomic::Ordering::Relaxed)
                    {
                        log::info!(
                            "Heartbeat: updating leader to node {} (port {})",
                            leader_id,
                            leader_port
                        );
                        raft_state.become_follower(leader_id, leader_port);
                    }
                }

                return Ok(response::json(StatusCode::OK, r#"{"status":"ok"}"#));
            }

            // Raft leader discovery endpoint
            if path == leader_path && method == hyper::Method::GET {
                let provided_token =
                    req.headers().get("X-Raft-Token").and_then(|v| v.to_str().ok());

                if !self.config.raft.validate_token(provided_token) {
                    return Ok(response::json(
                        StatusCode::UNAUTHORIZED,
                        r#"{"error":"Invalid Raft token"}"#,
                    ));
                }

                let response = serde_json::json!({
                    "leader_id": raft_state.current_leader_id.load(std::sync::atomic::Ordering::Relaxed),
                    "leader_port": raft_state.get_leader_port(),
                    "is_current_node_leader": raft_state.is_leader(),
                    "node_id": raft_state.node_id
                });

                return Ok(response::json(StatusCode::OK, response.to_string()));
            }

            // Redirect writes to leader if we're a follower
            // Exception: /internal/* and /_raft/* endpoints are internal cluster communication
            let is_write =
                matches!(method, hyper::Method::POST | hyper::Method::PUT | hyper::Method::DELETE);
            let is_internal = path.starts_with("/internal/") || path.starts_with("/_raft/");
            // Frontend reloads are NODE-LOCAL: each node serves its own assets
            // from its own disk, so a follower must reload ITS frontend, not
            // bounce to the leader (which would reload the wrong node's assets
            // and leave the follower stale). Exempt the frontend admin plane
            // from write redirection (PR #138 review). Note: /_admin/data and
            // /_admin/schema are NOT exempt — their writes mutate the
            // replicated event store and correctly belong on the leader.
            let is_node_local_admin =
                path == "/_admin/frontend" || path.starts_with("/_admin/frontend/");

            if is_write && !raft_state.is_leader() && !is_internal && !is_node_local_admin {
                let leader_port = raft_state.get_leader_port();
                if leader_port == 0 {
                    return Ok(Self::leader_port_unknown_503());
                }

                let redirect_url = format!(
                    "http://127.0.0.1:{}{}",
                    leader_port,
                    req.uri().path_and_query().map(|pq| pq.as_str()).unwrap_or(&path)
                );

                log::debug!("Redirecting write to leader on port {}", leader_port);

                return Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::TEMPORARY_REDIRECT)
                    .header(hyper::header::LOCATION, redirect_url.clone())
                    .header("Content-Type", "application/json")
                    .body(boxed_full(Bytes::from(format!(
                        r#"{{"message":"Redirected to leader","leader_url":"{}"}}"#,
                        redirect_url
                    ))))
                    .expect("valid HTTP response"));
            }
        }

        // Internal replication endpoints (for followers to receive data from leader)
        if path == "/internal/replicate" && method == hyper::Method::POST {
            return self.handle_internal_replicate(req).await;
        }

        if path == "/internal/replicate_bulk" && method == hyper::Method::POST {
            return self.handle_internal_replicate_bulk(req).await;
        }

        if path == "/internal/replicate_update" && method == hyper::Method::POST {
            return self.handle_internal_replicate_update(req).await;
        }

        if path == "/internal/replicate_delete" && method == hyper::Method::POST {
            return self.handle_internal_replicate_delete(req).await;
        }

        // Raft consensus log append entries endpoint (for followers to receive log entries from leader)
        if path == "/_raft/append" && method == hyper::Method::POST {
            return self.handle_raft_append_entries(req).await;
        }

        // Snapshot endpoints for resync of desynced followers
        if path == "/_raft/snapshot" && method == hyper::Method::GET {
            return self.handle_get_snapshot(req).await;
        }
        if path == "/_raft/snapshot" && method == hyper::Method::POST {
            return self.handle_install_snapshot(req).await;
        }

        // Cluster health endpoint (follower status)
        if path == "/_raft/health" && method == hyper::Method::GET {
            return self.handle_cluster_health().await;
        }

        // Resync stats endpoint (snapshot resync observability)
        if path == "/_raft/resync_stats" && method == hyper::Method::GET {
            return self.handle_resync_stats().await;
        }

        // Migration operation endpoint (for rolling upgrades)
        if path == "/_raft/migrate" && method == hyper::Method::POST {
            return self.handle_migrate_operation(req).await;
        }

        // Sync status endpoint (detailed follower sync state for ops)
        if path == "/_raft/sync-status" && method == hyper::Method::GET {
            return self.handle_sync_status().await;
        }

        // Force resync endpoint (manually trigger snapshot resync)
        if path.starts_with("/_raft/force-resync") && method == hyper::Method::POST {
            return self.handle_force_resync(req).await;
        }

        // Schema sync endpoints (cluster-internal)
        if path == "/_raft/schema/propose" && method == hyper::Method::POST {
            return self.handle_schema_propose(req).await;
        }
        if path == "/_raft/schema/vote" && method == hyper::Method::POST {
            return self.handle_schema_vote(req).await;
        }
        if path == "/_raft/schema/current" && method == hyper::Method::GET {
            return self.handle_schema_current(req).await;
        }

        // Schema admin endpoints (external management)
        if path == "/_admin/schema" && method == hyper::Method::GET {
            return self.handle_admin_schema_list().await;
        }
        if path == "/_admin/schema/pending" && method == hyper::Method::GET {
            return self.handle_admin_schema_pending().await;
        }
        if path.starts_with("/_admin/schema/approve/") && method == hyper::Method::POST {
            return self.handle_admin_schema_approve(req, &path).await;
        }
        if path.starts_with("/_admin/schema/reject/") && method == hyper::Method::POST {
            return self.handle_admin_schema_reject(req, &path).await;
        }
        // Phase 3: Schema management operations
        if path == "/_admin/schema/sync" && method == hyper::Method::POST {
            return self.handle_admin_schema_sync().await;
        }
        if path == "/_admin/schema/diff" && method == hyper::Method::GET {
            return self.handle_admin_schema_diff().await;
        }
        if path == "/_admin/schema/history" && method == hyper::Method::GET {
            return self.handle_admin_schema_history().await;
        }
        if path == "/_admin/schema/revalidate" && method == hyper::Method::POST {
            return self.handle_admin_schema_revalidate().await;
        }
        if path.starts_with("/_admin/schema/rollback/") && method == hyper::Method::POST {
            return self.handle_admin_schema_rollback(req, &path).await;
        }
        // Schema lock/unlock endpoints
        if path == "/_admin/schema/lock/status" && method == hyper::Method::GET {
            return self.handle_admin_schema_lock_status().await;
        }
        if path == "/_admin/schema/lock" && method == hyper::Method::POST {
            return self.handle_admin_schema_lock(req).await;
        }
        if path == "/_admin/schema/unlock" && method == hyper::Method::POST {
            return self.handle_admin_schema_unlock(req).await;
        }

        // Route Guards - Declarative protection (authentication, authorization, etc.)
        for guard_matcher in &self.route_guards {
            if guard_matcher.matches(&req) {
                log::debug!("Evaluating guard for pattern: {}", guard_matcher.pattern);
                match guard_matcher.guard.check(&req, self.session_manager.clone()).await {
                    Ok(crate::http::GuardResult::Allow) => {
                        log::debug!("Guard allowed request");
                        // Continue to next guard or routing
                    }
                    Ok(crate::http::GuardResult::Deny(response)) => {
                        log::debug!("Guard denied request");
                        // Convert Full<Bytes> to BoxBody<Bytes, Infallible>
                        use http_body_util::BodyExt;
                        let (parts, body) = response.into_parts();
                        return Ok(hyper::Response::from_parts(parts, body.boxed()));
                    }
                    Err(e) => {
                        log::error!("Guard check failed: {}", e);
                        return Ok(response::json(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            r#"{"error":"Internal server error"}"#,
                        ));
                    }
                }
            }
        }

        // Status endpoint (for health checks and cluster discovery)
        if path == "/status" && method == hyper::Method::GET {
            let mut status = serde_json::json!({
                "status": "ready",
                "service": "lithair-server",
                "version": "1.0.0"
            });

            // Add Raft cluster info if enabled
            if let Some(ref raft_state) = self.raft_state {
                status["raft"] = serde_json::json!({
                    "enabled": true,
                    "node_id": raft_state.node_id,
                    "is_leader": raft_state.is_leader(),
                    "leader_port": raft_state.get_leader_port(),
                    "peers": self.cluster_peers.len()
                });
            }

            return Ok(response::json(StatusCode::OK, status.to_string()));
        }

        // OpenAPI spec endpoint
        if self.openapi_enabled && path == "/openapi.json" && method == hyper::Method::GET {
            let spec = if let Some(cached) = self.openapi_spec_cache.get() {
                cached
            } else {
                let models = self.models.read().await;
                let model_infos: Vec<crate::http::OpenApiModelInfo> = models
                    .iter()
                    .filter_map(|m| {
                        m.handler.schema_spec().map(|spec| crate::http::OpenApiModelInfo {
                            name: m.name.clone(),
                            base_path: m.base_path.clone(),
                            spec,
                        })
                    })
                    .collect();
                let generated = crate::http::generate_openapi_spec(&model_infos);
                let _ = self.openapi_spec_cache.set(generated);
                self.openapi_spec_cache.get().expect("just set")
            };

            return Ok(hyper::Response::builder()
                .status(hyper::StatusCode::OK)
                .header("Content-Type", "application/json")
                .header("Access-Control-Allow-Origin", "*")
                .body(boxed_full(Bytes::from(spec.to_string())))
                .expect("valid HTTP response"));
        }

        // Swagger UI endpoint
        if self.openapi_enabled && path == "/docs" && method == hyper::Method::GET {
            let html = r##"<!DOCTYPE html>
<html><head>
<title>API Documentation</title>
<meta charset="utf-8"/>
<meta name="viewport" content="width=device-width, initial-scale=1"/>
<link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css"/>
</head><body>
<div id="swagger-ui"></div>
<script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
<script>SwaggerUIBundle({url:"/openapi.json",dom_id:"#swagger-ui"})</script>
</body></html>"##;

            return Ok(hyper::Response::builder()
                .status(hyper::StatusCode::OK)
                .header("Content-Type", "text/html; charset=utf-8")
                .body(boxed_full(Bytes::from(html)))
                .expect("valid HTTP response"));
        }

        // Metrics endpoint
        if self.config.admin.metrics_enabled && path == self.config.admin.metrics_path {
            return self.handle_metrics_request(req).await;
        }

        // Data Admin API endpoints (/_admin/data/*)
        if self.config.admin.data_admin_enabled && path.starts_with("/_admin/data/") {
            return self.handle_data_admin_request(req, &path, &method).await;
        }

        // Frontend lifecycle admin API (/_admin/frontend[/...]) — issue #134.
        // Gated behind the same `data_admin_enabled` toggle as the data admin
        // plane. `with_data_admin()` registers a secure-by-default
        // `RequireAuth` guard over `/_admin/data/*` and `/_admin/frontend/*`
        // (evaluated in the route-guard loop above, issue #143); the firewall,
        // when enabled, is additional defense-in-depth, not the sole guard.
        if self.config.admin.data_admin_enabled
            && (path == "/_admin/frontend" || path.starts_with("/_admin/frontend/"))
        {
            return self.handle_frontend_admin_request(&path, &method).await;
        }

        // Data Admin UI (embedded dashboard, requires admin-ui feature)
        #[cfg(feature = "admin-ui")]
        if let Some(ref ui_path) = self.config.admin.data_admin_ui_path {
            if path == *ui_path || path == format!("{}/", ui_path) {
                return self.handle_data_admin_ui_request().await;
            }
        }

        // Admin panel endpoint
        if self.config.admin.enabled && path == self.config.admin.path {
            return self.handle_admin_request(req).await;
        }

        // Custom routes checked first — user overrides take priority over model prefix matches
        for route in &self.custom_routes {
            if route.method == method && Self::path_matches(&route.path, &path) {
                return (route.handler)(req).await;
            }
        }

        // Built-in operations endpoints (`/health`, `/ready`, `/info`).
        //
        // These match the README's "Every Lithair server comes with
        // /health, /ready, and /info out of the box" promise (see issue
        // #40). They live AFTER the `custom_routes` loop above so a
        // user calling `.with_route(GET, "/health", ...)` always wins
        // over the default — the override remains a one-line opt-out.
        // They live BEFORE model dispatch so a model whose base_path
        // happens to be `/health` (unlikely but legal) cannot shadow
        // them.
        //
        // The dispatch is a method-and-path equality check so we never
        // accidentally swallow `POST /health` or `/health/sub`.
        if method == hyper::Method::GET {
            match path.as_str() {
                ops_endpoints::HEALTH_PATH => return Ok(ops_endpoints::serve_health()),
                ops_endpoints::READY_PATH => return Ok(ops_endpoints::serve_ready()),
                ops_endpoints::INFO_PATH => {
                    let models = self.models.read().await;
                    let base_paths: Vec<String> =
                        models.iter().map(|m| m.base_path.clone()).collect();
                    drop(models);
                    return Ok(ops_endpoints::serve_info(&base_paths));
                }
                _ => {}
            }
        }

        // Model routes (DeclarativeModel CRUD endpoints)
        let models = self.models.read().await;
        for model in models.iter() {
            if path.starts_with(&model.base_path) {
                return self.handle_model_request(req, model).await;
            }
        }
        drop(models);

        // Frontend assets (memory-first serving with SCC2)
        // Checked AFTER API routes so /admin/login.html is served but /admin/api/* can still work.
        //
        // Resolution order:
        //   1. Vhost-scoped frontends (if the request's Host: matches a
        //      registered vhost or a default vhost is set).
        //   2. Host-agnostic frontends declared via `.with_frontend_at`.
        //
        // This ordering means declaring a vhost *narrows* serving for
        // that host without breaking path-only apps that never call
        // `.with_vhost`.
        // If a vhost matched (even with zero engines — e.g. registration
        // succeeded but all frontend loads failed), we stay scoped to that
        // vhost: serving host-agnostic frontends in that case would leak
        // them into a host that explicitly opted into isolation.
        let active_engines: &std::collections::HashMap<
            String,
            Arc<crate::frontend::FrontendEngine>,
        > = match vhost_engines {
            Some(e) => e,
            None => &self.frontend_engines,
        };

        // Static-file dispatch accepts both GET and HEAD. HEAD must
        // return the same status + headers as GET with an empty body
        // (RFC 7231 §4.3.2). Before this change, HEAD requests fell
        // straight through to the default 404 below — search engines
        // and uptime monitors that probe with HEAD saw
        // `404 Not Found / Content-Type: application/json` on every
        // static page, even though the body served on GET was correct
        // (issue #56).
        let is_head = method == hyper::Method::HEAD;
        if (method == hyper::Method::GET || is_head) && !active_engines.is_empty() {
            // Sort prefixes by length (longest first) for proper matching
            let mut prefixes: Vec<_> = active_engines.keys().collect();
            prefixes.sort_by_key(|b| std::cmp::Reverse(b.len()));

            // Special handling for _astro assets: try ALL frontends as fallback
            // This allows admin frontend to reference /_astro/* even when served at /secure-xy3xir/
            if path.starts_with("/_astro/") {
                // Try each frontend engine directly via SCC2 lookup
                for prefix in &prefixes {
                    if let Some(engine) = active_engines.get(*prefix) {
                        // Check if this engine has the asset in its SCC2 storage
                        if let Some(asset) = engine.get_asset(&path).await {
                            // Use mime_type from asset
                            let content_length = asset.content.len();
                            let body_bytes: Bytes =
                                if is_head { Bytes::new() } else { Bytes::from(asset.content) };
                            return Ok(hyper::Response::builder()
                                .status(200)
                                .header("Content-Type", asset.mime_type)
                                .header("Content-Length", content_length)
                                .header("Cache-Control", "public, max-age=31536000, immutable")
                                .body(boxed_full(body_bytes))
                                .expect("valid HTTP response"));
                        }
                    }
                }
                // All frontends returned 404, fall through to final 404
            } else {
                // Normal path matching: find the first matching prefix
                for prefix in prefixes {
                    if path.starts_with(prefix) {
                        if let Some(engine) = active_engines.get(prefix) {
                            let frontend_server =
                                crate::frontend::FrontendServer::new_scc2(engine.clone());

                            // For non-root frontends, strip the prefix from the path
                            let asset_path = if prefix == "/" {
                                path.to_string()
                            } else {
                                path.strip_prefix(prefix.as_str())
                                    .unwrap_or(path.as_str())
                                    .to_string()
                            };

                            // Create modified request with stripped path
                            let (mut parts, body) = req.into_parts();
                            parts.uri = asset_path.parse().expect("valid URI path");
                            let modified_req = hyper::Request::from_parts(parts, body);

                            // Call frontend server (returns BoxBody, Infallible error)
                            let Ok(response) = frontend_server.handle_request(modified_req).await;
                            // Pass BoxBody through directly (no collection needed)
                            return Ok(response);
                        }
                        // Break after first prefix match to avoid consuming req multiple times
                        break;
                    }
                }
            }
        }

        // Custom 404 handler (if configured)
        if let Some(ref handler) = self.not_found_handler {
            return (handler)(req).await;
        }

        // 404 Not Found (default)
        Ok(response::json(StatusCode::NOT_FOUND, r#"{"error":"Not found"}"#))
    }

    /// Handle admin panel request — returns a JSON overview of the running server
    async fn handle_admin_request(
        &self,
        _req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<RouteResponse> {
        let models = self.models.read().await;
        let model_names: Vec<&str> = models.iter().map(|m| m.name.as_str()).collect();
        let custom_route_paths: Vec<String> =
            self.custom_routes.iter().map(|r| format!("{} {}", r.method, r.path)).collect();

        let dashboard = serde_json::json!({
            "server": {
                "host": self.config.server.host,
                "port": self.config.server.port,
            },
            "models": model_names,
            "custom_routes": custom_route_paths,
            "features": {
                "admin_panel": self.config.admin.enabled,
                "metrics": self.config.admin.metrics_enabled,
                "data_admin": self.config.admin.data_admin_enabled,
                "sessions": self.session_manager.is_some(),
                "frontend": !self.frontend_engines.is_empty(),
                "cluster": self.config.replication.enabled,
            },
            "endpoints": {
                "admin": self.config.admin.path,
                "metrics": if self.config.admin.metrics_enabled { Some(&self.config.admin.metrics_path) } else { None },
                "schema": "/_admin/schema",
                "data_admin": if self.config.admin.data_admin_enabled { Some("/_admin/data/models") } else { None },
            }
        });

        let body = serde_json::to_string_pretty(&dashboard).unwrap_or_default();

        Ok(response::json(StatusCode::OK, body))
    }

    /// Handle metrics request
    async fn handle_metrics_request(
        &self,
        _req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<RouteResponse> {
        // Escape a string for use as a Prometheus label value per the text
        // exposition format spec (backslash, double quote, newline).
        fn escape_prom_label(s: &str) -> String {
            let mut out = String::with_capacity(s.len());
            for c in s.chars() {
                match c {
                    '\\' => out.push_str("\\\\"),
                    '"' => out.push_str("\\\""),
                    '\n' => out.push_str("\\n"),
                    c => out.push(c),
                }
            }
            out
        }

        // Snapshot the registered models under the read lock, then drop it
        // before awaiting per-model `get_stats` — otherwise long-running
        // sampling I/O blocks any writer trying to register a new model
        // (Gemini PR #83 review). Cloning the Arcs and the small metadata
        // strings is cheap; holding the lock across `.await` is not.
        let models_snapshot: Vec<(String, String, Arc<dyn crate::app::ModelHandler>)> = {
            let models = self.models.read().await;
            models
                .iter()
                .map(|m| (m.name.clone(), m.data_path.clone(), Arc::clone(&m.handler)))
                .collect()
        };

        let mut lines = Vec::new();

        lines.push("# HELP lithair_models_total Number of registered models".to_string());
        lines.push("# TYPE lithair_models_total gauge".to_string());
        lines.push(format!("lithair_models_total {}", models_snapshot.len()));

        lines.push("# HELP lithair_custom_routes_total Number of custom routes".to_string());
        lines.push("# TYPE lithair_custom_routes_total gauge".to_string());
        lines.push(format!("lithair_custom_routes_total {}", self.custom_routes.len()));

        lines.push("# HELP lithair_frontend_engines_total Number of frontend engines".to_string());
        lines.push("# TYPE lithair_frontend_engines_total gauge".to_string());
        lines.push(format!("lithair_frontend_engines_total {}", self.frontend_engines.len()));

        // Per-model storage stats (issue #72). One series per registered model.
        // approx_ram_bytes is a sample-based estimate — see ModelStats docs.
        // Compute stats once per model to avoid 3x I/O for raftlog metadata.
        //
        // Stats collection is parallelised via `futures::future::join_all` so
        // `/metrics` scales with the slowest model, not the sum of all models
        // (Gemini PR #83 round-3 review). The read lock has already been
        // dropped above, so the per-model `get_stats` futures are independent
        // — there's no shared mutable state across them. With N models the
        // wall-clock cost drops from sum(get_stats_i) to max(get_stats_i),
        // which matters for operators with many small models scraped by a
        // tight Prometheus interval.
        let stats_futures =
            models_snapshot.into_iter().map(|(name, data_path, handler)| async move {
                let stats = handler.get_stats(&data_path).await;
                (escape_prom_label(&name), stats)
            });
        let per_model: Vec<(String, crate::app::ModelStats)> =
            futures::future::join_all(stats_futures).await;

        lines.push("# HELP lithair_model_items Number of items held in RAM per model".to_string());
        lines.push("# TYPE lithair_model_items gauge".to_string());
        for (label, stats) in &per_model {
            lines.push(format!("lithair_model_items{{model=\"{}\"}} {}", label, stats.item_count));
        }

        lines.push(
            "# HELP lithair_model_ram_bytes Approximate per-model RAM cost in bytes (sampled)"
                .to_string(),
        );
        lines.push("# TYPE lithair_model_ram_bytes gauge".to_string());
        for (label, stats) in &per_model {
            lines.push(format!(
                "lithair_model_ram_bytes{{model=\"{}\"}} {}",
                label, stats.approx_ram_bytes
            ));
        }

        lines.push(
            "# HELP lithair_model_raftlog_bytes Size of events.raftlog on disk per model"
                .to_string(),
        );
        lines.push("# TYPE lithair_model_raftlog_bytes gauge".to_string());
        for (label, stats) in &per_model {
            lines.push(format!(
                "lithair_model_raftlog_bytes{{model=\"{}\"}} {}",
                label, stats.raftlog_size_bytes
            ));
        }

        lines.push(String::new());

        Ok(hyper::Response::builder()
            .status(200)
            .header("Content-Type", "text/plain; version=0.0.4")
            .body(boxed_full(Bytes::from(lines.join("\n"))))
            .expect("valid HTTP response"))
    }
}

impl Default for LithairServer {
    fn default() -> Self {
        Self {
            config: LithairConfig::default(),
            session_manager: None,
            custom_routes: Vec::new(),
            not_found_handler: None,
            route_guards: Vec::new(),
            model_infos: Vec::new(),
            models_require_session: false,
            external_handler_gates: Vec::new(),
            external_handler_sse_wirings: Vec::new(),
            models: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            frontend_configs: Vec::new(),
            frontend_engines: std::collections::HashMap::new(),
            vhost_frontend_configs: Vec::new(),
            vhost_frontend_router: crate::http::HostRouter::new(),
            host_redirects: crate::http::HostRouter::new(),
            firewall_config: None,
            anti_ddos_config: None,
            access_log: false,
            access_log_capacity: crate::http::DEFAULT_ACCESS_LOG_CAPACITY,
            cluster_peers: Vec::new(),
            node_id: None,
            raft_state: None,
            raft_crud_sender: None,
            consensus_log: None,
            wal: None,
            replication_batcher: None,
            snapshot_manager: None,
            migration_manager: None,
            resync_stats: Arc::new(crate::cluster::ResyncStats::new()),
            schema_sync_state: Arc::new(tokio::sync::RwLock::new(
                crate::schema::SchemaSyncState::default(),
            )),
            openapi_enabled: false,
            openapi_spec_cache: std::sync::OnceLock::new(),
            sse_broadcaster: None,
            auto_compaction: None,
        }
    }
}

#[cfg(test)]
mod tests;
