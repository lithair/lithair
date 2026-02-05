//! URL-to-Function Handler System for Lithair
//!
//! MVC approach: Each URL maps directly to a function.
//! Zero boilerplate, pure declaration: handle("/api/products/getsum", my_function)

use crate::http::{Req, Resp};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;

/// Direct URL-to-Function Handler System
/// Pure declaration: handle("/api/endpoint", function_name)
pub struct UrlHandlerRegistry<T> {
    handlers: HashMap<String, Box<dyn UrlHandler<T>>>,
    exact_matches: HashMap<String, String>, // URL -> handler name
    prefix_matches: Vec<(String, String)>,  // (prefix, handler name) sorted by length
}

/// Trait for URL handlers - each function implements this
pub trait UrlHandler<T>: Send + Sync {
    fn handle<'a>(
        &'a self,
        req: Req,
        server: &'a T,
    ) -> Pin<Box<dyn Future<Output = Result<Resp, Infallible>> + Send + 'a>>;
}

impl<T> Default for UrlHandlerRegistry<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> UrlHandlerRegistry<T> {
    pub fn new() -> Self {
        Self { handlers: HashMap::new(), exact_matches: HashMap::new(), prefix_matches: Vec::new() }
    }

    /// Register exact URL match: handle("/api/products/getsum", handler)
    pub fn handle_exact<H>(&mut self, url: &str, handler: H)
    where
        H: UrlHandler<T> + 'static,
    {
        let handler_name = format!("exact_{}", self.handlers.len());
        self.handlers.insert(handler_name.clone(), Box::new(handler));
        self.exact_matches.insert(url.to_string(), handler_name);
        log::info!("🎯 Registered exact handler: {} → function", url);
    }

    /// Register prefix match: handle("/api/products/*", handler)
    pub fn handle_prefix<H>(&mut self, prefix: &str, handler: H)
    where
        H: UrlHandler<T> + 'static,
    {
        let handler_name = format!("prefix_{}", self.handlers.len());
        self.handlers.insert(handler_name.clone(), Box::new(handler));
        self.prefix_matches.push((prefix.to_string(), handler_name));

        // Sort by length (longest first for precise matching)
        self.prefix_matches.sort_by_key(|(prefix, _)| std::cmp::Reverse(prefix.len()));
        log::info!("🎯 Registered prefix handler: {} → function", prefix);
    }

    /// Route request to appropriate handler function
    pub async fn route_request(&self, req: Req, server: &T) -> Option<Result<Resp, Infallible>> {
        let path = req.uri().path();

        // 1. Check exact matches first
        if let Some(handler_name) = self.exact_matches.get(path) {
            if let Some(handler) = self.handlers.get(handler_name) {
                log::debug!("🎯 Exact match: {} → executing function", path);
                return Some(handler.handle(req, server).await);
            }
        }

        // 2. Check prefix matches (longest first)
        for (prefix, handler_name) in &self.prefix_matches {
            if path.starts_with(prefix) {
                if let Some(handler) = self.handlers.get(handler_name) {
                    log::debug!(
                        "🎯 Prefix match: {} matches {} → executing function",
                        path,
                        prefix
                    );
                    return Some(handler.handle(req, server).await);
                }
            }
        }

        None // No handler found
    }

    /// Get stats for debugging
    pub fn stats(&self) -> UrlHandlerStats {
        UrlHandlerStats {
            exact_handlers: self.exact_matches.len(),
            prefix_handlers: self.prefix_matches.len(),
            total_handlers: self.handlers.len(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UrlHandlerStats {
    pub exact_handlers: usize,
    pub prefix_handlers: usize,
    pub total_handlers: usize,
}

/// Macro pour simplifier l'enregistrement des handlers
/// Usage: register_handler!(registry, "/api/products/getsum", calculate_sum);
#[macro_export]
macro_rules! register_handler {
    ($registry:expr, $url:expr, $handler:expr) => {
        $registry.handle_exact($url, $handler);
    };
}

/// Macro pour les handlers avec préfixes
/// Usage: register_prefix_handler!(registry, "/api/products/", product_handler);
#[macro_export]
macro_rules! register_prefix_handler {
    ($registry:expr, $prefix:expr, $handler:expr) => {
        $registry.handle_prefix($prefix, $handler);
    };
}

/// Helper macro pour créer des handlers rapidement
/// Usage: simple_handler!(GetSumHandler, |req, server| async { ... });
#[macro_export]
macro_rules! simple_handler {
    ($name:ident, |$req:ident, $server:ident| $body:expr) => {
        pub struct $name;

        impl<T> $crate::http::UrlHandler<T> for $name {
            fn handle<'a>(
                &'a self,
                $req: $crate::http::Req,
                $server: &'a T,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = Result<$crate::http::Resp, std::convert::Infallible>,
                        > + Send
                        + 'a,
                >,
            > {
                Box::pin($body)
            }
        }
    };
}
