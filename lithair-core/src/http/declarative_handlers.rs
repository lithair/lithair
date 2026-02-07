//! Declarative Handlers System for Lithair
//!
//! Revolutionary Data-First approach to HTTP routing where users declare WHAT they want,
//! not HOW to implement it. Zero boilerplate, pure configuration.
//!
//! # Philosophy
//! Traditional frameworks force users to write imperative routing logic with if/else chains.
//! Lithair eliminates this by letting users declare their desired behavior and generating
//! all routing logic automatically.
//!
//! # Example
//! ```rust
//! let config = DeclarativeHandlerConfig {
//!     handlers: vec![
//!         HandlerDeclaration {
//!             path_prefix: "/admin/sites/".to_string(),
//!             handler_type: HandlerType::Admin(AdminHandlerConfig::automatic()),
//!         },
//!         HandlerDeclaration {
//!             path_prefix: "/api/".to_string(),
//!             handler_type: HandlerType::ApiProxy(ApiProxyConfig::declarative_crud()),
//!         },
//!         HandlerDeclaration {
//!             path_prefix: "/".to_string(),
//!             handler_type: HandlerType::Frontend(FrontendHandlerConfig::memory_first()),
//!         },
//!     ]
//! };
//! ```

use crate::http::firewall::Firewall;
use crate::http::{AutoAdminConfig, Req, Resp};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;

/// Declarative Handler Configuration - Pure Data-First Approach
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeclarativeHandlerConfig {
    /// List of handler declarations in priority order (first match wins)
    pub handlers: Vec<HandlerDeclaration>,
    /// Global configuration applied to all handlers
    pub global: GlobalHandlerConfig,
}

/// Individual handler declaration - describes WHAT, not HOW
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandlerDeclaration {
    /// Path prefix that triggers this handler (e.g., "/admin/", "/api/", "/")
    pub path_prefix: String,
    /// Type of handler and its configuration
    pub handler_type: HandlerType,
    /// Optional name for logging and debugging
    pub name: Option<String>,
    /// Whether this handler is enabled
    pub enabled: bool,
}

/// Types of declarative handlers available
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HandlerType {
    /// Automatic administration handler
    Admin(AdminHandlerConfig),
    /// API proxy handler for backend services
    ApiProxy(ApiProxyConfig),
    /// Frontend asset serving handler
    Frontend(FrontendHandlerConfig),
    /// Custom callback handler
    Custom(CustomHandlerConfig),
}

/// Configuration for automatic admin handler
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminHandlerConfig {
    /// Automatic admin endpoints configuration
    pub auto_admin: AutoAdminConfig,
    /// Optional firewall protection
    pub firewall_enabled: bool,
}

/// Configuration for API proxy handler
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiProxyConfig {
    /// Enable automatic CRUD generation for declarative models
    pub auto_crud: bool,
    /// Backend services to proxy to
    pub proxy_targets: Vec<ProxyTarget>,
}

/// Configuration for frontend handler
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontendHandlerConfig {
    /// Memory-first serving mode
    pub memory_first: bool,
    /// Development hot-reload mode
    pub dev_mode: bool,
    /// Fallback file for SPA routing
    pub fallback_file: Option<String>,
}

/// Configuration for custom handlers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomHandlerConfig {
    /// Callback name to invoke
    pub callback_name: String,
    /// Additional configuration data
    pub config_data: Option<serde_json::Value>,
}

/// Proxy target configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyTarget {
    /// Path pattern to match
    pub path_pattern: String,
    /// Target service URL or handler name
    pub target: String,
}

/// Global configuration for all handlers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalHandlerConfig {
    /// Enable request logging
    pub enable_logging: bool,
    /// Request timeout in milliseconds
    pub timeout_ms: Option<u64>,
    /// Global firewall configuration
    pub global_firewall: Option<String>,
}

impl Default for DeclarativeHandlerConfig {
    fn default() -> Self {
        Self {
            handlers: vec![
                // Default three-tier architecture
                HandlerDeclaration {
                    path_prefix: "/admin/sites/".to_string(),
                    handler_type: HandlerType::Admin(AdminHandlerConfig::automatic()),
                    name: Some("Admin Management".to_string()),
                    enabled: true,
                },
                HandlerDeclaration {
                    path_prefix: "/api/".to_string(),
                    handler_type: HandlerType::ApiProxy(ApiProxyConfig::declarative()),
                    name: Some("API Backend".to_string()),
                    enabled: true,
                },
                HandlerDeclaration {
                    path_prefix: "/".to_string(),
                    handler_type: HandlerType::Frontend(FrontendHandlerConfig::memory_first()),
                    name: Some("Frontend Assets".to_string()),
                    enabled: true,
                },
            ],
            global: GlobalHandlerConfig::default(),
        }
    }
}

impl AdminHandlerConfig {
    /// Create automatic admin configuration
    pub fn automatic() -> Self {
        Self {
            auto_admin: AutoAdminConfig {
                enable_status: true,
                enable_health: true,
                enable_info: true,
                enable_reload: true,
                admin_prefix: "/admin/sites".to_string(),
            },
            firewall_enabled: false,
        }
    }

    /// Create admin configuration with firewall
    pub fn with_firewall() -> Self {
        let mut config = Self::automatic();
        config.firewall_enabled = true;
        config
    }
}

impl ApiProxyConfig {
    /// Create declarative API proxy configuration
    pub fn declarative() -> Self {
        Self { auto_crud: true, proxy_targets: Vec::new() }
    }
}

impl FrontendHandlerConfig {
    /// Create memory-first frontend configuration
    pub fn memory_first() -> Self {
        Self { memory_first: true, dev_mode: false, fallback_file: Some("index.html".to_string()) }
    }

    /// Create development mode configuration
    pub fn dev_mode() -> Self {
        Self { memory_first: false, dev_mode: true, fallback_file: Some("index.html".to_string()) }
    }

    /// Create hybrid mode configuration
    pub fn hybrid_mode() -> Self {
        Self { memory_first: true, dev_mode: true, fallback_file: Some("index.html".to_string()) }
    }
}

impl Default for GlobalHandlerConfig {
    fn default() -> Self {
        Self {
            enable_logging: true,
            timeout_ms: Some(30000), // 30 seconds
            global_firewall: None,
        }
    }
}

/// Custom handler registry for storing and executing user-defined handlers
pub struct CustomHandlerRegistry<T> {
    handlers: HashMap<String, Box<dyn CustomHandlerCallback<T>>>,
}

/// Trait for custom handler callbacks
pub trait CustomHandlerCallback<T>: Send + Sync {
    fn call<'a>(
        &'a self,
        req: Req,
        server: &'a T,
        config: &'a CustomHandlerConfig,
    ) -> Pin<Box<dyn Future<Output = Result<Resp, Infallible>> + Send + 'a>>;
}

impl<T> Default for CustomHandlerRegistry<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> CustomHandlerRegistry<T> {
    pub fn new() -> Self {
        Self { handlers: HashMap::new() }
    }

    /// Register a custom handler with a callback
    pub fn register<F>(&mut self, name: String, callback: F)
    where
        F: CustomHandlerCallback<T> + 'static,
    {
        self.handlers.insert(name, Box::new(callback));
    }

    /// Get a custom handler callback by name
    pub fn get(&self, name: &str) -> Option<&dyn CustomHandlerCallback<T>> {
        self.handlers.get(name).map(|handler| handler.as_ref())
    }

    /// Execute a custom handler by name
    pub async fn execute(
        &self,
        name: &str,
        req: Req,
        server: &T,
        config: &CustomHandlerConfig,
    ) -> Result<Resp, Infallible> {
        if let Some(callback) = self.get(name) {
            log::debug!("Executing custom handler: {}", name);
            callback.call(req, server, config).await
        } else {
            log::warn!("Custom handler not found: {}", name);
            Ok(crate::http::not_found_response("custom handler"))
        }
    }
}

/// Declarative Handler System - Processes configuration and routes requests automatically
pub struct DeclarativeHandlerSystem {
    config: DeclarativeHandlerConfig,
}

impl DeclarativeHandlerSystem {
    /// Create new declarative handler system from configuration
    pub fn new(config: DeclarativeHandlerConfig) -> Self {
        Self { config }
    }

    /// Route request using declarative configuration with custom handlers support
    /// This is the magic - users never write routing code, just declare configuration!
    pub async fn route_request<T>(
        &self,
        req: Req,
        server: &T,
        firewall: Option<&Firewall>,
        custom_registry: Option<&CustomHandlerRegistry<T>>,
    ) -> Result<Resp, Infallible>
    where
        T: crate::http::ReloadableServer,
    {
        let path = req.uri().path();

        // Log request if enabled
        if self.config.global.enable_logging {
            log::debug!("Declarative routing: {} {}", req.method(), path);
        }

        // Find matching handler by path prefix (first match wins)
        for handler in &self.config.handlers {
            if !handler.enabled {
                continue;
            }

            if path.starts_with(&handler.path_prefix) {
                if let Some(name) = &handler.name {
                    log::debug!("Matched handler: {} for path: {}", name, path);
                }

                return self.execute_handler(req, server, handler, firewall, custom_registry).await;
            }
        }

        // No handler matched - return 404
        Ok(crate::http::not_found_response("route"))
    }

    /// Execute specific handler based on its type
    async fn execute_handler<T>(
        &self,
        req: Req,
        server: &T,
        handler: &HandlerDeclaration,
        firewall: Option<&Firewall>,
        custom_registry: Option<&CustomHandlerRegistry<T>>,
    ) -> Result<Resp, Infallible>
    where
        T: crate::http::ReloadableServer,
    {
        match &handler.handler_type {
            HandlerType::Admin(config) => {
                self.execute_admin_handler(req, server, config, firewall).await
            }
            HandlerType::ApiProxy(config) => {
                self.execute_api_proxy_handler(req, server, config).await
            }
            HandlerType::Frontend(config) => {
                self.execute_frontend_handler(req, server, config).await
            }
            HandlerType::Custom(config) => {
                self.execute_custom_handler(req, server, config, custom_registry).await
            }
        }
    }

    /// Execute admin handler
    async fn execute_admin_handler<T>(
        &self,
        req: Req,
        server: &T,
        config: &AdminHandlerConfig,
        firewall: Option<&Firewall>,
    ) -> Result<Resp, Infallible>
    where
        T: crate::http::ReloadableServer,
    {
        use crate::http::handle_auto_admin_endpoints_with_reload;

        let method = req.method();
        let path = req.uri().path();

        // Use the existing automatic admin system
        if let Some(auto_response) = handle_auto_admin_endpoints_with_reload(
            method,
            path,
            &req,
            server,
            &config.auto_admin,
            firewall,
        )
        .await
        {
            Ok(auto_response)
        } else {
            Ok(crate::http::not_found_response("admin endpoint"))
        }
    }

    /// Execute API proxy handler
    async fn execute_api_proxy_handler<T>(
        &self,
        _req: Req,
        _server: &T,
        _config: &ApiProxyConfig,
    ) -> Result<Resp, Infallible>
    where
        T: crate::http::ReloadableServer,
    {
        // For now, return not implemented
        // This would integrate with the existing DeclarativeHttpHandler system
        Ok(crate::http::not_found_response("API endpoint"))
    }

    /// Execute frontend handler
    async fn execute_frontend_handler<T>(
        &self,
        req: Req,
        server: &T,
        _config: &FrontendHandlerConfig,
    ) -> Result<Resp, Infallible>
    where
        T: crate::http::ReloadableServer,
    {
        // Use the existing frontend server system
        let frontend_state = server.get_frontend_state();
        let frontend_server = crate::frontend::FrontendServer::new(frontend_state);
        frontend_server.handle_request(req).await
    }

    /// Execute custom handler via callback registry
    async fn execute_custom_handler<T>(
        &self,
        req: Req,
        server: &T,
        config: &CustomHandlerConfig,
        custom_registry: Option<&CustomHandlerRegistry<T>>,
    ) -> Result<Resp, Infallible>
    where
        T: crate::http::ReloadableServer,
    {
        if let Some(registry) = custom_registry {
            registry.execute(&config.callback_name, req, server, config).await
        } else {
            log::warn!("No custom registry provided for handler: {}", config.callback_name);
            Ok(crate::http::not_found_response("custom handler"))
        }
    }
}
