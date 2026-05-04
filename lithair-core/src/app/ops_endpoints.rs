//! Built-in operations endpoints for `LithairServer`.
//!
//! This module defines the default `/health`, `/ready`, and `/info`
//! handlers that ship with every `LithairServer`. They are dispatched
//! from `LithairServer::handle_request` *after* user-registered custom
//! routes, so a user calling
//! `.with_route(GET, "/health", custom_handler)` always overrides the
//! default.
//!
//! # Why a separate module
//!
//! These endpoints are shared in spirit with the older
//! `DeclarativeServer` (see `crate::http::declarative_server`), but the
//! two builders have different surfaces:
//!
//! * `DeclarativeServer` is parameterised over a single model type `T`
//!   and reports model-specific fields (`"model"`, `"api"`).
//! * `LithairServer` hosts multiple models (or none at all), so its
//!   responses describe the *server*, not a single model.
//!
//! We deliberately keep the response shapes minimal and stable. They
//! are part of the public contract the README advertises ("Every
//! Lithair server comes with `/health`, `/ready`, and `/info`").
//!
//! # Response shapes
//!
//! * `GET /health` → `200 {"status":"healthy"}` — matches
//!   `DeclarativeServer` byte-for-byte.
//! * `GET /ready`  → `200 {"status":"ready","version":"<crate version>"}`.
//!   The `version` field mirrors `DeclarativeServer`'s
//!   `include_version` opt-in; we surface it unconditionally here
//!   because `LithairServer` has no equivalent `readiness_cfg` knob and
//!   leaving it out would make the endpoint silent.
//! * `GET /info`   → `200 <JSON document>` — server name, version,
//!   list of well-known endpoints, registered model base paths, and a
//!   UTC timestamp.

use bytes::Bytes;
use http::StatusCode;
use http_body_util::Full;
use hyper::Response;

use crate::app::response;

/// Path constant for the health endpoint.
pub(crate) const HEALTH_PATH: &str = "/health";

/// Path constant for the readiness endpoint.
pub(crate) const READY_PATH: &str = "/ready";

/// Path constant for the info endpoint.
pub(crate) const INFO_PATH: &str = "/info";

/// Build the default response for `GET /health`.
///
/// Always returns `200 {"status":"healthy"}`. The body matches the
/// `DeclarativeServer` implementation byte-for-byte so existing
/// monitoring tooling that scrapes either builder sees the same shape.
pub(crate) fn serve_health() -> Response<Full<Bytes>> {
    response::json(StatusCode::OK, r#"{"status":"healthy"}"#)
}

/// Build the default response for `GET /ready`.
///
/// Returns `200` with `{"status":"ready","version":"<crate version>"}`.
/// `LithairServer` has no opt-in readiness configuration today, so the
/// fields are intentionally minimal — extending this shape later
/// (consensus state, custom fields, etc.) is additive and won't break
/// existing clients.
pub(crate) fn serve_ready() -> Response<Full<Bytes>> {
    let body = format!(r#"{{"status":"ready","version":"{}"}}"#, env!("CARGO_PKG_VERSION"));
    response::json(StatusCode::OK, body)
}

/// Build the default response for `GET /info`.
///
/// `model_base_paths` is the list of currently registered model
/// base paths (e.g. `["/api/users", "/api/orders"]`) — empty when no
/// models are wired. The output is a JSON document describing the
/// server, the well-known endpoints it serves, and the registered
/// models, so operators can curl `/info` and confirm what's wired.
pub(crate) fn serve_info(model_base_paths: &[String]) -> Response<Full<Bytes>> {
    // Build the models array as a serde_json::Value so paths are
    // properly escaped — model base paths are user-supplied and should
    // not be inlined naively into a format string.
    let models_value = serde_json::Value::Array(
        model_base_paths.iter().map(|p| serde_json::Value::String(p.clone())).collect(),
    );
    let models_json = models_value.to_string();

    let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();

    let body = format!(
        r#"{{"server":"Lithair Server","version":"{version}","endpoints":{{"health":"/health","ready":"/ready","info":"/info"}},"models":{models},"timestamp":"{ts}"}}"#,
        version = env!("CARGO_PKG_VERSION"),
        models = models_json,
        ts = timestamp,
    );

    response::json(StatusCode::OK, body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    async fn body_to_string(resp: Response<Full<Bytes>>) -> String {
        let bytes = resp.into_body().collect().await.expect("collect body").to_bytes();
        String::from_utf8(bytes.to_vec()).expect("utf8 body")
    }

    #[tokio::test]
    async fn health_returns_200_with_canonical_body() {
        let resp = serve_health();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("Content-Type").map(|v| v.as_bytes()),
            Some(&b"application/json"[..])
        );
        let body = body_to_string(resp).await;
        assert_eq!(body, r#"{"status":"healthy"}"#);
    }

    #[tokio::test]
    async fn ready_returns_200_with_status_and_version() {
        let resp = serve_ready();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_to_string(resp).await;
        let parsed: serde_json::Value =
            serde_json::from_str(&body).expect("ready body must be valid JSON");
        assert_eq!(parsed["status"], "ready");
        assert_eq!(parsed["version"], env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn info_returns_200_with_server_metadata() {
        let resp = serve_info(&["/api/users".to_string(), "/api/orders".to_string()]);
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_to_string(resp).await;
        let parsed: serde_json::Value =
            serde_json::from_str(&body).expect("info body must be valid JSON");
        assert_eq!(parsed["server"], "Lithair Server");
        assert_eq!(parsed["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(parsed["endpoints"]["health"], "/health");
        assert_eq!(parsed["endpoints"]["ready"], "/ready");
        assert_eq!(parsed["endpoints"]["info"], "/info");
        let models = parsed["models"].as_array().expect("models must be a JSON array");
        assert_eq!(models.len(), 2);
        assert_eq!(models[0], "/api/users");
        assert_eq!(models[1], "/api/orders");
        assert!(parsed["timestamp"].is_string());
    }

    #[tokio::test]
    async fn info_with_no_models_emits_empty_array() {
        let resp = serve_info(&[]);
        let body = body_to_string(resp).await;
        let parsed: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
        let models = parsed["models"].as_array().expect("models is an array");
        assert!(models.is_empty());
    }

    #[tokio::test]
    async fn info_escapes_model_base_paths() {
        // Paranoid check: a base path containing a quote must not break the JSON.
        let resp = serve_info(&[r#"/api/"weird""#.to_string()]);
        let body = body_to_string(resp).await;
        // If escaping is wrong, parse will fail.
        let parsed: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
        assert_eq!(parsed["models"][0], r#"/api/"weird""#);
    }
}
