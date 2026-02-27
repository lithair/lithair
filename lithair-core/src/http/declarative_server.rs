//! Pure Declarative Server for Lithair (Hyper 1.x)
//!
//! This is the TRUE Lithair experience: define your model, get a complete server.
//! Hyper is completely hidden from users.

use brotli::CompressorWriter as BrotliCompressor;
use bytes::Bytes;
use flate2::{write::GzEncoder, Compression};
use hex as _hex;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as AutoBuilder;
use sha2::{Digest, Sha256};
use std::convert::Infallible;
use std::future::Future;
use std::io::Write;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use tokio::net::TcpListener;

use crate::consensus::{ConsensusConfig, DeclarativeConsensus, ReplicatedModel};
use crate::http::{log_access, DeclarativeHttpHandler, Firewall, FirewallConfig, HttpExposable};
use crate::rbac::{RbacConfig, RbacMiddleware};

type RespBody = BoxBody<Bytes, Infallible>;
type Req = Request<Incoming>;
type Resp = Response<RespBody>;

fn body_from<T: Into<Bytes>>(data: T) -> RespBody {
    Full::new(data.into()).boxed()
}
use crate::lifecycle::LifecycleAware;
use crate::security::anti_ddos::{AntiDDoSConfig, AntiDDoSProtection};

/// Pure Declarative Server - The ideal Lithair experience
///
/// Users only need to:
/// 1. Define their DeclarativeModel with attributes
/// 2. Call DeclarativeServer::new().serve()
/// 3. Everything else is auto-generated!
pub struct DeclarativeServer<T>
where
    T: HttpExposable + LifecycleAware + ReplicatedModel + Send + Sync + 'static,
{
    handler: DeclarativeHttpHandler<T>,
    port: u16,
    node_id: Option<u64>,
    consensus_config: Option<ConsensusConfig>,
    model_name: &'static str,
    firewall_config: Option<FirewallConfig>,
    anti_ddos_config: Option<AntiDDoSConfig>,
    // New unified observability configs
    readiness_config: Option<ReadinessConfig>,
    observe_config: Option<ObserveConfig>,
    // Legacy configs (for backward compatibility)
    perf_config: Option<PerfEndpointsConfig>,
    gzip_config: Option<GzipConfig>,
    route_policies: Vec<(String, RoutePolicy)>,
    access_log: bool,
    // Logging configuration
    logging_config: Option<crate::logging::LoggingConfig>,
    // RBAC configuration
    rbac_config: Option<RbacConfig>,
    rbac_middleware: Option<RbacMiddleware>,
    // Migration settings
    legacy_endpoints: bool,
    deprecation_warnings: bool,
}

fn resolve_perf_config(user: Option<PerfEndpointsConfig>) -> Option<PerfEndpointsConfig> {
    let mut cfg = user;
    if let Ok(v) = std::env::var("LT_PERF_ENABLED") {
        let enabled = v == "1" || v.eq_ignore_ascii_case("true");
        let base_default =
            cfg.as_ref().map(|c| c.base_path.clone()).unwrap_or_else(|| "/perf".into());
        let mut new_cfg = cfg.unwrap_or(PerfEndpointsConfig { enabled, base_path: base_default });
        new_cfg.enabled = enabled;
        cfg = Some(new_cfg);
    }
    if let Ok(base) = std::env::var("LT_PERF_BASE") {
        let enabled_default = cfg.as_ref().map(|c| c.enabled).unwrap_or(true);
        cfg = Some(PerfEndpointsConfig { enabled: enabled_default, base_path: base });
    }
    cfg
}

/// Declarative configuration for optional performance/stateless endpoints
#[derive(Clone, Debug)]
pub struct PerfEndpointsConfig {
    pub enabled: bool,
    pub base_path: String, // e.g., "/perf"
}

/// Declarative configuration for gzip compression
#[derive(Clone, Debug)]
pub struct GzipConfig {
    pub enabled: bool,
    pub min_bytes: usize,
}

/// Declarative configuration for readiness endpoint
#[derive(Clone, Debug)]
pub struct ReadinessConfig {
    pub enabled: bool,
    pub include_consensus: bool,
    pub include_version: bool,
    pub custom_fields: std::collections::HashMap<String, String>,
}

impl Default for ReadinessConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            include_consensus: true,
            include_version: true,
            custom_fields: std::collections::HashMap::new(),
        }
    }
}

/// Declarative configuration for unified observability endpoints
#[derive(Clone, Debug)]
pub struct ObserveConfig {
    pub enabled: bool,
    pub base_path: String,     // e.g., "/observe"
    pub metrics_enabled: bool, // /observe/metrics
    pub perf_enabled: bool,    // /observe/perf/*
    pub max_perf_bytes: usize,
    pub custom_metrics: Vec<String>, // Future: custom metric names
}

/// Router configuration bundle to reduce function arguments
#[derive(Clone)]
struct RouterConfig {
    readiness_cfg: Option<ReadinessConfig>,
    observe_cfg: Option<ObserveConfig>,
    perf_cfg: Option<PerfEndpointsConfig>,
    gzip_cfg: Option<GzipConfig>,
    route_policies: Vec<(String, RoutePolicy)>,
    access_log: bool,
    legacy_endpoints: bool,
    deprecation_warnings: bool,
    anti_ddos: Option<Arc<AntiDDoSProtection>>,
    rbac_middleware: Option<RbacMiddleware>,
}

impl Default for ObserveConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            base_path: "/observe".to_string(),
            metrics_enabled: true,
            perf_enabled: true,
            max_perf_bytes: 2_000_000,
            custom_metrics: vec![],
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RoutePolicy {
    pub gzip: Option<bool>,       // None = inherit, Some(true/false) = force
    pub no_store: bool,           // Add Cache-Control: no-store
    pub min_bytes: Option<usize>, // Override gzip min_bytes
}

#[derive(Clone, Debug, Default)]
pub struct HttpServerConfig {
    pub gzip: Option<GzipConfig>,
    pub perf_endpoints: Option<PerfEndpointsConfig>,
    pub route_policies: Vec<(String, RoutePolicy)>, // Vec of (prefix, policy)
}

async fn finalize_response_async(
    resp: Resp,
    accept_br: bool,
    accept_gzip: bool,
    gzip_cfg: Option<&GzipConfig>,
    no_store: bool,
) -> Resp {
    let (mut parts, body) = resp.into_parts();
    if no_store {
        parts
            .headers
            .insert("cache-control", "no-store".parse().expect("valid header value"));
    }
    // If compression is enabled and accepted, compress when body size >= min_bytes
    if let Some(cfg) = gzip_cfg {
        if cfg.enabled && !parts.headers.contains_key("content-encoding") {
            let bytes = body.collect().await.expect("infallible body").to_bytes();
            if bytes.len() >= cfg.min_bytes {
                // Prefer br when accepted
                if accept_br {
                    let mut enc = BrotliCompressor::new(Vec::new(), 4096, 5, 22);
                    if enc.write_all(&bytes).is_ok() {
                        let compressed = enc.into_inner();
                        parts
                            .headers
                            .insert("content-encoding", "br".parse().expect("valid header value"));
                        parts
                            .headers
                            .insert("vary", "Accept-Encoding".parse().expect("valid header value"));
                        parts.headers.insert(
                            "content-length",
                            compressed.len().to_string().parse().expect("valid header value"),
                        );
                        let resp = Response::from_parts(parts, body_from(compressed));
                        return add_common_headers(resp);
                    }
                }
                if accept_gzip {
                    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
                    if encoder.write_all(&bytes).is_ok() {
                        if let Ok(compressed) = encoder.finish() {
                            parts.headers.insert(
                                "content-encoding",
                                "gzip".parse().expect("valid header value"),
                            );
                            parts.headers.insert(
                                "vary",
                                "Accept-Encoding".parse().expect("valid header value"),
                            );
                            parts.headers.insert(
                                "content-length",
                                compressed.len().to_string().parse().expect("valid header value"),
                            );
                            let resp = Response::from_parts(parts, body_from(compressed));
                            return add_common_headers(resp);
                        }
                    }
                }
            }
            // Fallback to original body if not compressing
            parts.headers.insert(
                "content-length",
                bytes.len().to_string().parse().expect("valid header value"),
            );
            let resp = Response::from_parts(parts, body_from(bytes));
            return add_common_headers(resp);
        }
    }
    let resp = Response::from_parts(parts, body);
    add_common_headers(resp)
}

fn resolve_gzip_config(user: Option<GzipConfig>) -> Option<GzipConfig> {
    if user.is_some() {
        return user;
    }
    let enabled = std::env::var("LT_HTTP_GZIP")
        .ok()
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if !enabled {
        return None;
    }
    let min_bytes = std::env::var("LT_HTTP_GZIP_MIN")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1024usize);
    Some(GzipConfig { enabled, min_bytes })
}

fn find_route_policy<'a>(
    policies: &'a [(String, RoutePolicy)],
    uri: &str,
) -> Option<&'a RoutePolicy> {
    let mut best: Option<(&str, &RoutePolicy)> = None;
    for (prefix, p) in policies.iter() {
        if uri.starts_with(prefix.as_str()) {
            match best {
                None => best = Some((prefix.as_str(), p)),
                Some((bp, _)) => {
                    if prefix.len() > bp.len() {
                        best = Some((prefix.as_str(), p));
                    }
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

/// Add global CORS and security headers to a response
fn add_common_headers(resp: Resp) -> Resp {
    let (mut parts, body) = resp.into_parts();
    let headers = &mut parts.headers;
    headers.insert("access-control-allow-origin", "*".parse().expect("valid header value"));
    headers.insert(
        "access-control-allow-methods",
        "GET, POST, PUT, DELETE, OPTIONS".parse().expect("valid header value"),
    );
    headers.insert(
        "access-control-allow-headers",
        "Content-Type, Authorization".parse().expect("valid header value"),
    );
    headers.insert("x-content-type-options", "nosniff".parse().expect("valid header value"));
    headers.insert("x-frame-options", "DENY".parse().expect("valid header value"));
    headers.insert("referrer-policy", "no-referrer".parse().expect("valid header value"));
    headers.insert(
        "content-security-policy",
        "default-src 'none'; frame-ancestors 'none'; base-uri 'none'"
            .parse()
            .expect("valid header value"),
    );
    Response::from_parts(parts, body)
}

fn serve_static_file(
    root: &str,
    rel_path: &str,
    req_headers: Option<&hyper::HeaderMap>,
) -> Option<Resp> {
    use std::path::PathBuf;
    let mut path = PathBuf::from(root);
    path.push(rel_path);
    // Prevent directory traversal
    if rel_path.contains("..") {
        return None;
    }
    match std::fs::read(&path) {
        Ok(bytes) => {
            let mime = guess_mime(rel_path);
            // Compute ETag (sha256)
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            let digest = hasher.finalize();
            let etag = format!("\"{}\"", _hex::encode(digest));

            // If-None-Match handling
            if let Some(h) = req_headers {
                if let Some(inm) = h.get("if-none-match").and_then(|v| v.to_str().ok()) {
                    if inm.trim() == etag {
                        let mut b = Response::builder().status(StatusCode::NOT_MODIFIED);
                        b = b.header("etag", etag);
                        // Cache-Control strategy: long cache for assets, no-cache for index.html
                        if rel_path.starts_with("assets/") {
                            b = b.header("cache-control", "public, max-age=31536000, immutable");
                        } else {
                            b = b.header("cache-control", "no-cache");
                        }
                        return Some(b.body(body_from(Bytes::new())).expect("valid HTTP response"));
                    }
                }
            }

            let mut b = Response::builder()
                .status(StatusCode::OK)
                .header("content-type", mime)
                .header("etag", etag);
            if rel_path.starts_with("assets/") {
                b = b.header("cache-control", "public, max-age=31536000, immutable");
            } else {
                b = b.header("cache-control", "no-cache");
            }
            Some(b.body(body_from(bytes)).expect("valid HTTP response"))
        }
        Err(_) => None,
    }
}

fn guess_mime(p: &str) -> &'static str {
    if p.ends_with(".html") {
        return "text/html; charset=utf-8";
    }
    if p.ends_with(".css") {
        return "text/css";
    }
    if p.ends_with(".js") {
        return "application/javascript";
    }
    if p.ends_with(".json") {
        return "application/json";
    }
    if p.ends_with(".svg") {
        return "image/svg+xml";
    }
    if p.ends_with(".png") {
        return "image/png";
    }
    if p.ends_with(".jpg") || p.ends_with(".jpeg") {
        return "image/jpeg";
    }
    if p.ends_with(".gif") {
        return "image/gif";
    }
    "application/octet-stream"
}

impl<T> DeclarativeServer<T>
where
    T: HttpExposable + LifecycleAware + ReplicatedModel + Send + Sync + 'static,
{
    /// Create a new declarative server for the given model
    ///
    /// # Arguments
    /// * `event_store_path` - Path to store events (EventStore)
    /// * `port` - HTTP port to listen on
    ///
    /// # Example
    /// ```ignore
    /// let server = DeclarativeServer::<Product>::new("./data/products.events", 8080)?;
    /// server.serve().await?;
    /// ```
    pub fn new(event_store_path: &str, port: u16) -> anyhow::Result<Self> {
        let model_name = std::any::type_name::<T>().split("::").last().unwrap_or("UnknownModel");

        log::info!("Creating Pure Declarative Lithair Server");
        log::info!("   Model: {}", model_name);
        log::info!("   Base Path: /api/{}", T::http_base_path());
        log::info!("   Port: {}", port);

        let handler = DeclarativeHttpHandler::<T>::new(event_store_path)
            .map_err(|e| anyhow::anyhow!("Failed to create handler: {}", e))?;

        log::info!("Declarative Server ready - EventStore: {}", event_store_path);

        Ok(Self {
            handler,
            port,
            node_id: None,
            consensus_config: None,
            model_name,
            firewall_config: None,
            anti_ddos_config: None,
            // New unified configs
            readiness_config: Some(ReadinessConfig::default()),
            observe_config: None,
            // Legacy configs (maintained for compatibility)
            perf_config: None,
            gzip_config: None,
            route_policies: Vec::new(),
            access_log: false,
            // Logging configuration
            logging_config: None,
            // RBAC configuration
            rbac_config: None,
            rbac_middleware: None,
            // Migration settings
            legacy_endpoints: false,
            deprecation_warnings: true,
        })
    }

    /// Set node ID for distributed scenarios
    pub fn with_node_id(mut self, node_id: u64) -> Self {
        self.node_id = Some(node_id);

        // Auto-configure consensus if the model needs replication
        if T::needs_replication() {
            let consensus_config = ConsensusConfig {
                node_id,
                cluster_peers: vec![], // Will be configured by with_cluster_peers()
                consensus_port: self.port + 1000, // Default: HTTP port + 1000 for consensus
                data_dir: format!("./data/node_{}", node_id),
            };
            self.consensus_config = Some(consensus_config);

            log::info!(
                "Auto-configured distributed replication for model with replicated fields: {:?}",
                T::replicated_fields()
            );
        }

        self
    }

    /// Configure cluster peers for distributed replication
    pub fn with_cluster_peers(mut self, peers: Vec<String>) -> Self {
        if let Some(ref mut config) = self.consensus_config {
            config.cluster_peers = peers;
        }
        self
    }

    /// Start the declarative server - Hyper is completely hidden!
    ///
    /// This method provides the ideal Lithair experience:
    /// - No manual HTTP code required
    /// - No router configuration needed  
    /// - No endpoint definitions required
    /// - Everything auto-generated from DeclarativeModel attributes
    pub async fn serve(self) -> anyhow::Result<()> {
        let addr = SocketAddr::from(([127, 0, 0, 1], self.port));

        // Initialize logging system if configured
        if let Some(ref logging_config) = self.logging_config {
            if let Err(e) = crate::logging::init_logging(logging_config) {
                log::warn!("Logging initialization failed: {}", e);
                log::warn!("   Continuing without custom logging...");
            } else {
                log::info!("Lithair logging system initialized");
                log::debug!(
                    "Logging config: outputs={}, format={:?}",
                    logging_config.outputs.len(),
                    logging_config.format
                );
            }
        }

        // Initialize consensus if configured
        if let Some(ref consensus_config) = self.consensus_config {
            let mut consensus = DeclarativeConsensus::<T>::new(consensus_config.clone());
            if let Err(e) = consensus.initialize().await {
                log::warn!("Consensus initialization failed: {}", e);
                log::warn!("   Continuing in single-node mode...");
            }
        }

        self.print_startup_info();

        // Hyper 1.x server setup - completely hidden from user!
        let handler = Arc::new(self.handler);
        let fw_cfg = crate::http::firewall::resolve_firewall_config(
            self.firewall_config.clone(),
            <T as HttpExposable>::firewall_config(),
        );
        let firewall = Arc::new(Firewall::new(fw_cfg));
        let model_name = self.model_name;
        let perf_cfg = resolve_perf_config(self.perf_config.clone());
        let gzip_cfg = self.gzip_config.clone();
        let route_policies = self.route_policies.clone();
        // New unified configs
        let readiness_cfg = self.readiness_config.clone();
        let observe_cfg = self.observe_config.clone();
        // Migration settings
        let legacy_endpoints = self.legacy_endpoints;
        let deprecation_warnings = self.deprecation_warnings;

        // Initialize anti-DDoS protection if configured
        let anti_ddos = self
            .anti_ddos_config
            .as_ref()
            .map(|cfg| Arc::new(AntiDDoSProtection::new(cfg.clone())));

        let gzip_cfg = resolve_gzip_config(gzip_cfg);
        let access_log_enabled = self.access_log
            || std::env::var("LT_HTTP_ACCESS_LOG")
                .ok()
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
        log::info!("Pure Lithair Declarative Server listening on http://127.0.0.1:{}", self.port);

        let listener = TcpListener::bind(addr).await?;
        loop {
            let (stream, remote_addr) = listener.accept().await?;

            // Check anti-DDoS protection if enabled
            if let Some(ref protection) = anti_ddos {
                if !protection.is_connection_allowed(remote_addr.ip()).await {
                    // Connection rejected by anti-DDoS protection
                    log::warn!("Anti-DDoS: Connection rejected from {}", remote_addr.ip());
                    drop(stream);
                    continue;
                }
            }

            let handler = Arc::clone(&handler);
            let firewall = Arc::clone(&firewall);
            let anti_ddos_clone = anti_ddos.clone();
            let perf_cfg = perf_cfg.clone();
            let gzip_cfg = gzip_cfg.clone();
            let route_policies = route_policies.clone();
            let readiness_cfg = readiness_cfg.clone();
            let observe_cfg = observe_cfg.clone();
            let rbac_middleware = self.rbac_middleware.clone();
            let access_log = access_log_enabled;
            // model_name is &'static str and Copy; no need to clone

            tokio::spawn(async move {
                let service = service_fn(move |req: Req| {
                    let handler = Arc::clone(&handler);
                    let firewall = Arc::clone(&firewall);
                    let anti_ddos_service = anti_ddos_clone.clone();
                    let perf_cfg = perf_cfg.clone();
                    let gzip_cfg = gzip_cfg.clone();
                    let route_policies = route_policies.clone();
                    let readiness_cfg = readiness_cfg.clone();
                    let observe_cfg = observe_cfg.clone();
                    let rbac_middleware = rbac_middleware.clone();
                    async move {
                        pure_declarative_router(
                            req,
                            handler,
                            model_name,
                            Some(remote_addr),
                            firewall,
                            RouterConfig {
                                readiness_cfg,
                                observe_cfg,
                                perf_cfg,
                                gzip_cfg,
                                route_policies,
                                access_log,
                                legacy_endpoints,
                                deprecation_warnings,
                                anti_ddos: anti_ddos_service,
                                rbac_middleware,
                            },
                        )
                        .await
                    }
                });

                // Production-grade HTTP server with complete timeouts
                let mut builder = AutoBuilder::new(TokioExecutor::new());

                // Configure HTTP/1.1 for production robustness (without timer for now)
                builder.http1()
                    .keep_alive(true)                                           // Enable keep-alive
                    .max_buf_size(64 * 1024); // Limit header size (64KB)
                                              // NOTE: header_read_timeout requires timer configuration, disabled for now

                let conn = builder.serve_connection(TokioIo::new(stream), service);

                // Global connection timeout to prevent resource exhaustion
                let connection_timeout = std::time::Duration::from_secs(300); // 5 minutes max per connection
                match tokio::time::timeout(connection_timeout, conn).await {
                    Err(_) => {
                        log::error!(
                            "Connection timeout after {} seconds",
                            connection_timeout.as_secs()
                        );
                    }
                    Ok(Err(e)) => {
                        log::error!("Server connection error: {}", e);
                    }
                    Ok(Ok(())) => {
                        // Connection completed successfully
                    }
                }
            });
        }
    }

    fn print_startup_info(&self) {
        log::info!("Ready for pure declarative operations!");
        if let Some(node_id) = self.node_id {
            log::info!("   Node ID: {}", node_id);
        }
        log::info!(
            "   Test with: curl -X POST http://127.0.0.1:{}/api/{} \\",
            self.port,
            T::http_base_path()
        );
        log::info!("             -H 'Content-Type: application/json' \\");
        log::info!("             -d '{{...}}'  # Your model JSON");
        log::info!("Auto-generated endpoints:");
        log::info!("   GET    /api/{:<20} - List all items", T::http_base_path());
        log::info!("   POST   /api/{:<20} - Create item", T::http_base_path());
        log::info!("   GET    /api/{}/{{id}}{:<10} - Get item by ID", T::http_base_path(), "");
        log::info!("   PUT    /api/{}/{{id}}{:<10} - Update item", T::http_base_path(), "");
        log::info!("   DELETE /api/{}/{{id}}{:<10} - Delete item", T::http_base_path(), "");
        log::info!(
            "   GET    /api/{}/count{:<10} - Lightweight item count",
            T::http_base_path(),
            ""
        );
        log::info!(
            "   GET    /api/{}/random-id{:<6} - Lightweight random existing id",
            T::http_base_path(),
            ""
        );
        log::info!("Health & Observability endpoints:");
        log::info!("   GET    /health{:<20} - Liveness probe (always enabled)", "");

        if let Some(cfg) = &self.readiness_config {
            if cfg.enabled {
                log::info!("   GET    /ready{:<21} - Readiness probe (configurable)", "");
            }
        }

        log::info!("   GET    /info{:<22} - Server diagnostics (like phpinfo)", "");

        if let Some(cfg) = &self.observe_config {
            if cfg.enabled {
                log::info!("   GET    {}/metrics{:<13} - Prometheus metrics", cfg.base_path, "");
                if cfg.perf_enabled {
                    log::info!("   POST   {}/perf/echo{:<11} - Latency testing", cfg.base_path, "");
                    log::info!(
                        "   GET    {}/perf/json{:<11} - JSON throughput testing",
                        cfg.base_path,
                        ""
                    );
                    log::info!(
                        "   GET    {}/perf/bytes{:<10} - Raw throughput testing",
                        cfg.base_path,
                        ""
                    );
                }
            }
        }

        // Legacy endpoint info
        if self.legacy_endpoints {
            log::warn!("Legacy endpoints (deprecated):");
            log::warn!("   GET    /status{:<20} - Use /ready instead", "");
            if let Some(cfg) = &self.perf_config {
                if cfg.enabled {
                    log::warn!(
                        "   *      {}/...{:<17} - Use /observe/perf/... instead",
                        cfg.base_path,
                        ""
                    );
                }
            }
        }
    }
}

impl<T> DeclarativeServer<T>
where
    T: HttpExposable + LifecycleAware + ReplicatedModel + Send + Sync + 'static,
{
    /// Provide a declarative firewall configuration from code (overrides env).
    pub fn with_firewall_config(mut self, cfg: FirewallConfig) -> Self {
        self.firewall_config = Some(cfg);
        self
    }

    /// Configure anti-DDoS protection
    pub fn with_anti_ddos_config(mut self, cfg: AntiDDoSConfig) -> Self {
        self.anti_ddos_config = Some(cfg);
        self
    }

    /// Provide a declarative configuration for performance/stateless endpoints
    /// Example: PerfEndpointsConfig { enabled: true, base_path: "/perf".into() }
    pub fn with_perf_endpoints(mut self, cfg: PerfEndpointsConfig) -> Self {
        self.perf_config = Some(cfg);
        self
    }

    /// Provide a declarative configuration for gzip compression
    pub fn with_gzip_config(mut self, cfg: GzipConfig) -> Self {
        self.gzip_config = Some(cfg);
        self
    }

    /// Add or override a policy for a URI prefix (e.g., "/perf")
    pub fn with_route_policy(mut self, prefix: &str, policy: RoutePolicy) -> Self {
        self.route_policies.push((prefix.to_string(), policy));
        self
    }

    /// Replace all route policies
    pub fn with_route_policies(mut self, policies: Vec<(String, RoutePolicy)>) -> Self {
        self.route_policies = policies;
        self
    }

    /// Enable or disable simple access logging (stdout)
    pub fn with_access_log(mut self, enabled: bool) -> Self {
        self.access_log = enabled;
        self
    }

    /// Configure readiness endpoint (/ready)
    pub fn with_readiness_config(mut self, cfg: ReadinessConfig) -> Self {
        self.readiness_config = Some(cfg);
        self
    }

    /// Configure unified observability endpoints (/observe/*)
    pub fn with_observe_config(mut self, cfg: ObserveConfig) -> Self {
        self.observe_config = Some(cfg);
        self
    }

    /// Enable legacy endpoint support during migration (/status, /perf/*)
    pub fn with_legacy_endpoints(mut self, enabled: bool) -> Self {
        self.legacy_endpoints = enabled;
        self
    }

    /// Enable deprecation warnings for legacy endpoints
    pub fn with_deprecation_warnings(mut self, enabled: bool) -> Self {
        self.deprecation_warnings = enabled;
        self
    }

    /// Convenience: Enable observe config with defaults
    pub fn with_observe_defaults(mut self) -> Self {
        self.observe_config = Some(ObserveConfig::default());
        self
    }

    /// Convenience: Customize observe base path
    pub fn with_observe_base_path(mut self, base_path: &str) -> Self {
        let mut cfg = self.observe_config.unwrap_or_default();
        cfg.base_path = base_path.to_string();
        self.observe_config = Some(cfg);
        self
    }

    /// Configure logging system (automatically initialized)
    ///
    /// # Example
    /// ```ignore
    /// let config = LoggingConfig::production()
    ///     .with_file_output("./logs/app.log", FileRotation::Daily, Some(7))
    ///     .with_context_field("service", "lithair");
    ///
    /// let server = DeclarativeServer::<Product>::new("./data/products.events", 8080)?
    ///     .with_logging_config(config);
    /// ```
    pub fn with_logging_config(mut self, config: crate::logging::LoggingConfig) -> Self {
        self.logging_config = Some(config);
        self
    }

    /// Provide a custom permission extractor for declarative read filtering.
    /// This sets an extractor on the inner DeclarativeHttpHandler that converts
    /// the incoming request into a list of permission identifiers (strings).
    pub fn with_permission_extractor<F>(mut self, extractor: F) -> Self
    where
        F: Fn(&Req) -> Vec<String> + Send + Sync + 'static,
    {
        self.handler.set_permission_extractor(extractor);
        self
    }

    /// Convenience: Production logging configuration with file rotation
    pub fn with_production_logging(mut self, log_file: &str) -> Self {
        let config = crate::logging::LoggingConfig::production()
            .with_file_output(log_file, crate::logging::FileRotation::Daily, Some(7))
            .with_context_field("service", "lithair")
            .with_context_field("port", &self.port.to_string());
        self.logging_config = Some(config);
        self
    }

    /// Convenience: Development logging configuration (human-readable to stdout)
    pub fn with_development_logging(mut self) -> Self {
        let config = crate::logging::LoggingConfig::development()
            .with_context_field("service", "lithair")
            .with_context_field("port", &self.port.to_string());
        self.logging_config = Some(config);
        self
    }

    /// Configure RBAC (Role-Based Access Control)
    ///
    /// # Example
    /// ```rust,ignore
    /// use lithair_core::rbac::{RbacConfig, ProviderConfig};
    ///
    /// let rbac_config = RbacConfig::new()
    ///     .enabled()
    ///     .with_provider(ProviderConfig::Password {
    ///         password: "secret123".to_string(),
    ///         default_role: "User".to_string(),
    ///     });
    ///
    /// let server = DeclarativeServer::<Product>::new("./data/products.events", 8080)?
    ///     .with_rbac(rbac_config);
    /// ```
    pub fn with_rbac(mut self, config: RbacConfig) -> Self {
        log::info!("Configuring RBAC: enabled={}", config.enabled);

        // Create middleware if provider is configured
        if let Some(provider) = config.provider.create_provider() {
            log::info!("RBAC middleware created with provider: {}", provider.name());
            // Convert Box to Arc
            self.rbac_middleware = Some(RbacMiddleware::new(Arc::from(provider)));
        } else {
            log::warn!("RBAC enabled but no provider configured!");
        }
        self.rbac_config = Some(config);
        self
    }
}

/// Pure declarative router - handles all HTTP routing automatically
/// Users never see this code!
async fn pure_declarative_router<T>(
    req: Req,
    handler: Arc<DeclarativeHttpHandler<T>>,
    model_name: &str,
    remote_addr: Option<std::net::SocketAddr>,
    firewall: Arc<Firewall>,
    config: RouterConfig,
) -> Result<Resp, Infallible>
where
    T: HttpExposable + LifecycleAware + Send + Sync + 'static + crate::consensus::ReplicatedModel,
{
    let uri = req.uri().path().to_string();
    let method = req.method().clone();
    let accept_enc = req
        .headers()
        .get("accept-encoding")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();
    let accept_gzip = accept_enc.contains("gzip");
    let accept_br = accept_enc.contains("br");

    let start_time = std::time::Instant::now();

    // Derive effective policy for this URI
    let policy = find_route_policy(&config.route_policies, &uri).cloned().unwrap_or_default();
    let mut effective_gzip = config.gzip_cfg.clone();
    if let Some(force) = policy.gzip {
        match (force, effective_gzip.take()) {
            (true, None) => {
                effective_gzip =
                    Some(GzipConfig { enabled: true, min_bytes: policy.min_bytes.unwrap_or(1024) });
            }
            (true, Some(cfg)) => {
                effective_gzip = Some(GzipConfig {
                    enabled: true,
                    min_bytes: policy.min_bytes.unwrap_or(cfg.min_bytes),
                });
            }
            (false, _) => effective_gzip = None,
        }
    } else if let (Some(cfg), Some(min_b)) = (effective_gzip.clone(), policy.min_bytes) {
        effective_gzip = Some(GzipConfig { enabled: cfg.enabled, min_bytes: min_b });
    }
    let no_store = policy.no_store;

    // OPTIONS preflight is universally supported with 204
    if method == Method::OPTIONS {
        let resp = finalize_response_async(
            Response::builder()
                .status(StatusCode::NO_CONTENT)
                .body(body_from(Bytes::new()))
                .expect("valid HTTP response"),
            accept_br,
            accept_gzip,
            effective_gzip.as_ref(),
            no_store,
        )
        .await;
        if config.access_log {
            log_access(remote_addr, method.as_str(), &uri, &resp, start_time);
        }
        return Ok(resp);
    }

    // Enforce firewall checks (allow/deny, rate limiting). OPTIONS are exempt inside.
    if let Err(resp) = firewall.check(remote_addr, &method, &uri) {
        return Ok(*resp);
    }

    // Anti-DDoS protection - rate limiting per IP
    if let (Some(protection), Some(addr)) = (&config.anti_ddos, remote_addr) {
        if !protection.is_request_allowed(addr.ip()).await {
            log::warn!("Anti-DDoS: Rate limit exceeded for {}", addr.ip());
            let resp = Response::builder()
                .status(StatusCode::TOO_MANY_REQUESTS)
                .header("content-type", "application/json")
                .header("retry-after", "60") // Suggest retry after 60 seconds
                .body(body_from(r#"{"error":"Rate limit exceeded","retry_after_seconds":60}"#))
                .expect("valid HTTP response");
            return Ok(resp);
        }
    }

    // ========================================================================
    // RBAC AUTHENTICATION
    // ========================================================================

    // Authenticate request if RBAC is enabled
    // Note: We need to convert Incoming to Full<Bytes> for authentication
    // For now, we'll authenticate after body collection in the handler
    // This is a placeholder for the authentication flow
    let _auth_context = if let Some(ref _middleware) = config.rbac_middleware {
        // Convert request for authentication
        // Note: proper request conversion and authentication is not yet wired up;
        // returns an unauthenticated context for now
        log::debug!("RBAC middleware enabled, authentication will be performed");
        Some(crate::rbac::AuthContext::unauthenticated())
    } else {
        None
    };

    // ========================================================================
    // NEW UNIFIED OBSERVABILITY ENDPOINTS
    // ========================================================================

    // Health check endpoint (Kubernetes liveness probe)
    // Always enabled - simple and fast
    if uri == "/health" && method == Method::GET {
        let resp = finalize_response_async(
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(body_from(r#"{"status":"healthy"}"#))
                .expect("valid HTTP response"),
            accept_br,
            accept_gzip,
            effective_gzip.as_ref(),
            no_store,
        )
        .await;
        if config.access_log {
            log_access(remote_addr, method.as_str(), &uri, &resp, start_time);
        }
        return Ok(resp);
    }

    // Readiness endpoint (Kubernetes readiness probe)
    // Configurable and feature-rich
    if uri == "/ready" && method == Method::GET {
        if let Some(cfg) = &config.readiness_cfg {
            if cfg.enabled {
                let mut json_parts = vec![
                    r#""status":"ready""#.to_string(),
                    format!(r#""model":"{}""#, model_name),
                    format!(r#""api":"/api/{}""#, T::http_base_path()),
                ];

                if cfg.include_consensus {
                    // Note: reports single-node; actual consensus status is not yet exposed
                    json_parts.push(r#""consensus":"single-node""#.to_string());
                }

                if cfg.include_version {
                    json_parts.push(format!(r#""version":"{}""#, env!("CARGO_PKG_VERSION")));
                }

                // Add custom fields
                for (key, value) in &cfg.custom_fields {
                    json_parts.push(format!(r#""{}":"{}""#, key, value));
                }

                let json_body = format!("{{{}}}", json_parts.join(","));

                let resp = finalize_response_async(
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(body_from(json_body))
                        .expect("valid HTTP response"),
                    accept_br,
                    accept_gzip,
                    effective_gzip.as_ref(),
                    no_store,
                )
                .await;
                if config.access_log {
                    log_access(remote_addr, method.as_str(), &uri, &resp, start_time);
                }
                return Ok(resp);
            }
        }
    }

    // Server Info endpoint (like phpinfo() for diagnostics)
    if uri == "/info" && method == Method::GET {
        let anti_ddos_enabled = std::env::var("LT_ANTI_DDOS")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);
        let max_connections =
            std::env::var("LT_MAX_CONNECTIONS").unwrap_or_else(|_| "not set".to_string());
        let rate_limit = std::env::var("LT_RATE_LIMIT").unwrap_or_else(|_| "not set".to_string());

        // Determine firewall status from environment or presence of firewall config
        let firewall_status = if std::env::var("LT_FIREWALL_ENABLED")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false)
        {
            "enabled"
        } else {
            "disabled"
        };
        let gzip_status = if config.gzip_cfg.is_some() { "enabled" } else { "disabled" };

        let info_json = format!(
            r#"{{
    "server": "Lithair Declarative Server",
    "version": "{}",
    "model": "{}",
    "api_base": "/api/{}",
    "security": {{
        "anti_ddos": {{"enabled": {}, "max_connections": "{}", "rate_limit": "{}"}},
        "firewall": {{"enabled": "{}"}},
        "gzip": {{"enabled": {}}}
    }},
    "environment": {{
        "RUST_LOG": "{}",
        "LT_ANTI_DDOS": "{}",
        "LT_MAX_CONNECTIONS": "{}",
        "LT_RATE_LIMIT": "{}"
    }},
    "endpoints": {{
        "health": "/health",
        "ready": "/ready",
        "info": "/info",
        "api": "/api/{}",
        "observe": "/observe/*"
    }},
    "timestamp": "{}"
}}"#,
            env!("CARGO_PKG_VERSION"),
            model_name,
            T::http_base_path(),
            anti_ddos_enabled,
            max_connections,
            rate_limit,
            firewall_status,
            gzip_status == "enabled",
            std::env::var("RUST_LOG").unwrap_or_else(|_| "not set".to_string()),
            std::env::var("LT_ANTI_DDOS").unwrap_or_else(|_| "not set".to_string()),
            max_connections,
            rate_limit,
            T::http_base_path(),
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
        );

        let resp = finalize_response_async(
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(body_from(info_json))
                .expect("valid HTTP response"),
            accept_br,
            accept_gzip,
            effective_gzip.as_ref(),
            no_store,
        )
        .await;
        if config.access_log {
            log_access(remote_addr, method.as_str(), &uri, &resp, start_time);
        }
        return Ok(resp);
    }

    // Unified observability endpoints (/observe/*)
    if let Some(cfg) = &config.observe_cfg {
        if cfg.enabled {
            let base = cfg.base_path.trim_end_matches('/');

            // /observe/metrics - Prometheus metrics endpoint
            if uri == format!("{}/metrics", base) && method == Method::GET && cfg.metrics_enabled {
                // Note: returns placeholder metrics; full Prometheus instrumentation is not yet implemented
                let metrics_body = format!(
                    "# HELP lithair_requests_total Total HTTP requests\n# TYPE lithair_requests_total counter\nlithair_requests_total{{model=\"{}\"}} 0\n",
                    model_name
                );
                let resp = finalize_response_async(
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "text/plain; charset=utf-8")
                        .body(body_from(metrics_body))
                        .expect("valid HTTP response"),
                    accept_br,
                    accept_gzip,
                    effective_gzip.as_ref(),
                    no_store,
                )
                .await;
                if config.access_log {
                    log_access(remote_addr, method.as_str(), &uri, &resp, start_time);
                }
                return Ok(resp);
            }

            // /observe/perf/echo - Echo endpoint for latency testing
            if uri == format!("{}/perf/echo", base) && method == Method::POST && cfg.perf_enabled {
                match req.into_body().collect().await.map(|c| c.to_bytes()) {
                    Ok(bytes) => {
                        let resp = Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "application/octet-stream")
                            .body(body_from(bytes))
                            .expect("valid HTTP response");
                        let resp = finalize_response_async(
                            resp,
                            accept_br,
                            accept_gzip,
                            effective_gzip.as_ref(),
                            no_store,
                        )
                        .await;
                        if config.access_log {
                            log_access(remote_addr, method.as_str(), &uri, &resp, start_time);
                        }
                        return Ok(resp);
                    }
                    Err(_) => {
                        let resp = Response::builder()
                            .status(StatusCode::BAD_REQUEST)
                            .header("content-type", "application/json")
                            .body(body_from(r#"{"error":"invalid body"}"#))
                            .expect("valid HTTP response");
                        let resp = finalize_response_async(
                            resp,
                            accept_br,
                            accept_gzip,
                            effective_gzip.as_ref(),
                            no_store,
                        )
                        .await;
                        if config.access_log {
                            log_access(remote_addr, method.as_str(), &uri, &resp, start_time);
                        }
                        return Ok(resp);
                    }
                }
            }

            // /observe/perf/json - JSON payload endpoint for throughput testing
            if uri.starts_with(&format!("{}/perf/json", base))
                && method == Method::GET
                && cfg.perf_enabled
            {
                let q = req.uri().query().unwrap_or("");
                let mut want: usize = 1024; // default 1KB
                for pair in q.split('&') {
                    if let Some((k, v)) = pair.split_once('=') {
                        if k == "bytes" {
                            if let Ok(n) = v.parse::<usize>() {
                                want = n.min(cfg.max_perf_bytes);
                            }
                        }
                    }
                }
                let payload = "x".repeat(want);
                let body = format!("{{\"data\":\"{}\"}}", payload);
                let resp = Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(body_from(body))
                    .expect("valid HTTP response");
                let resp = finalize_response_async(
                    resp,
                    accept_br,
                    accept_gzip,
                    effective_gzip.as_ref(),
                    no_store,
                )
                .await;
                if config.access_log {
                    log_access(remote_addr, method.as_str(), &uri, &resp, start_time);
                }
                return Ok(resp);
            }

            // /observe/perf/bytes - Raw bytes endpoint for throughput testing
            if uri.starts_with(&format!("{}/perf/bytes", base))
                && method == Method::GET
                && cfg.perf_enabled
            {
                let q = req.uri().query().unwrap_or("");
                let mut want: usize = 1024; // default 1KB
                for pair in q.split('&') {
                    if let Some((k, v)) = pair.split_once('=') {
                        if k == "n" || k == "bytes" {
                            if let Ok(n) = v.parse::<usize>() {
                                want = n.min(cfg.max_perf_bytes);
                            }
                        }
                    }
                }
                let data = vec![0x42u8; want]; // Fill with 'B' bytes
                let resp = Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/octet-stream")
                    .body(body_from(data))
                    .expect("valid HTTP response");
                let resp = finalize_response_async(
                    resp,
                    accept_br,
                    accept_gzip,
                    effective_gzip.as_ref(),
                    no_store,
                )
                .await;
                if config.access_log {
                    log_access(remote_addr, method.as_str(), &uri, &resp, start_time);
                }
                return Ok(resp);
            }
        }
    }

    // ========================================================================
    // LEGACY ENDPOINTS (for backward compatibility)
    // ========================================================================

    // Legacy Status endpoint (/status)
    if config.legacy_endpoints && uri == "/status" && method == Method::GET {
        if config.deprecation_warnings {
            log::warn!("DEPRECATED: /status endpoint is deprecated, use /ready instead");
        }
        let resp = finalize_response_async(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(body_from(format!(r#"{{"status":"ready","model":"{}","service":"pure-lithair-declarative","base_path":"/api/{}","deprecated":true,"use_instead":"/ready"}}"#, model_name, T::http_base_path())))
            .expect("valid HTTP response"), accept_br, accept_gzip, effective_gzip.as_ref(), no_store).await;
        if config.access_log {
            log_access(remote_addr, method.as_str(), &uri, &resp, start_time);
        }
        return Ok(resp);
    }

    // Legacy Performance endpoints (/perf/*)
    if config.legacy_endpoints && config.deprecation_warnings && uri.starts_with("/perf/") {
        log::warn!("DEPRECATED: /perf/* endpoints are deprecated, use /observe/perf/* instead");
    }

    // Optional performance/stateless endpoints (legacy support)
    if let Some(cfg) = &config.perf_cfg {
        if cfg.enabled {
            let base = cfg.base_path.trim_end_matches('/');
            if uri == format!("{}/echo", base) && method == Method::POST {
                // Echo request body as-is (binary)
                match req.into_body().collect().await.map(|c| c.to_bytes()) {
                    Ok(bytes) => {
                        let resp = Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "application/octet-stream")
                            .body(body_from(bytes))
                            .expect("valid HTTP response");
                        let resp = finalize_response_async(
                            resp,
                            accept_br,
                            accept_gzip,
                            effective_gzip.as_ref(),
                            no_store,
                        )
                        .await;
                        if config.access_log {
                            log_access(remote_addr, method.as_str(), &uri, &resp, start_time);
                        }
                        return Ok(resp);
                    }
                    Err(_) => {
                        let resp = Response::builder()
                            .status(StatusCode::BAD_REQUEST)
                            .header("content-type", "application/json")
                            .body(body_from(r#"{"error":"invalid body"}"#))
                            .expect("valid HTTP response");
                        let resp = finalize_response_async(
                            resp,
                            accept_br,
                            accept_gzip,
                            effective_gzip.as_ref(),
                            no_store,
                        )
                        .await;
                        if config.access_log {
                            log_access(remote_addr, method.as_str(), &uri, &resp, start_time);
                        }
                        return Ok(resp);
                    }
                }
            }

            if uri.starts_with(&format!("{}/json", base)) && method == Method::GET {
                // Returns a JSON with a string payload of requested size (approx.)
                let max_bytes: usize = std::env::var("LT_PERF_MAX_BYTES")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(2_000_000);
                let q = req.uri().query().unwrap_or("");
                let mut want: usize = 1024; // default 1KB
                for pair in q.split('&') {
                    if let Some((k, v)) = pair.split_once('=') {
                        if k == "bytes" {
                            if let Ok(n) = v.parse::<usize>() {
                                want = n.min(max_bytes);
                            }
                        }
                    }
                }
                let payload = "x".repeat(want);
                let body = format!("{{\"data\":\"{}\"}}", payload);
                let resp = Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(body_from(body))
                    .expect("valid HTTP response");
                let resp = finalize_response_async(
                    resp,
                    accept_br,
                    accept_gzip,
                    effective_gzip.as_ref(),
                    no_store,
                )
                .await;
                if config.access_log {
                    log_access(remote_addr, method.as_str(), &uri, &resp, start_time);
                }
                return Ok(resp);
            }

            if uri.starts_with(&format!("{}/bytes", base)) && method == Method::GET {
                // Returns raw bytes (application/octet-stream)
                let max_bytes: usize = std::env::var("LT_PERF_MAX_BYTES")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(2_000_000);
                let q = req.uri().query().unwrap_or("");
                let mut want: usize = 1024; // default 1KB
                for pair in q.split('&') {
                    if let Some((k, v)) = pair.split_once('=') {
                        if k == "n" || k == "bytes" {
                            if let Ok(n) = v.parse::<usize>() {
                                want = n.min(max_bytes);
                            }
                        }
                    }
                }
                let payload = vec![b'x'; want];
                let resp = Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/octet-stream")
                    .body(body_from(payload))
                    .expect("valid HTTP response");
                let resp = finalize_response_async(
                    resp,
                    accept_br,
                    accept_gzip,
                    effective_gzip.as_ref(),
                    no_store,
                )
                .await;
                if config.access_log {
                    log_access(remote_addr, method.as_str(), &uri, &resp, start_time);
                }
                return Ok(resp);
            }
        }
    }

    // API endpoints - delegate EVERYTHING to DeclarativeHttpHandler
    let api_base_path = format!("/api/{}", T::http_base_path());
    if uri.starts_with(&api_base_path) {
        let path_after_api = uri.strip_prefix(&api_base_path).unwrap_or("");
        let path_segments: Vec<&str> = path_after_api
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        // RBAC Authorization Check
        if let Some(ref middleware) = config.rbac_middleware {
            log::info!("RBAC: Checking authorization for {} {}", method, uri);

            // Extract auth headers
            let password = req.headers().get("x-auth-password").and_then(|v| v.to_str().ok());

            let role = req.headers().get("x-auth-role").and_then(|v| v.to_str().ok());

            log::info!("RBAC: password={:?}, role={:?}", password.is_some(), role);

            // Create a simple request for authentication
            let auth_req = Request::builder().method(method.clone()).uri(uri.clone());

            let auth_req = if let Some(pwd) = password {
                auth_req.header("x-auth-password", pwd)
            } else {
                auth_req
            };

            let auth_req =
                if let Some(r) = role { auth_req.header("x-auth-role", r) } else { auth_req };

            let auth_req = auth_req
                .body(http_body_util::Full::new(bytes::Bytes::new()))
                .expect("valid HTTP request");

            // Authenticate
            let auth_result = middleware.authenticate(&auth_req);

            // Check if operation is allowed based on method
            let requires_auth = matches!(method, Method::POST | Method::PUT | Method::DELETE);

            let is_authenticated =
                auth_result.as_ref().map(|ctx| ctx.authenticated).unwrap_or(false);

            if requires_auth && !is_authenticated {
                log::warn!("RBAC: Unauthenticated request to {} {}", method, uri);
                let resp = Response::builder()
                    .status(StatusCode::UNAUTHORIZED)
                    .header("content-type", "application/json")
                    .body(body_from(r#"{"error":"Authentication required"}"#))
                    .expect("valid HTTP response");
                return Ok(resp);
            }

            // For DELETE, check if user has admin/delete permissions
            if method == Method::DELETE && is_authenticated {
                // Simple role-based check: only Administrator can delete
                if let Some(r) = role {
                    if r != "Administrator" {
                        log::warn!("RBAC: User {} attempted DELETE without permission", r);
                        let resp = Response::builder()
                            .status(StatusCode::FORBIDDEN)
                            .header("content-type", "application/json")
                            .body(body_from(
                                r#"{"error":"Insufficient permissions for DELETE operation"}"#,
                            ))
                            .expect("valid HTTP response");
                        return Ok(resp);
                    }
                }
            }
        }

        // Per-request timeout (covers handler execution)
        let timeout_ms: u64 = std::env::var("LT_HTTP_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(10_000);

        let fut = handler.handle_request(req, &path_segments);
        let resp =
            match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), fut).await {
                Ok(r) => r?,
                Err(_) => Response::builder()
                    .status(StatusCode::GATEWAY_TIMEOUT)
                    .header("content-type", "application/json")
                    .body(body_from(r#"{"error":"request timeout"}"#))
                    .expect("valid HTTP response"),
            };

        let resp = finalize_response_async(
            resp,
            accept_br,
            accept_gzip,
            effective_gzip.as_ref(),
            no_store,
        )
        .await;
        if config.access_log {
            log_access(remote_addr, method.as_str(), &uri, &resp, start_time);
        }
        return Ok(resp);
    }

    // Optional static file serving for frontend benchmarks/examples
    // Enable by setting LT_STATIC_DIR to a directory containing index.html and assets/
    if let Ok(static_dir) = std::env::var("LT_STATIC_DIR") {
        let path = req.uri().path().to_string();
        if path == "/" || path == "/index.html" {
            if let Some(resp0) = serve_static_file(&static_dir, "index.html", Some(req.headers())) {
                let resp = finalize_response_async(
                    resp0,
                    accept_br,
                    accept_gzip,
                    effective_gzip.as_ref(),
                    no_store,
                )
                .await;
                if config.access_log {
                    log_access(remote_addr, method.as_str(), &uri, &resp, start_time);
                }
                return Ok(resp);
            }
        } else if let Some(rel) = path.strip_prefix("/assets/") {
            if let Some(resp0) =
                serve_static_file(&static_dir, &format!("assets/{}", rel), Some(req.headers()))
            {
                let resp = finalize_response_async(
                    resp0,
                    accept_br,
                    accept_gzip,
                    effective_gzip.as_ref(),
                    no_store,
                )
                .await;
                if config.access_log {
                    log_access(remote_addr, method.as_str(), &uri, &resp, start_time);
                }
                return Ok(resp);
            }
        }
    }

    // 404 for other endpoints
    let resp = finalize_response_async(Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header("content-type", "application/json")
        .body(body_from(r#"{"error":"Not found","hint":"Use /api/{model_base_path} for model operations, /status for server status"}"#))
        .expect("valid HTTP response"), accept_br, accept_gzip, effective_gzip.as_ref(), no_store).await;
    if config.access_log {
        log_access(remote_addr, method.as_str(), &uri, &resp, start_time);
    }
    Ok(resp)
}

// ============================================================================
// Convenience traits for ultra-simple usage
// ============================================================================

/// Extension trait to make model serving even simpler
///
/// This allows: `MyModel::serve_on_port(8080).await?`
pub trait DeclarativeServe:
    HttpExposable + LifecycleAware + ReplicatedModel + Send + Sync + 'static
{
    /// Serve this model on the given port with auto-generated EventStore path
    ///
    /// This is the simplest possible Lithair experience!
    fn serve_on_port(port: u16) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        let model_name = std::any::type_name::<Self>()
            .split("::")
            .last()
            .unwrap_or("UnknownModel")
            .to_lowercase();

        let event_store_path = format!("./data/{}.events", model_name);
        std::fs::create_dir_all("./data").ok();

        Box::pin(
            async move { DeclarativeServer::<Self>::new(&event_store_path, port)?.serve().await },
        )
    }

    /// Serve with explicit EventStore path
    fn serve_with_storage(
        port: u16,
        event_store_path: &str,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        let path = event_store_path.to_string();
        Box::pin(async move { DeclarativeServer::<Self>::new(&path, port)?.serve().await })
    }
}

// Auto-implement for all HttpExposable + LifecycleAware types
impl<T> DeclarativeServe for T where
    T: HttpExposable + LifecycleAware + ReplicatedModel + Send + Sync + 'static
{
}
