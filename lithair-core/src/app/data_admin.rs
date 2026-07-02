//! Data-admin API plane (`/_admin/data/*`): model listing/inspection,
//! logical backup + import (issue #37), per-entity history/edit, and the
//! embedded admin-UI page handler.

use super::*;
use anyhow::Result;
use std::sync::Arc;

impl LithairServer {
    /// Handle data admin API requests (/_admin/data/*)
    ///
    /// Endpoints:
    /// - GET /_admin/data/models - List all registered models with stats
    /// - GET /_admin/data/models/{name} - Get model info and data
    /// - GET /_admin/data/models/{name}/export - Export model data as JSON
    /// - GET /_admin/data/routes - List all registered API routes
    /// - POST /_admin/data/backup - Trigger full logical data backup (current state, JSON)
    /// - POST /_admin/data/import - Re-apply a logical backup as events (issue #37)
    pub(super) async fn handle_data_admin_request(
        &self,
        _req: hyper::Request<hyper::body::Incoming>,
        path: &str,
        method: &hyper::Method,
    ) -> Result<RouteResponse> {
        use bytes::Bytes;

        // Parse the path: /_admin/data/{resource}[/{name}][/{action}]
        let path_parts: Vec<&str> = path
            .strip_prefix("/_admin/data/")
            .unwrap_or("")
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        match (method, path_parts.as_slice()) {
            // GET /_admin/data/models - List all models
            (&hyper::Method::GET, ["models"]) => {
                let models = self.models.read().await;
                let mut model_list = Vec::new();

                for model in models.iter() {
                    let count = model.handler.get_count().await;
                    model_list.push(serde_json::json!({
                        "name": model.name,
                        "base_path": model.base_path,
                        "data_path": model.data_path,
                        "count": count
                    }));
                }

                let response = serde_json::json!({
                    "models": model_list,
                    "total_models": models.len()
                });

                Ok(hyper::Response::builder()
                    .status(200)
                    .header("Content-Type", "application/json")
                    .body(boxed_full(Bytes::from(
                        serde_json::to_string_pretty(&response).expect("serializable response"),
                    )))
                    .expect("valid HTTP response"))
            }

            // GET /_admin/data/models/{name} - Get model data
            (&hyper::Method::GET, ["models", name]) => {
                let models = self.models.read().await;

                if let Some(model) = models.iter().find(|m| m.name == *name) {
                    let data = model.handler.get_all_data_json().await;
                    let count = model.handler.get_count().await;

                    let response = serde_json::json!({
                        "model": model.name,
                        "base_path": model.base_path,
                        "count": count,
                        "data": data
                    });

                    Ok(hyper::Response::builder()
                        .status(200)
                        .header("Content-Type", "application/json")
                        .body(boxed_full(Bytes::from(
                            serde_json::to_string_pretty(&response).expect("serializable response"),
                        )))
                        .expect("valid HTTP response"))
                } else {
                    Ok(hyper::Response::builder()
                        .status(404)
                        .header("Content-Type", "application/json")
                        .body(boxed_full(Bytes::from(format!(
                            r#"{{"error":"Model '{}' not found"}}"#,
                            name
                        ))))
                        .expect("valid HTTP response"))
                }
            }

            // GET /_admin/data/models/{name}/_stats - Per-model storage stats (issue #72)
            (&hyper::Method::GET, ["models", name, "_stats"]) => {
                // Resolve the model under the read lock, snapshot the handler
                // + data_path, then drop the lock before awaiting get_stats.
                // Same rationale as handle_metrics_request: stats sampling
                // must not block writers (Gemini PR #83 review).
                let resolved: Option<(String, Arc<dyn crate::app::ModelHandler>)> = {
                    let models = self.models.read().await;
                    models
                        .iter()
                        .find(|m| m.name == *name)
                        .map(|m| (m.data_path.clone(), Arc::clone(&m.handler)))
                };

                if let Some((data_path, handler)) = resolved {
                    let stats = handler.get_stats(&data_path).await;

                    Ok(hyper::Response::builder()
                        .status(200)
                        .header("Content-Type", "application/json")
                        .body(boxed_full(Bytes::from(
                            serde_json::to_string_pretty(&stats).expect("serializable response"),
                        )))
                        .expect("valid HTTP response"))
                } else {
                    // Build the JSON body via serde_json so `name` is properly
                    // escaped — a naked `format!` would let a model name like
                    // `x", "y":"z` break out of the error string and produce
                    // malformed (or worse, attacker-shaped) JSON. See Gemini
                    // review on PR #83.
                    Ok(hyper::Response::builder()
                        .status(404)
                        .header("Content-Type", "application/json")
                        .body(boxed_full(Bytes::from(
                            serde_json::to_string(&serde_json::json!({
                                "error": format!("Model '{}' not found", name)
                            }))
                            .expect("error response is serializable"),
                        )))
                        .expect("valid HTTP response"))
                }
            }

            // GET /_admin/data/models/{name}/export - Export model data
            (&hyper::Method::GET, ["models", name, "export"]) => {
                let models = self.models.read().await;

                if let Some(model) = models.iter().find(|m| m.name == *name) {
                    let export = model.handler.export_json().await;

                    Ok(hyper::Response::builder()
                        .status(200)
                        .header("Content-Type", "application/json")
                        .header(
                            "Content-Disposition",
                            format!("attachment; filename=\"{}_export.json\"", name),
                        )
                        .body(boxed_full(Bytes::from(
                            serde_json::to_string_pretty(&export).expect("serializable response"),
                        )))
                        .expect("valid HTTP response"))
                } else {
                    Ok(hyper::Response::builder()
                        .status(404)
                        .header("Content-Type", "application/json")
                        .body(boxed_full(Bytes::from(format!(
                            r#"{{"error":"Model '{}' not found"}}"#,
                            name
                        ))))
                        .expect("valid HTTP response"))
                }
            }

            // GET /_admin/data/models/{name}/{id}/history - Get entity event history
            (&hyper::Method::GET, ["models", name, id, "history"]) => {
                let models = self.models.read().await;

                if let Some(model) = models.iter().find(|m| m.name == *name) {
                    let history = model.handler.get_entity_history(id).await;

                    Ok(hyper::Response::builder()
                        .status(200)
                        .header("Content-Type", "application/json")
                        .body(boxed_full(Bytes::from(
                            serde_json::to_string_pretty(&history).expect("serializable response"),
                        )))
                        .expect("valid HTTP response"))
                } else {
                    Ok(hyper::Response::builder()
                        .status(404)
                        .header("Content-Type", "application/json")
                        .body(boxed_full(Bytes::from(format!(
                            r#"{{"error":"Model '{}' not found"}}"#,
                            name
                        ))))
                        .expect("valid HTTP response"))
                }
            }

            // POST /_admin/data/models/{name}/{id}/edit - Submit edit event (event-sourced)
            (&hyper::Method::POST, ["models", name, id, "edit"]) => {
                use http_body_util::BodyExt;

                let models = self.models.read().await;

                if let Some(model) = models.iter().find(|m| m.name == *name) {
                    // Parse request body
                    let body_bytes = match _req.into_body().collect().await.map(|c| c.to_bytes()) {
                        Ok(bytes) => bytes,
                        Err(_) => {
                            return Ok(hyper::Response::builder()
                                .status(400)
                                .header("Content-Type", "application/json")
                                .body(boxed_full(Bytes::from(
                                    r#"{"error":"Invalid request body"}"#,
                                )))
                                .expect("valid HTTP response"));
                        }
                    };

                    let changes: serde_json::Value = match serde_json::from_slice(&body_bytes) {
                        Ok(v) => v,
                        Err(_) => {
                            return Ok(hyper::Response::builder()
                                .status(400)
                                .header("Content-Type", "application/json")
                                .body(boxed_full(Bytes::from(r#"{"error":"Invalid JSON"}"#)))
                                .expect("valid HTTP response"));
                        }
                    };

                    match model.handler.submit_edit_event(id, changes).await {
                        Ok(updated) => {
                            let response = serde_json::json!({
                                "success": true,
                                "message": "Edit event submitted successfully",
                                "entity_id": id,
                                "model": name,
                                "updated_data": updated
                            });

                            Ok(hyper::Response::builder()
                                .status(200)
                                .header("Content-Type", "application/json")
                                .body(boxed_full(Bytes::from(
                                    serde_json::to_string_pretty(&response)
                                        .expect("serializable response"),
                                )))
                                .expect("valid HTTP response"))
                        }
                        Err(e) => Ok(hyper::Response::builder()
                            .status(400)
                            .header("Content-Type", "application/json")
                            .body(boxed_full(Bytes::from(
                                serde_json::json!({"error": e.to_string()}).to_string(),
                            )))
                            .expect("valid HTTP response")),
                    }
                } else {
                    Ok(hyper::Response::builder()
                        .status(404)
                        .header("Content-Type", "application/json")
                        .body(boxed_full(Bytes::from(format!(
                            r#"{{"error":"Model '{}' not found"}}"#,
                            name
                        ))))
                        .expect("valid HTTP response"))
                }
            }

            // GET /_admin/data/routes - List all routes
            (&hyper::Method::GET, ["routes"]) => {
                let models = self.models.read().await;
                let mut routes = Vec::new();

                // Model routes
                for model in models.iter() {
                    routes.push(serde_json::json!({
                        "method": "GET",
                        "path": model.base_path.clone(),
                        "type": "model",
                        "model": model.name
                    }));
                    routes.push(serde_json::json!({
                        "method": "POST",
                        "path": model.base_path.clone(),
                        "type": "model",
                        "model": model.name
                    }));
                    routes.push(serde_json::json!({
                        "method": "GET",
                        "path": format!("{}/:id", model.base_path),
                        "type": "model",
                        "model": model.name
                    }));
                    routes.push(serde_json::json!({
                        "method": "PUT",
                        "path": format!("{}/:id", model.base_path),
                        "type": "model",
                        "model": model.name
                    }));
                    routes.push(serde_json::json!({
                        "method": "DELETE",
                        "path": format!("{}/:id", model.base_path),
                        "type": "model",
                        "model": model.name
                    }));
                }
                drop(models);

                // Custom routes
                for route in &self.custom_routes {
                    routes.push(serde_json::json!({
                        "method": route.method.to_string(),
                        "path": route.path,
                        "type": "custom"
                    }));
                }

                // Admin routes
                if self.config.admin.data_admin_enabled {
                    routes.push(serde_json::json!({
                        "method": "GET",
                        "path": "/_admin/data/models",
                        "type": "admin"
                    }));
                    routes.push(serde_json::json!({
                        "method": "GET",
                        "path": "/_admin/data/models/:name",
                        "type": "admin"
                    }));
                    routes.push(serde_json::json!({
                        "method": "GET",
                        "path": "/_admin/data/models/:name/export",
                        "type": "admin"
                    }));
                    routes.push(serde_json::json!({
                        "method": "GET",
                        "path": "/_admin/data/routes",
                        "type": "admin"
                    }));
                    routes.push(serde_json::json!({
                        "method": "POST",
                        "path": "/_admin/data/backup",
                        "type": "admin"
                    }));
                    routes.push(serde_json::json!({
                        "method": "POST",
                        "path": "/_admin/data/import",
                        "type": "admin"
                    }));
                }

                let response = serde_json::json!({
                    "routes": routes,
                    "total_routes": routes.len()
                });

                Ok(hyper::Response::builder()
                    .status(200)
                    .header("Content-Type", "application/json")
                    .body(boxed_full(Bytes::from(
                        serde_json::to_string_pretty(&response).expect("serializable response"),
                    )))
                    .expect("valid HTTP response"))
            }

            // POST /_admin/data/backup - Backup all models
            (&hyper::Method::POST, ["backup"]) => {
                let models = self.models.read().await;
                let mut backup_data = Vec::new();

                for model in models.iter() {
                    let export = model.handler.export_json().await;
                    backup_data.push(export);
                }

                let backup = serde_json::json!({
                    "backup_type": "full",
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                    "model_count": models.len(),
                    "models": backup_data
                });

                Ok(hyper::Response::builder()
                    .status(200)
                    .header("Content-Type", "application/json")
                    .header("Content-Disposition", "attachment; filename=\"lithair_backup.json\"")
                    .body(boxed_full(Bytes::from(
                        serde_json::to_string_pretty(&backup).expect("serializable response"),
                    )))
                    .expect("valid HTTP response"))
            }

            // POST /_admin/data/import - Logical import: re-apply a backup's
            // items as events (the symmetric counterpart of POST .../backup,
            // issue #37). This is for content migration / seeding, NOT
            // disaster recovery: it re-applies the *current state* carried in
            // the backup as new events. It is idempotent by `id` (re-importing
            // overwrites the entity, never duplicates it) but appends an event
            // per item each run (the log grows); it does NOT restore event
            // history. The DR path remains a physical event-store copy + replay
            // (docs/operations/backup-restore.md). As a write it is correctly
            // subject to leader redirection on a follower (not exempted above).
            (&hyper::Method::POST, ["import"]) => {
                use http_body_util::BodyExt;

                let body_bytes = match _req.into_body().collect().await.map(|c| c.to_bytes()) {
                    Ok(bytes) => bytes,
                    Err(_) => {
                        return Ok(hyper::Response::builder()
                            .status(400)
                            .header("Content-Type", "application/json")
                            .body(boxed_full(Bytes::from(r#"{"error":"Invalid request body"}"#)))
                            .expect("valid HTTP response"));
                    }
                };

                let parsed: serde_json::Value = match serde_json::from_slice(&body_bytes) {
                    Ok(v) => v,
                    Err(e) => {
                        return Ok(hyper::Response::builder()
                            .status(400)
                            .header("Content-Type", "application/json")
                            .body(boxed_full(Bytes::from(
                                serde_json::json!({"error": format!("Invalid JSON: {}", e)})
                                    .to_string(),
                            )))
                            .expect("valid HTTP response"));
                    }
                };

                // Accept the exact shape produced by POST /_admin/data/backup
                // (`{ "models": [ {model, data}, ... ] }`), a bare array of
                // per-model exports, or a single `{model, data}` object — so an
                // operator can re-feed a whole backup or just one model's slice.
                let model_exports: Vec<serde_json::Value> = if let Some(models) =
                    parsed.get("models").and_then(|m| m.as_array())
                {
                    models.clone()
                } else if let Some(arr) = parsed.as_array() {
                    arr.clone()
                } else if parsed.get("model").is_some() {
                    vec![parsed.clone()]
                } else {
                    return Ok(hyper::Response::builder()
                            .status(400)
                            .header("Content-Type", "application/json")
                            .body(boxed_full(Bytes::from(
                                r#"{"error":"Expected a backup object with a 'models' array, a bare array of model exports, or a single {model,data} object"}"#,
                            )))
                            .expect("valid HTTP response"));
                };

                let models = self.models.read().await;
                let mut results = Vec::new();
                let mut total_imported = 0usize;
                let mut any_error = false;

                for export in &model_exports {
                    let model_name = export.get("model").and_then(|m| m.as_str()).unwrap_or("");

                    if model_name.is_empty() {
                        any_error = true;
                        results.push(serde_json::json!({
                            "model": model_name,
                            "status": "error",
                            "error": "missing 'model' name in export entry"
                        }));
                        continue;
                    }

                    // `data` must be an array; an empty array is fine (0 records).
                    // A missing or non-array `data` is a malformed entry — report
                    // it as an error in the 207 set rather than silently coercing
                    // it to empty and reporting a 0-record success.
                    let data: Vec<serde_json::Value> = match export.get("data") {
                        Some(serde_json::Value::Array(items)) => items.clone(),
                        _ => {
                            any_error = true;
                            results.push(serde_json::json!({
                                "model": model_name,
                                "status": "error",
                                "error": "missing or non-array 'data' field"
                            }));
                            continue;
                        }
                    };

                    match models.iter().find(|m| m.name == model_name) {
                        Some(model) => {
                            let requested = data.len();
                            match model.handler.apply_replicated_items_json(data).await {
                                Ok(applied) => {
                                    total_imported += applied;
                                    results.push(serde_json::json!({
                                        "model": model_name,
                                        "status": "imported",
                                        "requested": requested,
                                        "imported": applied
                                    }));
                                }
                                Err(e) => {
                                    any_error = true;
                                    results.push(serde_json::json!({
                                        "model": model_name,
                                        "status": "error",
                                        "requested": requested,
                                        "error": e
                                    }));
                                }
                            }
                        }
                        None => {
                            any_error = true;
                            results.push(serde_json::json!({
                                "model": model_name,
                                "status": "unknown_model",
                                "error": format!("no registered model named '{}'", model_name)
                            }));
                        }
                    }
                }

                // 200 when every model imported cleanly; 207 Multi-Status when
                // any entry failed (unknown model, malformed data, deserialize
                // error). 207 is a 2xx, so `curl --fail` does NOT flag it —
                // automation must test the status code (`!= 200`) or inspect the
                // per-model `status` in the body to detect partial success.
                let status = if any_error { 207 } else { 200 };
                let response = serde_json::json!({
                    "status": if any_error { "partial" } else { "imported" },
                    "total_imported": total_imported,
                    "models": results,
                    "note": "logical import: re-applies items as events, idempotent by id, does not restore event history"
                });

                Ok(hyper::Response::builder()
                    .status(status)
                    .header("Content-Type", "application/json")
                    .body(boxed_full(Bytes::from(
                        serde_json::to_string_pretty(&response).expect("serializable response"),
                    )))
                    .expect("valid HTTP response"))
            }

            // 404 for unknown data admin paths
            _ => Ok(hyper::Response::builder()
                .status(404)
                .header("Content-Type", "application/json")
                .body(boxed_full(Bytes::from(r#"{"error":"Unknown data admin endpoint"}"#)))
                .expect("valid HTTP response")),
        }
    }

    /// Handle embedded data admin UI request (serves the dashboard HTML)
    /// Only available when the `admin-ui` feature is enabled
    #[cfg(feature = "admin-ui")]
    pub(super) async fn handle_data_admin_ui_request(&self) -> Result<RouteResponse> {
        use bytes::Bytes;

        Ok(hyper::Response::builder()
            .status(200)
            .header("Content-Type", "text/html; charset=utf-8")
            .header("Cache-Control", "no-cache")
            .body(boxed_full(Bytes::from(crate::admin_ui::DASHBOARD_HTML)))
            .expect("valid HTTP response"))
    }
}
