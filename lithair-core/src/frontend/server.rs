//! Asset Server for Lithair Frontend

use super::{FrontendEngine, FrontendState};
use anyhow::Result;
use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper::{Method, Request, Response, StatusCode};
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::RwLock;

pub type RespBody = BoxBody<Bytes, Infallible>;
pub type Req = Request<hyper::body::Incoming>;
pub type Resp = Response<RespBody>;

fn body_from<T: Into<Bytes>>(data: T) -> RespBody {
    Full::new(data.into()).boxed()
}

/// Asset Server for serving static assets from Lithair memory
pub enum AssetServer {
    /// Legacy RwLock-based server (deprecated, use Scc2 variant)
    Legacy { state: Arc<RwLock<FrontendState>>, fallback_path: Option<String> },
    /// SCC2-based lock-free server (40M+ ops/sec)
    Scc2 { engine: Arc<FrontendEngine>, fallback_path: Option<String> },
}

impl AssetServer {
    /// Create legacy RwLock-based asset server (deprecated)
    pub fn new(state: Arc<RwLock<FrontendState>>) -> Self {
        Self::Legacy { state, fallback_path: Some("/index.html".to_string()) }
    }

    /// Create SCC2-based lock-free asset server (recommended)
    pub fn new_scc2(engine: Arc<FrontendEngine>) -> Self {
        Self::Scc2 { engine, fallback_path: Some("/index.html".to_string()) }
    }

    /// Serve asset from memory with virtual host routing
    pub async fn serve_asset(&self, request_path: &str) -> Option<(Vec<u8>, String)> {
        match self {
            AssetServer::Scc2 { engine, fallback_path } => {
                // SCC2 lock-free path - 40M+ ops/sec
                let clean_path = Self::clean_path_static(request_path);

                // Try direct asset lookup
                if let Some(asset) = engine.get_asset(&clean_path).await {
                    log::info!(
                        "[{}] Serving {} from SCC2 memory ({} bytes)",
                        engine.host_id(),
                        clean_path,
                        asset.size_bytes
                    );
                    return Some((asset.content, asset.mime_type));
                }

                // Try fallback for root/empty path
                if (clean_path == "/" || clean_path.is_empty()) && fallback_path.is_some() {
                    if let Some(asset) = engine.get_asset("/index.html").await {
                        log::info!(
                            "[{}] Serving fallback /index.html from SCC2 memory",
                            engine.host_id()
                        );
                        return Some((asset.content, asset.mime_type));
                    }
                }

                None
            }
            AssetServer::Legacy { state, fallback_path } => {
                // Legacy RwLock path (deprecated)
                let clean_path = Self::clean_path_static(request_path);
                let state = state.read().await;

                // Sort virtual hosts by base_path length (longest first) for accurate matching
                let mut vhosts: Vec<_> = state.virtual_hosts.values().collect();
                vhosts.sort_by_key(|vh| std::cmp::Reverse(vh.base_path.len()));

                for vhost in vhosts {
                    if !vhost.active {
                        continue;
                    }

                    // Check if request path matches this virtual host's base path
                    if clean_path.starts_with(&vhost.base_path) || vhost.base_path == "/" {
                        // Strip base_path to get asset path within virtual host
                        let asset_path = if vhost.base_path == "/" {
                            clean_path.clone()
                        } else {
                            clean_path
                                .strip_prefix(&vhost.base_path)
                                .map(|p| if p.is_empty() { "/".to_string() } else { p.to_string() })
                                .unwrap_or_else(|| "/".to_string())
                        };

                        // Try to find asset in this virtual host
                        if let Some(asset_id) = vhost.path_index.get(&asset_path) {
                            if let Some(asset) = vhost.assets.get(asset_id) {
                                log::info!(
                                    "[{}] Serving {} from memory ({} bytes)",
                                    vhost.host_id,
                                    clean_path,
                                    asset.size_bytes
                                );
                                return Some((asset.content.clone(), asset.mime_type.clone()));
                            }
                        }

                        // Try fallback for root/empty path
                        if (asset_path == "/" || asset_path.is_empty()) && fallback_path.is_some() {
                            if let Some(fallback_id) = vhost.path_index.get("/index.html") {
                                if let Some(asset) = vhost.assets.get(fallback_id) {
                                    log::info!(
                                        "[{}] Serving fallback /index.html from memory",
                                        vhost.host_id
                                    );
                                    return Some((asset.content.clone(), asset.mime_type.clone()));
                                }
                            }
                        }
                    }
                }

                None
            }
        }
    }

    fn clean_path_static(path: &str) -> String {
        if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{}", path)
        }
    }
}

/// Frontend Server for serving assets via HTTP
pub struct FrontendServer {
    asset_server: AssetServer,
}

impl FrontendServer {
    /// Create frontend server with legacy RwLock (deprecated)
    pub fn new(state: Arc<RwLock<FrontendState>>) -> Self {
        Self { asset_server: AssetServer::new(state) }
    }

    /// Create frontend server with SCC2 lock-free engine (recommended)
    pub fn new_scc2(engine: Arc<FrontendEngine>) -> Self {
        Self { asset_server: AssetServer::new_scc2(engine) }
    }

    /// Handle HTTP request for assets
    ///
    /// Supports both `GET` and `HEAD`. For a HEAD request the response
    /// is identical to GET (same status, same headers including
    /// `Content-Type` and `Content-Length`) but the body is empty, as
    /// required by RFC 7231 §4.3.2. Before this changed (issue #56),
    /// HEAD requests fell through the `Method::GET` arm into the
    /// `_ => METHOD_NOT_ALLOWED` branch, which in turn caused the
    /// caller in `LithairServer::handle_request` to skip static
    /// dispatch entirely and emit the default `{"error":"Not found"}`
    /// 404 — confusing SEO crawlers and monitoring probes that issue
    /// HEAD by default (e.g. `curl -I`, Uptime Robot, Googlebot).
    pub async fn handle_request(&self, req: Req) -> Result<Resp, Infallible> {
        let method = req.method();
        let is_head = method == Method::HEAD;
        let mut path = req.uri().path().to_string();

        match method {
            &Method::GET | &Method::HEAD => {
                // Try exact path first
                let mut result = self.asset_server.serve_asset(&path).await;

                // If not found and path ends with /, try index.html
                if result.is_none() && path.ends_with('/') {
                    path.push_str("index.html");
                    result = self.asset_server.serve_asset(&path).await;
                }

                // If still not found and doesn't end with /, try with trailing slash + index.html
                // This handles cases like /documentation/introduction -> /documentation/introduction/index.html
                if result.is_none() && !path.ends_with('/') && !path.contains('.') {
                    let path_with_slash = format!("{}/index.html", path);
                    result = self.asset_server.serve_asset(&path_with_slash).await;
                }

                match result {
                    Some((content, mime_type)) => {
                        // For HEAD we still emit Content-Length matching
                        // what GET would return, but with an empty body.
                        let content_length = content.len();
                        let body_bytes: Bytes =
                            if is_head { Bytes::new() } else { Bytes::from(content) };
                        Ok(Response::builder()
                            .status(StatusCode::OK)
                            .header("Content-Type", mime_type)
                            .header("Content-Length", content_length)
                            .header("X-Served-From", "Lithair-Memory")
                            .body(body_from(body_bytes))
                            .unwrap())
                    }
                    None => Ok(self.not_found(is_head).await),
                }
            }
            _ => Ok(Response::builder()
                .status(StatusCode::METHOD_NOT_ALLOWED)
                .body(body_from("Method not allowed"))
                .unwrap()),
        }
    }

    /// Build the 404 response.
    ///
    /// A site that shipped its own `/404.html` gets that back, with its own
    /// MIME type and its own design — convention rather than configuration, so
    /// there is nothing to switch on and a site without one is unaffected.
    /// Otherwise the built-in page, which is a framework default and may look
    /// like one.
    async fn not_found(&self, is_head: bool) -> Resp {
        if let Some((content, mime_type)) = self.asset_server.serve_asset("/404.html").await {
            let content_length = content.len();
            let body_bytes: Bytes = if is_head { Bytes::new() } else { Bytes::from(content) };
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header("Content-Type", mime_type)
                .header("Content-Length", content_length)
                .header("X-Served-From", "Lithair-Memory")
                .body(body_from(body_bytes))
                .unwrap();
        }

        let html_404 = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>404 - Page Not Found</title>
    <style>
        @keyframes blink { 0%, 50% { opacity: 1; } 51%, 100% { opacity: 0; } }
        body { margin: 0; padding: 0; background: #0a0a0a; font-family: 'Courier New', monospace; display: flex; align-items: center; justify-content: center; min-height: 100vh; color: #00ff00; }
        .container { text-align: center; max-width: 600px; padding: 2rem; border: 2px solid #00ff00; border-radius: 8px; background: #1a1a1a; box-shadow: 0 0 30px rgba(0, 255, 0, 0.3); }
        h1 { font-size: 4rem; margin: 0; text-shadow: 0 0 10px #00ff00; }
        .code { font-size: 1.2rem; margin: 1rem 0; opacity: 0.8; }
        .message { margin: 2rem 0; line-height: 1.6; }
        .blink { animation: blink 1s infinite; }
        a { color: #00ff00; text-decoration: none; border: 1px solid #00ff00; padding: 0.5rem 1.5rem; display: inline-block; margin-top: 1rem; transition: all 0.3s; }
        a:hover { background: #00ff00; color: #0a0a0a; box-shadow: 0 0 20px rgba(0, 255, 0, 0.5); }
        .terminal { text-align: left; background: #0a0a0a; padding: 1rem; border-radius: 4px; margin: 1rem 0; border: 1px solid #00ff00; }
    </style>
</head>
<body>
    <div class="container">
        <h1>404</h1>
        <div class="code">$ cat /dev/null<span class="blink">_</span></div>
        <div class="message">
            <p><strong>Error: Page Not Found</strong></p>
            <div class="terminal">
                <div>> File not found in filesystem</div>
                <div>> Path does not exist</div>
                <div>> Returning to safety...</div>
            </div>
        </div>
        <a href="/">← Back to Home</a>
    </div>
</body>
</html>"#;
        let body_bytes: Bytes = if is_head { Bytes::new() } else { Bytes::from(html_404) };
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("Content-Type", "text/html; charset=utf-8")
            .header("Content-Length", html_404.len())
            .body(body_from(body_bytes))
            .unwrap()
    }

    /// Handle admin requests completely automatically using Lithair declarative approach
    /// This makes admin management 100% transparent - users never see implementation details
    pub async fn handle_admin_request<T>(
        &self,
        req: Req,
        server: &T,
        config: &crate::http::AutoAdminConfig,
        firewall: Option<&crate::http::firewall::Firewall>,
    ) -> Result<Resp, Infallible>
    where
        T: crate::http::ReloadableServer,
    {
        use crate::http::handle_auto_admin_endpoints_with_reload;

        let method = req.method();
        let path = req.uri().path();

        // All admin logic is now completely automatic and declarative
        if let Some(auto_response) =
            handle_auto_admin_endpoints_with_reload(method, path, &req, server, config, firewall)
                .await
        {
            Ok(auto_response)
        } else {
            // If no automatic endpoint matches, return not found
            Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header("Content-Type", "application/json")
                .body(body_from(
                    serde_json::json!({
                        "error": "not_found",
                        "message": "Admin endpoint not found"
                    })
                    .to_string(),
                ))
                .unwrap())
        }
    }

    /// Handle HTTP request using declarative handler system - Revolutionary Data-First approach
    /// This method eliminates ALL manual routing by using pure configuration-driven handlers
    pub async fn handle_request_declarative<T>(
        &self,
        req: Req,
        server: &T,
        config: &crate::http::DeclarativeHandlerConfig,
        firewall: Option<&crate::http::firewall::Firewall>,
        custom_registry: Option<&crate::http::CustomHandlerRegistry<T>>,
    ) -> Result<Resp, Infallible>
    where
        T: crate::http::ReloadableServer,
    {
        use crate::http::DeclarativeHandlerSystem;

        // Create handler system from configuration
        let handler_system = DeclarativeHandlerSystem::new(config.clone());

        // Route request using pure declarative configuration with custom handlers support
        handler_system.route_request(req, server, firewall, custom_registry).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::{StaticAsset, VirtualHostLocation};
    use std::collections::HashMap;

    fn asset(path: &str, content: &[u8]) -> StaticAsset {
        StaticAsset::new(path.to_string(), content.to_vec())
    }

    fn legacy_state_with_hosts() -> Arc<RwLock<FrontendState>> {
        let mut state = FrontendState::default();

        let index_asset = asset("/index.html", b"<h1>home</h1>");
        let docs_asset = asset("/guide/index.html", b"guide");

        let mut root_host = VirtualHostLocation {
            host_id: "main".to_string(),
            base_path: "/".to_string(),
            assets: HashMap::new(),
            path_index: HashMap::new(),
            static_root: String::new(),
            active: true,
        };
        root_host.path_index.insert("/index.html".to_string(), index_asset.id);
        root_host.assets.insert(index_asset.id, index_asset);

        let mut docs_host = VirtualHostLocation {
            host_id: "docs".to_string(),
            base_path: "/docs".to_string(),
            assets: HashMap::new(),
            path_index: HashMap::new(),
            static_root: String::new(),
            active: true,
        };
        docs_host.path_index.insert("/guide/index.html".to_string(), docs_asset.id);
        docs_host.assets.insert(docs_asset.id, docs_asset);

        state.virtual_hosts.insert("main".to_string(), root_host);
        state.virtual_hosts.insert("docs".to_string(), docs_host);

        Arc::new(RwLock::new(state))
    }

    #[tokio::test]
    async fn legacy_asset_server_serves_root_fallback() {
        let server = AssetServer::new(legacy_state_with_hosts());

        let served = server.serve_asset("/").await;

        let (content, mime) = served.expect("root fallback should resolve /index.html");
        assert_eq!(mime, "text/html");
        assert_eq!(content, b"<h1>home</h1>");
    }

    #[tokio::test]
    async fn legacy_asset_server_resolves_virtual_host_base_path() {
        let server = AssetServer::new(legacy_state_with_hosts());

        let served = server.serve_asset("/docs/guide/index.html").await;

        let (content, mime) = served.expect("docs virtual host should resolve nested asset");
        assert_eq!(mime, "text/html");
        assert_eq!(content, b"guide");
    }

    #[tokio::test]
    async fn legacy_asset_server_returns_none_for_missing_asset() {
        let server = AssetServer::new(legacy_state_with_hosts());

        let served = server.serve_asset("/missing.txt").await;

        assert!(served.is_none());
    }

    fn legacy_state_with(assets: &[(&str, &[u8])]) -> Arc<RwLock<FrontendState>> {
        let mut state = FrontendState::default();
        let mut host = VirtualHostLocation {
            host_id: "main".to_string(),
            base_path: "/".to_string(),
            assets: HashMap::new(),
            path_index: HashMap::new(),
            static_root: String::new(),
            active: true,
        };
        for (path, content) in assets {
            let a = asset(path, content);
            host.path_index.insert(path.to_string(), a.id);
            host.assets.insert(a.id, a);
        }
        state.virtual_hosts.insert("main".to_string(), host);
        Arc::new(RwLock::new(state))
    }

    async fn body_string(resp: Resp) -> String {
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8_lossy(&bytes).to_string()
    }

    #[tokio::test]
    async fn not_found_prefers_the_site_own_404_asset() {
        let server =
            FrontendServer::new(legacy_state_with(&[("/404.html", b"<h1>nothing here</h1>")]));

        let resp = server.not_found(false).await;

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(resp.headers()["Content-Type"], "text/html");
        assert_eq!(body_string(resp).await, "<h1>nothing here</h1>");
    }

    #[tokio::test]
    async fn not_found_respects_the_stored_mime_override() {
        // A `/404.html` whose MIME was set explicitly (set_mime_type, #197)
        // must be served with that exact Content-Type, not re-detected.
        let state = legacy_state_with(&[("/404.html", b"<h1>gone</h1>")]);
        {
            let mut s = state.write().await;
            let host = s.virtual_hosts.get_mut("main").unwrap();
            let id = host.path_index["/404.html"];
            host.assets.get_mut(&id).unwrap().set_mime_type("text/html; charset=utf-8");
        }
        let server = FrontendServer::new(state);

        let resp = server.not_found(false).await;

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(resp.headers()["Content-Type"], "text/html; charset=utf-8");
    }

    #[tokio::test]
    async fn not_found_falls_back_to_the_built_in_page() {
        let server = FrontendServer::new(legacy_state_with(&[("/index.html", b"home")]));

        let resp = server.not_found(false).await;

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert!(body_string(resp).await.contains("404 - Page Not Found"));
    }

    #[tokio::test]
    async fn not_found_head_keeps_the_headers_and_drops_the_body() {
        let server = FrontendServer::new(legacy_state_with(&[("/404.html", b"<h1>gone</h1>")]));

        let resp = server.not_found(true).await;

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(resp.headers()["Content-Length"], "13");
        assert_eq!(body_string(resp).await, "");
    }
}
