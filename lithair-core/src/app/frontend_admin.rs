//! Frontend lifecycle admin plane (`/_admin/frontend/*`, issue #134):
//! inventory of both frontend storage paths, per-frontend detail
//! (config + sha256 version + manifest), and node-local hot reload.

use super::*;
use anyhow::Result;
use std::sync::Arc;

impl LithairServer {
    /// Flatten both frontend storage paths into a single addressable
    /// inventory.
    ///
    /// Lithair stores frontends in two places (issue #134):
    ///   - `frontend_engines`: host-agnostic, path-prefix frontends declared
    ///     via `with_frontend` / `with_frontend_at`.
    ///   - `vhost_frontend_router`: per-`Host:` frontends declared via
    ///     `with_vhost` / `with_default_vhost`.
    ///
    /// Each entry gets a stable, URL-addressable `key`:
    ///   - path-prefix:  `path:<route_prefix>`     e.g. `path:/`
    ///   - vhost:        `vhost:<host>:<prefix>`    e.g. `vhost:blog.example.com:/`
    ///   - default vhost: `vhost:<default>:<prefix>`
    ///
    /// The returned tuples are `(key, kind, host_opt, route_prefix, engine)`.
    pub(super) fn frontend_inventory(
        &self,
    ) -> Vec<(
        String,
        &'static str,
        Option<String>,
        String,
        Arc<crate::frontend::FrontendEngine>,
    )> {
        let mut out = Vec::new();

        // Path-prefix frontends (host-agnostic).
        for (prefix, engine) in &self.frontend_engines {
            out.push((format!("path:{}", prefix), "path", None, prefix.clone(), engine.clone()));
        }

        // Per-vhost frontends (host-header routed).
        for (host, engines) in self.vhost_frontend_router.iter() {
            for (prefix, engine) in engines {
                out.push((
                    format!("vhost:{}:{}", host, prefix),
                    "vhost",
                    Some(host.to_string()),
                    prefix.clone(),
                    engine.clone(),
                ));
            }
        }
        if let Some(engines) = self.vhost_frontend_router.default_value() {
            for (prefix, engine) in engines {
                out.push((
                    format!("vhost:<default>:{}", prefix),
                    "vhost",
                    Some("<default>".to_string()),
                    prefix.clone(),
                    engine.clone(),
                ));
            }
        }

        out
    }

    /// Build the JSON detail object for a single frontend inventory entry.
    pub(super) fn frontend_detail_json(
        key: &str,
        kind: &str,
        host: &Option<String>,
        route_prefix: &str,
        engine: &crate::frontend::FrontendEngine,
        include_paths: bool,
    ) -> serde_json::Value {
        // Use the cached metadata (O(1)) — do NOT list+hash assets here, that
        // reintroduced O(total asset bytes) work on every GET (PR #138 review).
        // list_assets() is only needed below, and only when paths are requested.
        let mut detail = serde_json::json!({
            "key": key,
            "kind": kind,
            "host": host,
            "base_path": route_prefix,
            "source_dir": engine.source_dir(),
            "asset_count": engine.asset_count(),
            "total_bytes": engine.total_bytes(),
            "version": engine.version(),
            "last_reload_at": engine.last_reload_at().map(|t| t.to_rfc3339()),
        });
        if include_paths {
            // Cap the inline path list so a large frontend's detail response
            // stays bounded; callers needing the full manifest can page via
            // the asset store directly. This is the only path that lists assets.
            const MAX_INLINE_PATHS: usize = 500;
            let assets = engine.list_assets();
            let mut paths: Vec<String> = assets.iter().map(|a| a.path.clone()).collect();
            paths.sort();
            if paths.len() <= MAX_INLINE_PATHS {
                detail["paths"] = serde_json::json!(paths);
            } else {
                detail["paths_truncated"] = serde_json::json!(true);
                detail["paths"] = serde_json::json!(&paths[..MAX_INLINE_PATHS]);
            }
        }
        detail
    }

    /// Handle frontend lifecycle admin API requests (`/_admin/frontend/*`).
    ///
    /// Endpoints (issue #134):
    /// - `GET  /_admin/frontend`              — list every configured frontend
    /// - `GET  /_admin/frontend/{key}`        — one frontend's full config + version + manifest
    /// - `POST /_admin/frontend/{key}/reload` — atomically reload one frontend
    /// - `POST /_admin/frontend/reload`       — reload every frontend
    ///
    /// `{key}` is the opaque, percent-encoded inventory key returned by the
    /// list endpoint (e.g. `path:/`, `vhost:blog.example.com:/`).
    pub(super) async fn handle_frontend_admin_request(
        &self,
        path: &str,
        method: &hyper::Method,
    ) -> Result<RouteResponse> {
        use bytes::Bytes;

        let json_resp = |status: u16, value: serde_json::Value| -> Result<RouteResponse> {
            Ok(hyper::Response::builder()
                .status(status)
                .header("Content-Type", "application/json")
                .body(boxed_full(Bytes::from(
                    serde_json::to_string_pretty(&value).expect("serializable response"),
                )))
                .expect("valid HTTP response"))
        };

        // GET /_admin/frontend — list all frontends across both paths.
        if path == "/_admin/frontend" {
            if method != hyper::Method::GET {
                return json_resp(
                    405,
                    serde_json::json!({"error": "method_not_allowed", "message": "use GET to list frontends"}),
                );
            }
            let inventory = self.frontend_inventory();
            let mut list = Vec::with_capacity(inventory.len());
            for (key, kind, host, prefix, engine) in &inventory {
                list.push(Self::frontend_detail_json(key, kind, host, prefix, engine, false));
            }
            return json_resp(
                200,
                serde_json::json!({ "frontends": list, "total": inventory.len() }),
            );
        }

        // POST /_admin/frontend/reload — reload every frontend.
        if path == "/_admin/frontend/reload" {
            if method != hyper::Method::POST {
                return json_resp(
                    405,
                    serde_json::json!({"error": "method_not_allowed", "message": "use POST to reload"}),
                );
            }
            let inventory = self.frontend_inventory();
            // Reload every frontend concurrently (PR #138 review): each
            // engine's reload is independent (`reload(&self)`, distinct dirs),
            // so there's no reason to serialize the per-engine disk reads.
            let reload_futs =
                inventory.iter().map(|(key, kind, host, prefix, engine)| async move {
                    match engine.reload().await {
                        Ok(outcome) => serde_json::json!({
                            "key": key,
                            "kind": kind,
                            "host": host,
                            "base_path": prefix,
                            "status": "reloaded",
                            "asset_count": outcome.asset_count,
                            "total_bytes": outcome.total_bytes,
                            "version": outcome.version,
                            "changed": outcome.changed,
                        }),
                        Err(e) => serde_json::json!({
                            "key": key,
                            "kind": kind,
                            "host": host,
                            "base_path": prefix,
                            "status": "error",
                            "error": e.to_string(),
                        }),
                    }
                });
            let results: Vec<serde_json::Value> = futures::future::join_all(reload_futs).await;
            return json_resp(200, serde_json::json!({ "results": results }));
        }

        // /_admin/frontend/{key}            (GET, inspect)
        // /_admin/frontend/{key}/reload     (POST, reload one)
        let rest = path.strip_prefix("/_admin/frontend/").unwrap_or("");
        let (encoded_key, is_reload) = match rest.strip_suffix("/reload") {
            Some(k) => (k, true),
            None => (rest, false),
        };

        // Keys are percent-encoded by clients (they contain ':' and '/').
        let key = crate::http::query::percent_decode(encoded_key);
        if key.is_empty() {
            return json_resp(
                404,
                serde_json::json!({"error": "not_found", "message": "missing frontend key"}),
            );
        }

        let inventory = self.frontend_inventory();
        let entry = inventory.iter().find(|(k, ..)| *k == key);
        let Some((k, kind, host, prefix, engine)) = entry else {
            return json_resp(
                404,
                serde_json::json!({"error": "not_found", "message": format!("no frontend with key '{}'", key)}),
            );
        };

        if is_reload {
            if method != hyper::Method::POST {
                return json_resp(
                    405,
                    serde_json::json!({"error": "method_not_allowed", "message": "use POST to reload"}),
                );
            }
            return match engine.reload().await {
                Ok(outcome) => json_resp(
                    200,
                    serde_json::json!({
                        "key": k,
                        "kind": kind,
                        "host": host,
                        "base_path": prefix,
                        "status": "reloaded",
                        "asset_count": outcome.asset_count,
                        "total_bytes": outcome.total_bytes,
                        "version": outcome.version,
                        "changed": outcome.changed,
                    }),
                ),
                Err(e) => json_resp(
                    500,
                    serde_json::json!({
                        "key": k,
                        "status": "error",
                        "error": e.to_string(),
                    }),
                ),
            };
        }

        // GET /_admin/frontend/{key} — inspect one frontend.
        if method != hyper::Method::GET {
            return json_resp(
                405,
                serde_json::json!({"error": "method_not_allowed", "message": "use GET to inspect"}),
            );
        }
        json_resp(200, Self::frontend_detail_json(k, kind, host, prefix, engine, true))
    }
}
