//! Builder pattern for LithairServer

use super::{CustomRoute, LithairServer};
use crate::config::LithairConfig;
use crate::session::{PersistentSessionStore, SessionManager};
use anyhow::Result;
use std::sync::Arc;

/// Builder for LithairServer
pub struct LithairServerBuilder {
    config: LithairConfig,
    session_manager: Option<Arc<dyn std::any::Any + Send + Sync>>,
    permission_checker: Option<Arc<dyn crate::rbac::PermissionChecker>>,
    custom_routes: Vec<CustomRoute>,
    not_found_handler: Option<super::RouteHandler>,
    model_infos: Vec<crate::app::ModelRegistrationInfo>,

    // HTTP Features
    logging_config: Option<crate::logging::LoggingConfig>,
    readiness_config: Option<crate::http::declarative_server::ReadinessConfig>,
    observe_config: Option<crate::http::declarative_server::ObserveConfig>,
    perf_config: Option<crate::http::declarative_server::PerfEndpointsConfig>,
    gzip_config: Option<crate::http::declarative_server::GzipConfig>,
    route_policies: std::collections::HashMap<String, crate::http::declarative_server::RoutePolicy>,
    route_guards: Vec<crate::http::RouteGuardMatcher>, // Declarative route protection
    firewall_config: Option<crate::http::FirewallConfig>,
    anti_ddos_config: Option<crate::security::anti_ddos::AntiDDoSConfig>,
    mfa_storage: Option<Arc<crate::mfa::MfaStorage>>, // MFA/TOTP storage
    legacy_endpoints: bool,
    deprecation_warnings: bool,

    // Frontend configurations (path_prefix -> static_dir)
    frontend_configs: Vec<(String, String)>, // (route_prefix, static_dir)

    // Cluster/Raft configuration
    cluster_peers: Vec<String>,
    node_id: Option<u64>,

    // Schema sync policy (for cluster consensus)
    schema_vote_policy: Option<crate::schema::SchemaVotePolicy>,
}

impl LithairServerBuilder {
    /// Get the session manager (for custom routes that need session validation)
    pub fn session_manager(&self) -> Option<Arc<dyn std::any::Any + Send + Sync>> {
        self.session_manager.clone()
    }

    /// Create a new builder with default configuration
    pub fn new() -> Self {
        Self {
            config: LithairConfig::load().unwrap_or_default(),
            session_manager: None,
            permission_checker: None,
            custom_routes: Vec::new(),
            not_found_handler: None,
            model_infos: Vec::new(),
            logging_config: None,
            readiness_config: None,
            observe_config: None,
            perf_config: None,
            gzip_config: None,
            route_policies: std::collections::HashMap::new(),
            route_guards: Vec::new(),
            firewall_config: None,
            anti_ddos_config: None,
            mfa_storage: None,
            legacy_endpoints: false,
            deprecation_warnings: false,
            frontend_configs: Vec::new(),
            cluster_peers: Vec::new(),
            node_id: None,
            schema_vote_policy: None,
        }
    }

    /// Create a builder with custom configuration
    pub fn with_config(config: LithairConfig) -> Self {
        Self {
            config,
            session_manager: None,
            permission_checker: None,
            custom_routes: Vec::new(),
            not_found_handler: None,
            model_infos: Vec::new(),
            logging_config: None,
            readiness_config: None,
            observe_config: None,
            perf_config: None,
            gzip_config: None,
            route_policies: std::collections::HashMap::new(),
            route_guards: Vec::new(),
            firewall_config: None,
            anti_ddos_config: None,
            mfa_storage: None,
            legacy_endpoints: false,
            deprecation_warnings: false,
            frontend_configs: Vec::new(),
            cluster_peers: Vec::new(),
            node_id: None,
            schema_vote_policy: None,
        }
    }

    // ========================================================================
    // SERVER CONFIGURATION
    // ========================================================================

    /// Set server port (overrides config file and env vars)
    pub fn with_port(mut self, port: u16) -> Self {
        self.config.server.port = port;
        self
    }

    /// Set server host
    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.config.server.host = host.into();
        self
    }

    /// Set number of worker threads
    pub fn with_workers(mut self, workers: usize) -> Self {
        self.config.server.workers = Some(workers);
        self
    }

    /// Enable/disable CORS
    pub fn with_cors(mut self, enabled: bool) -> Self {
        self.config.server.cors_enabled = enabled;
        self
    }

    /// Set CORS origins
    pub fn with_cors_origins(mut self, origins: Vec<String>) -> Self {
        self.config.server.cors_origins = origins;
        self
    }

    /// Set request timeout
    pub fn with_timeout(mut self, timeout: u64) -> Self {
        self.config.server.request_timeout = timeout;
        self
    }

    /// Set max body size
    pub fn with_max_body_size(mut self, size: usize) -> Self {
        self.config.server.max_body_size = size;
        self
    }

    /// Enable TLS with certificate and key PEM files
    ///
    /// When set, the server serves HTTPS with HSTS.
    /// Alternatively, set `LT_TLS_CERT` and `LT_TLS_KEY` environment variables.
    pub fn with_tls(mut self, cert_path: impl Into<String>, key_path: impl Into<String>) -> Self {
        self.config.server.tls_cert_path = Some(cert_path.into());
        self.config.server.tls_key_path = Some(key_path.into());
        self
    }

    // ========================================================================
    // SESSIONS
    // ========================================================================

    /// Add session manager
    pub fn with_sessions<S>(mut self, manager: SessionManager<S>) -> Self
    where
        S: crate::session::SessionStore + 'static + Send + Sync,
    {
        self.config.sessions.enabled = true;
        self.session_manager = Some(Arc::new(manager));
        self
    }

    /// Set session cleanup interval
    pub fn with_session_cleanup(mut self, interval: u64) -> Self {
        self.config.sessions.cleanup_interval = interval;
        self
    }

    /// Set session max age
    pub fn with_session_max_age(mut self, max_age: u64) -> Self {
        self.config.sessions.max_age = max_age;
        self
    }

    /// Enable/disable session cookies
    pub fn with_session_cookie(mut self, enabled: bool) -> Self {
        self.config.sessions.cookie_enabled = enabled;
        self
    }

    // ========================================================================
    // RBAC
    // ========================================================================

    /// Enable/disable RBAC
    pub fn with_rbac(mut self, enabled: bool) -> Self {
        self.config.rbac.enabled = enabled;
        self
    }

    /// Set default role
    pub fn with_default_role(mut self, role: impl Into<String>) -> Self {
        self.config.rbac.default_role = role.into();
        self
    }

    /// Enable/disable audit trail
    pub fn with_audit(mut self, enabled: bool) -> Self {
        self.config.rbac.audit_enabled = enabled;
        self
    }

    /// Enable/disable rate limiting
    pub fn with_rate_limit(mut self, enabled: bool) -> Self {
        self.config.rbac.rate_limit_enabled = enabled;
        self
    }

    // ========================================================================
    // REPLICATION
    // ========================================================================

    /// Enable/disable replication
    pub fn with_replication(mut self, enabled: bool) -> Self {
        self.config.replication.enabled = enabled;
        self
    }

    /// Set node ID
    pub fn with_node_id(mut self, id: impl Into<String>) -> Self {
        self.config.replication.node_id = id.into();
        self
    }

    /// Set cluster nodes
    pub fn with_cluster(mut self, nodes: Vec<String>) -> Self {
        self.config.replication.cluster_nodes = nodes;
        self
    }

    // ========================================================================
    // RAFT CLUSTER (Distributed Consensus)
    // ========================================================================

    /// Configure Raft cluster mode with peers
    ///
    /// This enables distributed consensus with automatic leader election,
    /// heartbeat monitoring, and write redirection to the leader.
    ///
    /// # Example
    /// ```rust,ignore
    /// LithairServer::new()
    ///     .with_port(8080)
    ///     .with_raft_cluster(1, vec!["127.0.0.1:8081", "127.0.0.1:8082"])
    ///     .with_model::<Product>("./data/products", "/api/products")
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_raft_cluster(mut self, node_id: u64, peers: Vec<impl Into<String>>) -> Self {
        self.node_id = Some(node_id);
        self.cluster_peers = peers.into_iter().map(|p| p.into()).collect();
        self.config.raft.enabled = true;
        self
    }

    /// Set the Raft configuration (path, auth, timeouts)
    ///
    /// # Example
    /// ```rust,ignore
    /// use lithair_core::config::RaftConfig;
    ///
    /// LithairServer::new()
    ///     .with_raft_cluster(1, vec!["127.0.0.1:8081"])
    ///     .with_raft_config(RaftConfig::new()
    ///         .with_path("/_internal/raft")
    ///         .with_auth("secret-token"))
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_raft_config(mut self, config: crate::config::RaftConfig) -> Self {
        self.config.raft = config;
        self
    }

    /// Set Raft endpoint path (default: "/raft")
    pub fn with_raft_path(mut self, path: impl Into<String>) -> Self {
        self.config.raft.path = path.into();
        self
    }

    /// Enable Raft authentication with token
    pub fn with_raft_auth(mut self, token: impl Into<String>) -> Self {
        self.config.raft.auth_required = true;
        self.config.raft.auth_token = Some(token.into());
        self
    }

    /// Set schema vote policy for cluster-wide schema consensus
    ///
    /// Controls how schema changes are handled in a cluster:
    /// - `additive`: Policy for safe changes (nullable fields, fields with defaults)
    /// - `breaking`: Policy for dangerous changes (removing fields, adding required fields)
    /// - `versioned`: Policy for structural changes (type changes, renames)
    ///
    /// # Example
    /// ```rust,ignore
    /// use lithair_core::schema::{SchemaVotePolicy, VoteStrategy};
    ///
    /// LithairServer::new()
    ///     .with_raft_cluster(1, vec!["127.0.0.1:8081"])
    ///     .with_schema_policy(SchemaVotePolicy {
    ///         additive: VoteStrategy::AutoAccept,      // Safe changes auto-accepted
    ///         breaking: VoteStrategy::Reject,          // Dangerous changes rejected
    ///         versioned: VoteStrategy::Consensus,      // Structural changes need vote
    ///     })
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_schema_policy(mut self, policy: crate::schema::SchemaVotePolicy) -> Self {
        self.schema_vote_policy = Some(policy);
        self
    }

    // ========================================================================
    // ADMIN PANEL
    // ========================================================================

    /// Enable/disable admin panel
    pub fn with_admin_panel(mut self, enabled: bool) -> Self {
        self.config.admin.enabled = enabled;
        self
    }

    /// Set admin panel path
    pub fn with_admin_path(mut self, path: impl Into<String>) -> Self {
        self.config.admin.path = path.into();
        self
    }

    /// Enable/disable admin authentication
    pub fn with_admin_auth(mut self, enabled: bool) -> Self {
        self.config.admin.auth_required = enabled;
        self
    }

    /// Enable/disable metrics
    pub fn with_metrics(mut self, enabled: bool) -> Self {
        self.config.admin.metrics_enabled = enabled;
        self
    }

    // ========================================================================
    // LOGGING
    // ========================================================================

    /// Set log level
    pub fn with_log_level(mut self, level: impl Into<String>) -> Self {
        self.config.logging.level = level.into();
        self
    }

    /// Set log format
    pub fn with_log_format(mut self, format: impl Into<String>) -> Self {
        self.config.logging.format = format.into();
        self
    }

    /// Enable/disable file logging
    pub fn with_log_file(mut self, enabled: bool) -> Self {
        self.config.logging.file_enabled = enabled;
        self
    }

    // ========================================================================
    // STORAGE
    // ========================================================================

    /// Set data directory
    pub fn with_data_dir(mut self, dir: impl Into<String>) -> Self {
        self.config.storage.data_dir = dir.into();
        self
    }

    /// Enable/disable backups
    pub fn with_backup(mut self, enabled: bool) -> Self {
        self.config.storage.backup_enabled = enabled;
        self
    }

    // ========================================================================
    // PERFORMANCE
    // ========================================================================

    /// Enable/disable cache
    pub fn with_cache(mut self, enabled: bool) -> Self {
        self.config.performance.cache_enabled = enabled;
        self
    }

    // ========================================================================
    // HTTP FEATURES (from DeclarativeServer)
    // ========================================================================

    /// Configure logging
    pub fn with_logging_config(mut self, config: crate::logging::LoggingConfig) -> Self {
        self.logging_config = Some(config);
        self
    }

    /// Configure readiness checks
    pub fn with_readiness_config(
        mut self,
        config: crate::http::declarative_server::ReadinessConfig,
    ) -> Self {
        self.readiness_config = Some(config);
        self
    }

    /// Configure observability endpoints
    pub fn with_observe_config(
        mut self,
        config: crate::http::declarative_server::ObserveConfig,
    ) -> Self {
        self.observe_config = Some(config);
        self
    }

    /// Configure performance endpoints
    pub fn with_perf_endpoints(
        mut self,
        config: crate::http::declarative_server::PerfEndpointsConfig,
    ) -> Self {
        self.perf_config = Some(config);
        self
    }

    /// Configure gzip compression
    pub fn with_gzip_config(mut self, config: crate::http::declarative_server::GzipConfig) -> Self {
        self.gzip_config = Some(config);
        self
    }

    /// Set route-specific policy
    pub fn with_route_policy(
        mut self,
        path: impl Into<String>,
        policy: crate::http::declarative_server::RoutePolicy,
    ) -> Self {
        self.route_policies.insert(path.into(), policy);
        self
    }

    /// Enable legacy endpoints
    pub fn with_legacy_endpoints(mut self, enabled: bool) -> Self {
        self.legacy_endpoints = enabled;
        self
    }

    /// Enable deprecation warnings
    pub fn with_deprecation_warnings(mut self, enabled: bool) -> Self {
        self.deprecation_warnings = enabled;
        self
    }

    /// Configure firewall
    pub fn with_firewall_config(mut self, config: crate::http::FirewallConfig) -> Self {
        self.firewall_config = Some(config);
        self
    }

    /// Configure anti-DDoS protection
    pub fn with_anti_ddos_config(
        mut self,
        config: crate::security::anti_ddos::AntiDDoSConfig,
    ) -> Self {
        self.anti_ddos_config = Some(config);
        self
    }

    /// Configure RBAC with automatic auth routes generation
    ///
    /// This method automatically:
    /// - Creates PersistentSessionStore for sessions
    /// - Registers POST /auth/login handler
    /// - Registers POST /auth/logout handler
    /// - Registers GET /auth/validate handler (session validation)
    /// - Returns PermissionChecker for use with models
    ///
    /// # Example
    /// ```ignore
    /// use lithair_core::rbac::{ServerRbacConfig, RbacUser};
    ///
    /// LithairServer::new()
    ///     .with_rbac_config(ServerRbacConfig::new()
    ///         .with_roles(vec![
    ///             ("Admin".to_string(), vec!["*".to_string()]),
    ///             ("Editor".to_string(), vec!["Read".to_string(), "Write".to_string()]),
    ///         ])
    ///         .with_users(vec![
    ///             RbacUser::new("admin", "password123", "Admin"),
    ///             RbacUser::new("editor", "password123", "Editor"),
    ///         ])
    ///         .with_session_store("./data/sessions")
    ///     )
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_rbac_config(mut self, config: crate::rbac::ServerRbacConfig) -> Self {
        use crate::rbac::{handle_rbac_login, handle_rbac_logout};
        use std::sync::Arc;

        // Create session store path (default if not provided)
        let session_path = config
            .session_store_path
            .clone()
            .unwrap_or_else(|| "./data/sessions".to_string());

        // Store session duration for handlers
        let session_duration = config.session_duration;

        // Clone users for handlers
        let users_login = config.users.clone();
        let users_clone = config.users.clone();

        // Create permission checker from config
        let permission_checker = config.create_permission_checker();

        // CRITICAL: Create SHARED session store for login AND models
        // This ensures all components use the SAME session storage
        let session_store_shared = {
            let path = std::path::PathBuf::from(session_path.clone());
            // Create it synchronously here (PersistentSessionStore::new is sync)
            match PersistentSessionStore::new(path) {
                Ok(store) => Arc::new(store) as Arc<dyn std::any::Any + Send + Sync>,
                Err(e) => {
                    log::error!("Failed to create session store: {}", e);
                    panic!("Cannot start without session store: {}", e);
                }
            }
        };

        // Store session manager AND permission checker for use by models
        self.session_manager = Some(session_store_shared.clone());
        self.permission_checker = Some(permission_checker);

        // Add login route
        let session_store_login = session_store_shared.clone();
        let mfa_storage_login = self.mfa_storage.clone();
        self = self.with_route(http::Method::POST, "/auth/login", move |req| {
            let users = users_login.clone();
            let duration = session_duration;
            let store_clone = session_store_login.clone();
            let mfa_clone = mfa_storage_login.clone();

            Box::pin(async move {
                // Use shared session store (already created above)
                let session_store: Arc<PersistentSessionStore> = store_clone
                    .downcast()
                    .map_err(|_| anyhow::anyhow!("Failed to downcast session store"))?;

                match handle_rbac_login(req, session_store, &users, duration, mfa_clone).await {
                    Ok(resp) => Ok(resp),
                    Err(e) => {
                        log::error!("Login error: {}", e);
                        Ok(hyper::Response::builder()
                            .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                            .body(http_body_util::Full::new(bytes::Bytes::from(format!(
                                "Internal error: {}",
                                e
                            ))))
                            .unwrap())
                    }
                }
            })
        });

        // Add logout route
        let session_store_logout = session_store_shared.clone();
        self = self.with_route(http::Method::POST, "/auth/logout", move |req| {
            let store_clone = session_store_logout.clone();

            Box::pin(async move {
                // Use shared session store
                let session_store: Arc<PersistentSessionStore> = store_clone
                    .downcast()
                    .map_err(|_| anyhow::anyhow!("Failed to downcast session store"))?;

                match handle_rbac_logout(req, session_store).await {
                    Ok(resp) => Ok(resp),
                    Err(e) => {
                        log::error!("Logout error: {}", e);
                        Ok(hyper::Response::builder()
                            .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                            .body(http_body_util::Full::new(bytes::Bytes::from(format!(
                                "Internal error: {}",
                                e
                            ))))
                            .unwrap())
                    }
                }
            })
        });

        // Add validate route (GET endpoint for session validation)
        let session_store_validate = session_store_shared.clone();
        self = self.with_route(http::Method::GET, "/auth/validate", move |req| {
            let store_clone = session_store_validate.clone();

            Box::pin(async move {
                use crate::session::SessionStore;
                use bytes::Bytes;
                use http_body_util::Full;

                // Use shared session store
                let session_store: Arc<PersistentSessionStore> = store_clone
                    .downcast()
                    .map_err(|_| anyhow::anyhow!("Failed to downcast session store"))?;

                // Extract token from Authorization header or Cookie
                let token = req
                    .headers()
                    .get(hyper::header::AUTHORIZATION)
                    .and_then(|h| h.to_str().ok())
                    .and_then(|h| h.strip_prefix("Bearer "))
                    .or_else(|| {
                        req.headers()
                            .get(hyper::header::COOKIE)
                            .and_then(|h| h.to_str().ok())
                            .and_then(|cookies| {
                                cookies
                                    .split(';')
                                    .find(|c| c.trim().starts_with("session_token="))
                                    .and_then(|c| c.split('=').nth(1))
                            })
                    });

                let is_valid = if let Some(token) = token {
                    session_store.get(token).await.ok().flatten().is_some()
                } else {
                    false
                };

                Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::OK)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(format!(r#"{{"valid":{}}}"#, is_valid))))
                    .unwrap())
            })
        });

        log::info!(
            "RBAC configured with {} roles and {} users",
            config.roles.len(),
            users_clone.len()
        );
        log::info!("   POST /auth/login - Authentication endpoint");
        log::info!("   POST /auth/logout - Logout endpoint");
        log::info!("   GET /auth/validate - Session validation endpoint");

        self
    }

    /// Configure MFA/TOTP with automatic route generation
    ///
    /// This method automatically:
    /// - Creates MfaStorage for secure secret persistence
    /// - Registers GET /auth/mfa/status - Check MFA status
    /// - Registers POST /auth/mfa/setup - Generate secret + QR code
    /// - Registers POST /auth/mfa/enable - Activate MFA (verify code)
    /// - Registers POST /auth/mfa/disable - Deactivate MFA
    /// - Registers POST /auth/mfa/verify - Validate TOTP code
    ///
    /// # Example
    /// ```ignore
    /// use lithair_core::mfa::MfaConfig;
    ///
    /// LithairServer::new()
    ///     .with_rbac_config(rbac_config)
    ///     .with_mfa_totp(MfaConfig {
    ///         issuer: "Lithair Blog".to_string(),
    ///         enforce_for_roles: vec!["Admin".to_string()],
    ///         ..Default::default()
    ///     })
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_mfa_totp(mut self, config: crate::mfa::MfaConfig) -> Self {
        use crate::mfa::{handlers, MfaStorage};
        use std::sync::Arc;

        // Create MFA storage
        let storage = MfaStorage::new(&config.storage_path).expect("Failed to create MFA storage");
        let storage_arc = Arc::new(storage);

        // Store for use in other routes
        self.mfa_storage = Some(storage_arc.clone());

        let config_arc = Arc::new(config.clone());

        // Auto-generate MFA routes

        // GET /auth/mfa/status
        let storage_status = storage_arc.clone();
        let config_status = config_arc.clone();
        self = self.with_route(http::Method::GET, "/auth/mfa/status", move |req| {
            let storage = storage_status.clone();
            let config = config_status.clone();
            Box::pin(async move {
                handlers::handle_mfa_status(storage, config, req)
                    .await
                    .map_err(|e| anyhow::anyhow!("MFA status error: {}", e))
            })
        });

        // POST /auth/mfa/setup
        let storage_setup = storage_arc.clone();
        let config_setup = config_arc.clone();
        self = self.with_route(http::Method::POST, "/auth/mfa/setup", move |req| {
            let storage = storage_setup.clone();
            let config = config_setup.clone();
            Box::pin(async move {
                handlers::handle_mfa_setup(storage, config, req)
                    .await
                    .map_err(|e| anyhow::anyhow!("MFA setup error: {}", e))
            })
        });

        // POST /auth/mfa/enable
        let storage_enable = storage_arc.clone();
        let config_enable = config_arc.clone();
        self = self.with_route(http::Method::POST, "/auth/mfa/enable", move |req| {
            let storage = storage_enable.clone();
            let config = config_enable.clone();
            Box::pin(async move {
                handlers::handle_mfa_enable(storage, config, req)
                    .await
                    .map_err(|e| anyhow::anyhow!("MFA enable error: {}", e))
            })
        });

        // POST /auth/mfa/disable
        let storage_disable = storage_arc.clone();
        let config_disable = config_arc.clone();
        self = self.with_route(http::Method::POST, "/auth/mfa/disable", move |req| {
            let storage = storage_disable.clone();
            let config = config_disable.clone();
            Box::pin(async move {
                handlers::handle_mfa_disable(storage, config, req)
                    .await
                    .map_err(|e| anyhow::anyhow!("MFA disable error: {}", e))
            })
        });

        // POST /auth/mfa/verify
        let storage_verify = storage_arc.clone();
        let config_verify = config_arc.clone();
        self = self.with_route(http::Method::POST, "/auth/mfa/verify", move |req| {
            let storage = storage_verify.clone();
            let config = config_verify.clone();
            Box::pin(async move {
                handlers::handle_mfa_verify(storage, config, req)
                    .await
                    .map_err(|e| anyhow::anyhow!("MFA verify error: {}", e))
            })
        });

        log::info!("MFA/TOTP configured");
        log::info!("   Issuer: {}", config.issuer);
        log::info!("   GET /auth/mfa/status - Check MFA status");
        log::info!("   POST /auth/mfa/setup - Generate secret + QR code");
        log::info!("   POST /auth/mfa/enable - Activate MFA");
        log::info!("   POST /auth/mfa/disable - Deactivate MFA");
        log::info!("   POST /auth/mfa/verify - Validate TOTP code");
        log::info!("   Storage: {}", config.storage_path);

        self
    }

    /// Serve static files from a directory at root path (memory-first with SCC2)
    ///
    /// This loads all files from the specified directory into memory
    /// and serves them with ultra-low latency (40M+ ops/sec) at the root path "/".
    ///
    /// # Example
    /// ```ignore
    /// LithairServer::new()
    ///     .with_frontend("public")  // Serves public/ from memory at /
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_frontend(mut self, static_dir: impl Into<String>) -> Self {
        // Store the static directory path and enable frontend (legacy support)
        self.config.frontend.static_dir = Some(static_dir.into());
        self.config.frontend.enabled = true;
        self
    }

    /// Serve static files from a directory at a specific path prefix (memory-first with SCC2)
    ///
    /// This loads all files from the specified directory into memory
    /// and serves them with ultra-low latency (40M+ ops/sec) at the specified route prefix.
    ///
    /// You can call this multiple times to serve different frontends at different paths.
    ///
    /// # Arguments
    /// * `route_prefix` - URL path prefix (e.g., "/", "/admin", "/dashboard")
    /// * `static_dir` - Filesystem directory containing static files
    ///
    /// # Example
    /// ```ignore
    /// LithairServer::new()
    ///     .with_frontend_at("/", "public")         // Public frontend at /
    ///     .with_frontend_at("/admin", "admin-ui")  // Admin frontend at /admin/*
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_frontend_at(
        mut self,
        route_prefix: impl Into<String>,
        static_dir: impl Into<String>,
    ) -> Self {
        self.frontend_configs.push((route_prefix.into(), static_dir.into()));
        self
    }

    // ========================================================================
    // ROUTES & MODELS
    // ========================================================================

    /// Register an existing DeclarativeHttpHandler with automatic CRUD route generation
    ///
    /// This method automatically registers all CRUD routes for the given handler:
    /// - `GET /base_path` - List all items
    /// - `POST /base_path` - Create new item
    /// - `GET /base_path/*` - Get single item by ID
    /// - `PUT /base_path/*` - Update item by ID
    /// - `DELETE /base_path/*` - Delete item by ID
    ///
    /// Unlike `with_model<T>`, this method allows you to keep a reference to the handler
    /// for use in custom routes (e.g., FK queries, JOIN queries).
    ///
    /// # Example
    /// ```ignore
    /// use lithair_core::http::DeclarativeHttpHandler;
    /// use std::sync::Arc;
    ///
    /// let products_handler = Arc::new(DeclarativeHttpHandler::<Product>::new_with_replay("./data/products")?);
    /// let orders_handler = Arc::new(DeclarativeHttpHandler::<Order>::new_with_replay("./data/orders")?);
    ///
    /// // Clone handlers before passing to with_handler (it takes ownership via move)
    /// let products_for_custom = products_handler.clone();
    ///
    /// LithairServer::new()
    ///     .with_handler(products_handler, "/api/products")
    ///     .with_handler(orders_handler.clone(), "/api/orders")
    ///     // Custom routes using the cloned handler reference
    ///     .with_route(GET, "/api/orders/*/expanded", move |req| {
    ///         let handler = orders_handler.clone();
    ///         // ... use handler for custom queries
    ///     })
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_handler<T>(
        mut self,
        handler: std::sync::Arc<crate::http::DeclarativeHttpHandler<T>>,
        base_path: impl Into<String>,
    ) -> Self
    where
        T: Clone
            + Send
            + Sync
            + crate::http::HttpExposable
            + crate::lifecycle::LifecycleAware
            + crate::consensus::ReplicatedModel
            + 'static,
    {
        let base_path_str = base_path.into();
        let base_path_normalized = base_path_str.trim_end_matches('/').to_string();

        // GET /base_path - List all
        let handler_list = handler.clone();
        let path_list = base_path_normalized.clone();
        self = self.with_route(http::Method::GET, base_path_normalized.clone(), move |req| {
            let h = handler_list.clone();
            let bp = path_list.clone();
            Box::pin(async move {
                let segments: Vec<&str> = Vec::new();
                match h.handle_request(req, &segments).await {
                    Ok(resp) => {
                        // Convert BoxBody to Full<Bytes>
                        use http_body_util::BodyExt;
                        let (parts, body) = resp.into_parts();
                        let bytes = body.collect().await.map(|c| c.to_bytes()).unwrap_or_default();
                        Ok(hyper::Response::from_parts(parts, http_body_util::Full::new(bytes)))
                    }
                    Err(_infallible) => {
                        log::error!("Handler error for GET {}", bp);
                        Ok(hyper::Response::builder()
                            .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                            .body(http_body_util::Full::new(bytes::Bytes::from("Internal error")))
                            .unwrap())
                    }
                }
            })
        });

        // POST /base_path - Create
        let handler_create = handler.clone();
        let path_create = base_path_normalized.clone();
        self = self.with_route(http::Method::POST, base_path_normalized.clone(), move |req| {
            let h = handler_create.clone();
            let bp = path_create.clone();
            Box::pin(async move {
                let segments: Vec<&str> = Vec::new();
                match h.handle_request(req, &segments).await {
                    Ok(resp) => {
                        use http_body_util::BodyExt;
                        let (parts, body) = resp.into_parts();
                        let bytes = body.collect().await.map(|c| c.to_bytes()).unwrap_or_default();
                        Ok(hyper::Response::from_parts(parts, http_body_util::Full::new(bytes)))
                    }
                    Err(_infallible) => {
                        log::error!("Handler error for POST {}", bp);
                        Ok(hyper::Response::builder()
                            .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                            .body(http_body_util::Full::new(bytes::Bytes::from("Internal error")))
                            .unwrap())
                    }
                }
            })
        });

        // GET /base_path/* - Get single
        let handler_get = handler.clone();
        let base_for_get = base_path_normalized.clone();
        self =
            self.with_route(http::Method::GET, format!("{}/*", base_path_normalized), move |req| {
                let h = handler_get.clone();
                let bp = base_for_get.clone();
                Box::pin(async move {
                    // Extract path segment (ID) from URL
                    let path = req.uri().path().to_string();
                    let segments: Vec<&str> = path
                        .strip_prefix(&bp)
                        .unwrap_or("")
                        .trim_start_matches('/')
                        .split('/')
                        .filter(|s| !s.is_empty())
                        .collect();

                    match h.handle_request(req, &segments).await {
                        Ok(resp) => {
                            use http_body_util::BodyExt;
                            let (parts, body) = resp.into_parts();
                            let bytes =
                                body.collect().await.map(|c| c.to_bytes()).unwrap_or_default();
                            Ok(hyper::Response::from_parts(parts, http_body_util::Full::new(bytes)))
                        }
                        Err(_infallible) => {
                            log::error!("Handler error for GET {}/*", bp);
                            Ok(hyper::Response::builder()
                                .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                                .body(http_body_util::Full::new(bytes::Bytes::from(
                                    "Internal error",
                                )))
                                .unwrap())
                        }
                    }
                })
            });

        // PUT /base_path/* - Update
        let handler_put = handler.clone();
        let base_for_put = base_path_normalized.clone();
        self =
            self.with_route(http::Method::PUT, format!("{}/*", base_path_normalized), move |req| {
                let h = handler_put.clone();
                let bp = base_for_put.clone();
                Box::pin(async move {
                    let path = req.uri().path().to_string();
                    let segments: Vec<&str> = path
                        .strip_prefix(&bp)
                        .unwrap_or("")
                        .trim_start_matches('/')
                        .split('/')
                        .filter(|s| !s.is_empty())
                        .collect();

                    match h.handle_request(req, &segments).await {
                        Ok(resp) => {
                            use http_body_util::BodyExt;
                            let (parts, body) = resp.into_parts();
                            let bytes =
                                body.collect().await.map(|c| c.to_bytes()).unwrap_or_default();
                            Ok(hyper::Response::from_parts(parts, http_body_util::Full::new(bytes)))
                        }
                        Err(_infallible) => {
                            log::error!("Handler error for PUT {}/*", bp);
                            Ok(hyper::Response::builder()
                                .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                                .body(http_body_util::Full::new(bytes::Bytes::from(
                                    "Internal error",
                                )))
                                .unwrap())
                        }
                    }
                })
            });

        // DELETE /base_path/* - Delete
        let handler_del = handler.clone();
        let base_for_del = base_path_normalized.clone();
        self = self.with_route(
            http::Method::DELETE,
            format!("{}/*", base_path_normalized),
            move |req| {
                let h = handler_del.clone();
                let bp = base_for_del.clone();
                Box::pin(async move {
                    let path = req.uri().path().to_string();
                    let segments: Vec<&str> = path
                        .strip_prefix(&bp)
                        .unwrap_or("")
                        .trim_start_matches('/')
                        .split('/')
                        .filter(|s| !s.is_empty())
                        .collect();

                    match h.handle_request(req, &segments).await {
                        Ok(resp) => {
                            use http_body_util::BodyExt;
                            let (parts, body) = resp.into_parts();
                            let bytes =
                                body.collect().await.map(|c| c.to_bytes()).unwrap_or_default();
                            Ok(hyper::Response::from_parts(parts, http_body_util::Full::new(bytes)))
                        }
                        Err(_infallible) => {
                            log::error!("Handler error for DELETE {}/*", bp);
                            Ok(hyper::Response::builder()
                                .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                                .body(http_body_util::Full::new(bytes::Bytes::from(
                                    "Internal error",
                                )))
                                .unwrap())
                        }
                    }
                })
            },
        );

        let name = std::any::type_name::<T>().split("::").last().unwrap_or("Unknown");
        log::info!("Registered handler for {} at {}", name, base_path_normalized);
        log::info!("   GET {} - List all", base_path_normalized);
        log::info!("   POST {} - Create", base_path_normalized);
        log::info!("   GET {}/* - Get by ID", base_path_normalized);
        log::info!("   PUT {}/* - Update by ID", base_path_normalized);
        log::info!("   DELETE {}/* - Delete by ID", base_path_normalized);

        self
    }

    /// Add a custom route with an async handler.
    ///
    /// Custom routes are dispatched **before** model routes, so a custom
    /// `/api/items/search` will take priority over the DeclarativeModel
    /// prefix match on `/api/items`.
    ///
    /// The handler receives a `hyper::Request<Incoming>` and returns a
    /// `Result<Response<Full<Bytes>>>`.
    ///
    /// # Example
    /// ```ignore
    /// use bytes::Bytes;
    /// use http_body_util::Full;
    /// use hyper::{Request, Response, body::Incoming};
    ///
    /// fn health(
    ///     _req: Request<Incoming>,
    /// ) -> std::pin::Pin<Box<dyn std::future::Future<
    ///     Output = anyhow::Result<Response<Full<Bytes>>>,
    /// > + Send>> {
    ///     Box::pin(async {
    ///         Ok(Response::builder()
    ///             .status(200)
    ///             .body(Full::new(Bytes::from(r#"{"status":"ok"}"#)))
    ///             .unwrap())
    ///     })
    /// }
    ///
    /// LithairServer::new()
    ///     .with_route(Method::GET, "/health", health)
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_route<F>(
        mut self,
        method: http::Method,
        path: impl Into<String>,
        handler: F,
    ) -> Self
    where
        F: Fn(
                hyper::Request<hyper::body::Incoming>,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = Result<hyper::Response<http_body_util::Full<bytes::Bytes>>>,
                        > + Send,
                >,
            > + Send
            + Sync
            + 'static,
    {
        self.custom_routes.push(CustomRoute {
            method,
            path: path.into(),
            handler: Arc::new(handler),
        });
        self
    }

    /// Set a custom handler for 404 Not Found responses.
    ///
    /// When set, this handler is called instead of the default JSON 404 response
    /// whenever no route matches the incoming request.
    ///
    /// The handler has the same signature as `with_route` handlers.
    ///
    /// # Example
    /// ```ignore
    /// use lithair_core::app::response;
    /// use http::StatusCode;
    ///
    /// LithairServer::new()
    ///     .with_not_found_handler(|_req| {
    ///         Box::pin(async {
    ///             Ok(response::html(StatusCode::NOT_FOUND, "<h1>Page not found</h1>"))
    ///         })
    ///     })
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_not_found_handler<F>(mut self, handler: F) -> Self
    where
        F: Fn(
                hyper::Request<hyper::body::Incoming>,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = Result<hyper::Response<http_body_util::Full<bytes::Bytes>>>,
                        > + Send,
                >,
            > + Send
            + Sync
            + 'static,
    {
        self.not_found_handler = Some(Arc::new(handler));
        self
    }

    /// Add declarative route guard for authentication, authorization, etc.
    ///
    /// # Example
    /// ```ignore
    /// .with_route_guard("/admin/*", RouteGuard::RequireAuth {
    ///     redirect_to: Some("/login".to_string()),
    ///     exclude: vec!["/login".to_string()],
    /// })
    /// ```
    pub fn with_route_guard(
        mut self,
        pattern: impl Into<String>,
        guard: crate::http::RouteGuard,
    ) -> Self {
        self.route_guards.push(crate::http::RouteGuardMatcher {
            pattern: pattern.into(),
            methods: None,
            guard,
        });
        self
    }

    /// Add declarative route guard with specific HTTP methods
    pub fn with_route_guard_methods(
        mut self,
        pattern: impl Into<String>,
        methods: Vec<http::Method>,
        guard: crate::http::RouteGuard,
    ) -> Self {
        self.route_guards.push(crate::http::RouteGuardMatcher {
            pattern: pattern.into(),
            methods: Some(methods),
            guard,
        });
        self
    }

    /// Register a model with automatic CRUD generation
    pub fn with_model_full<T>(
        mut self,
        data_path: impl Into<String>,
        base_path: impl Into<String>,
        permission_checker: Option<Arc<dyn crate::rbac::PermissionChecker>>,
        session_store: Option<Arc<dyn std::any::Any + Send + Sync>>,
    ) -> Self
    where
        T: crate::http::HttpExposable
            + crate::lifecycle::LifecycleAware
            + crate::consensus::ReplicatedModel
            + 'static,
    {
        use crate::app::{DeclarativeModelHandler, ModelRegistrationInfo};
        use std::sync::Arc;

        let name = std::any::type_name::<T>().split("::").last().unwrap_or("Unknown");
        let data_path_str = data_path.into();
        let base_path_str = base_path.into();

        // Use provided OR fallback to builder's values
        let effective_session_store = session_store.or_else(|| self.session_manager.clone());
        let effective_permission_checker =
            permission_checker.or_else(|| self.permission_checker.clone());

        // Create factory that will create the handler async in serve()
        let factory: crate::app::ModelFactory = Arc::new(move |data_path: String| {
            let pc = effective_permission_checker.clone();
            let ss = effective_session_store.clone();
            Box::pin(async move {
                let mut handler = DeclarativeModelHandler::<T>::new(data_path)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to create handler: {}", e))?;

                // Configure permission checker and session store
                if let Some(checker) = pc {
                    handler = handler.with_permission_checker(checker);
                }
                if let Some(store) = ss {
                    handler = handler.set_session_store_any(store);
                }

                Ok(Arc::new(handler) as Arc<dyn crate::app::ModelHandler>)
            })
        });

        self.model_infos.push(ModelRegistrationInfo {
            name: name.to_string(),
            base_path: base_path_str,
            data_path: data_path_str,
            factory,
            schema_extractor: None,
        });

        self
    }

    /// Register a model with automatic CRUD generation (simple version without RBAC).
    ///
    /// Generates GET, POST, PUT, DELETE endpoints under `base_path`.
    /// Events are persisted to `data_path`.
    ///
    /// For schema migration support, use `with_declarative_model` instead.
    ///
    /// # Example
    /// ```ignore
    /// use lithair_core::prelude::*;
    /// use lithair_macros::DeclarativeModel;
    /// use serde::{Serialize, Deserialize};
    ///
    /// #[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
    /// struct Todo {
    ///     #[http(expose)]
    ///     id: String,
    ///     #[http(expose)]
    ///     title: String,
    /// }
    ///
    /// LithairServer::new()
    ///     .with_model::<Todo>("./data/todos", "/api/todos")
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_model<T>(
        mut self,
        data_path: impl Into<String>,
        base_path: impl Into<String>,
    ) -> Self
    where
        T: crate::http::HttpExposable
            + crate::lifecycle::LifecycleAware
            + crate::consensus::ReplicatedModel
            + 'static,
    {
        use crate::app::{DeclarativeModelHandler, ModelRegistrationInfo};
        use std::sync::Arc;

        let name = std::any::type_name::<T>().split("::").last().unwrap_or("Unknown");
        let data_path_str = data_path.into();
        let base_path_str = base_path.into();

        // Create factory that will create the handler async in serve()
        let factory: crate::app::ModelFactory = Arc::new(move |data_path: String| {
            Box::pin(async move {
                let handler = DeclarativeModelHandler::<T>::new(data_path)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to create handler: {}", e))?;
                Ok(Arc::new(handler) as Arc<dyn crate::app::ModelHandler>)
            })
        });

        self.model_infos.push(ModelRegistrationInfo {
            name: name.to_string(),
            base_path: base_path_str,
            data_path: data_path_str,
            factory,
            schema_extractor: None,
        });

        self
    }

    /// Register a DeclarativeModel with schema migration support
    ///
    /// This method enables automatic schema change detection at startup.
    /// If the model's schema has changed since last run, changes are logged
    /// and optionally auto-migrated based on configuration.
    ///
    /// # Example
    /// ```rust,ignore
    /// LithairServer::new()
    ///     .with_port(8080)
    ///     .with_declarative_model::<Product>("./data/products", "/api/products")
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_declarative_model<T>(
        mut self,
        data_path: impl Into<String>,
        base_path: impl Into<String>,
    ) -> Self
    where
        T: crate::http::HttpExposable
            + crate::lifecycle::LifecycleAware
            + crate::consensus::ReplicatedModel
            + crate::schema::HasSchemaSpec
            + 'static,
    {
        use crate::app::{DeclarativeModelHandler, ModelRegistrationInfo, SchemaSpecExtractor};
        use std::sync::Arc;

        let name = <T as crate::schema::HasSchemaSpec>::model_name();
        let data_path_str = data_path.into();
        let base_path_str = base_path.into();

        // Create factory that will create the handler async in serve()
        let factory: crate::app::ModelFactory = Arc::new(move |data_path: String| {
            Box::pin(async move {
                let handler = DeclarativeModelHandler::<T>::new(data_path)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to create handler: {}", e))?;
                Ok(Arc::new(handler) as Arc<dyn crate::app::ModelHandler>)
            })
        });

        // Create schema extractor for migration detection
        let schema_extractor: SchemaSpecExtractor = Arc::new(|| T::schema_spec());

        self.model_infos.push(ModelRegistrationInfo {
            name: name.to_string(),
            base_path: base_path_str,
            data_path: data_path_str,
            factory,
            schema_extractor: Some(schema_extractor),
        });

        self
    }

    // ========================================================================
    // DATA ADMIN - Database management API for admin dashboards
    // ========================================================================

    /// Enable data admin endpoints for database management
    ///
    /// This adds the following admin endpoints:
    /// - `GET /_admin/data/models` - List all registered models with stats
    /// - `GET /_admin/data/models/{name}` - Get model info and data
    /// - `GET /_admin/data/models/{name}/export` - Export model data as JSON
    /// - `GET /_admin/data/routes` - List all registered API routes
    /// - `POST /_admin/data/backup` - Trigger full data backup
    ///
    /// These endpoints require authentication if RBAC is configured.
    ///
    /// # Example
    /// ```rust,ignore
    /// LithairServer::new()
    ///     .with_model::<Article>("./data/articles", "/api/articles")
    ///     .with_data_admin() // Enable admin endpoints
    ///     .serve()
    ///     .await?;
    /// ```
    pub fn with_data_admin(mut self) -> Self {
        log::info!("Data Admin API enabled");
        log::info!("   GET  /_admin/data/models        - List models");
        log::info!("   GET  /_admin/data/models/{{name}} - Model data");
        log::info!("   GET  /_admin/data/routes        - List routes");
        log::info!("   POST /_admin/data/backup        - Backup all");

        // Note: The actual endpoint handlers are registered in LithairServer::serve()
        // after models are initialized. Here we just set a flag.
        self.config.admin.data_admin_enabled = true;

        self
    }

    /// Enable embedded data admin UI at the specified path (with automatic auth protection)
    ///
    /// This serves an embedded dashboard for browsing and managing data.
    /// Requires the `admin-ui` feature to be enabled.
    ///
    /// **Security**: The dashboard and API are automatically protected with RouteGuard::RequireAuth
    /// when RBAC is configured. Use `.with_data_admin_ui_public()` if you want no auth.
    ///
    /// # Example
    /// ```rust,ignore
    /// LithairServer::new()
    ///     .with_rbac_config(rbac_config)  // Auth is automatically applied
    ///     .with_model::<Article>("./data/articles", "/api/articles")
    ///     .with_data_admin()              // Enable API endpoints
    ///     .with_data_admin_ui("/_data")   // Enable embedded dashboard (auto-secured)
    ///     .serve()
    ///     .await?;
    /// ```
    ///
    /// The dashboard will be available at:
    /// - `/_data/` - Main dashboard page (requires authentication)
    ///
    /// It uses the `/_admin/data/*` API endpoints automatically.
    #[cfg(feature = "admin-ui")]
    pub fn with_data_admin_ui(mut self, path: impl Into<String>) -> Self {
        let ui_path = path.into();
        log::info!("Data Admin UI enabled at {}", ui_path);

        // Also enable the API if not already enabled
        if !self.config.admin.data_admin_enabled {
            self.config.admin.data_admin_enabled = true;
            log::info!("   (API endpoints auto-enabled)");
        }

        // Store the UI path in admin config
        self.config.admin.data_admin_ui_path = Some(ui_path.clone());

        // Auto-apply authentication guard to data admin paths
        // This protects both the UI and API endpoints
        log::info!("   Auth protection enabled (RequireAuth)");

        // Protect the UI path
        self.route_guards.push(crate::http::RouteGuardMatcher {
            pattern: format!("{}/*", ui_path),
            methods: None,
            guard: crate::http::RouteGuard::RequireAuth {
                redirect_to: Some("/login".to_string()),
                exclude: vec![],
            },
        });

        // Also protect the exact UI path
        self.route_guards.push(crate::http::RouteGuardMatcher {
            pattern: ui_path,
            methods: None,
            guard: crate::http::RouteGuard::RequireAuth {
                redirect_to: Some("/login".to_string()),
                exclude: vec![],
            },
        });

        // Protect the API endpoints
        self.route_guards.push(crate::http::RouteGuardMatcher {
            pattern: "/_admin/data/*".to_string(),
            methods: None,
            guard: crate::http::RouteGuard::RequireAuth {
                redirect_to: None, // Return 401 JSON for API calls
                exclude: vec![],
            },
        });

        self
    }

    /// Enable embedded data admin UI WITHOUT authentication (for development/internal use only)
    ///
    /// WARNING: This exposes your data without any authentication!
    /// Only use this for local development or behind a VPN/firewall.
    ///
    /// For production, use `.with_data_admin_ui()` which auto-applies auth guards.
    #[cfg(feature = "admin-ui")]
    pub fn with_data_admin_ui_public(mut self, path: impl Into<String>) -> Self {
        let ui_path = path.into();
        log::warn!("Data Admin UI enabled at {} (NO AUTH - development only!)", ui_path);

        // Also enable the API if not already enabled
        if !self.config.admin.data_admin_enabled {
            self.config.admin.data_admin_enabled = true;
            log::info!("   (API endpoints auto-enabled)");
        }

        // Store the UI path in admin config
        self.config.admin.data_admin_ui_path = Some(ui_path);

        self
    }

    // ========================================================================
    // BUILD
    // ========================================================================

    /// Build the server
    pub fn build(self) -> Result<LithairServer> {
        Ok(LithairServer {
            config: self.config,
            session_manager: self.session_manager,
            custom_routes: self.custom_routes,
            not_found_handler: self.not_found_handler,
            route_guards: self.route_guards,
            model_infos: self.model_infos,
            models: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            frontend_configs: self.frontend_configs, // Frontend configs to load in serve()
            frontend_engines: std::collections::HashMap::new(), // Will be populated in serve()
            logging_config: self.logging_config,
            readiness_config: self.readiness_config,
            observe_config: self.observe_config,
            perf_config: self.perf_config,
            gzip_config: self.gzip_config,
            route_policies: self.route_policies,
            firewall_config: self.firewall_config,
            anti_ddos_config: self.anti_ddos_config,
            legacy_endpoints: self.legacy_endpoints,
            deprecation_warnings: self.deprecation_warnings,
            // Raft cluster
            cluster_peers: self.cluster_peers.clone(),
            node_id: self.node_id,
            raft_state: None,       // Initialized in serve() if cluster mode enabled
            raft_crud_sender: None, // Initialized in serve() if cluster mode enabled
            // Initialize consensus log only if cluster mode is enabled
            consensus_log: if !self.cluster_peers.is_empty() {
                Some(Arc::new(crate::cluster::ConsensusLog::new()))
            } else {
                None
            },
            // Initialize WAL for durability (only in cluster mode)
            wal: if !self.cluster_peers.is_empty() {
                let wal_path = format!("./data/raft/node_{}/wal", self.node_id.unwrap_or(0));
                match crate::cluster::WriteAheadLog::new(&wal_path) {
                    Ok(wal) => {
                        log::info!("WAL initialized at {}", wal_path);
                        Some(Arc::new(wal))
                    }
                    Err(e) => {
                        log::warn!("Failed to initialize WAL: {}", e);
                        None
                    }
                }
            } else {
                None
            },
            // Initialize replication batcher for intelligent batching
            replication_batcher: if !self.cluster_peers.is_empty() {
                let batcher = crate::cluster::ReplicationBatcher::with_default_config();
                Some(Arc::new(batcher))
            } else {
                None
            },
            // Initialize snapshot manager for resync
            snapshot_manager: if !self.cluster_peers.is_empty() {
                let snapshot_path =
                    format!("./data/raft/node_{}/snapshots", self.node_id.unwrap_or(0));
                match crate::cluster::SnapshotManager::new(&snapshot_path) {
                    Ok(mgr) => {
                        log::info!("Snapshot manager initialized at {}", snapshot_path);
                        Some(Arc::new(tokio::sync::RwLock::new(mgr)))
                    }
                    Err(e) => {
                        log::warn!("Failed to initialize snapshot manager: {}", e);
                        None
                    }
                }
            } else {
                None
            },
            // Initialize migration manager for rolling upgrades
            migration_manager: if !self.cluster_peers.is_empty() {
                log::info!("Migration manager initialized for rolling upgrades");
                Some(Arc::new(crate::cluster::MigrationManager::default()))
            } else {
                None
            },
            // Resync stats for observability
            resync_stats: Arc::new(crate::cluster::ResyncStats::new()),
            // Schema sync state for cluster-wide schema consensus
            schema_sync_state: Arc::new(tokio::sync::RwLock::new(
                self.schema_vote_policy
                    .map(crate::schema::SchemaSyncState::with_policy)
                    .unwrap_or_default(),
            )),
        })
    }

    /// Build and start the server
    pub async fn serve(self) -> Result<()> {
        let server = self.build()?;
        server.serve().await
    }
}

impl Default for LithairServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}
