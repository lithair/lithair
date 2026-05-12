//! Response helpers for custom route handlers.
//!
//! These functions simplify building common HTTP responses
//! so you don't need to assemble `Response::builder()` chains by hand.
//!
//! Every helper returns a [`RouteResponse`] — the same type aliased in
//! [`crate::app`] — so handler call sites can drop direct deps on `hyper`,
//! `http-body-util`, and `bytes` (see lithair/lithair#59).
//!
//! # Example
//!
//! ```rust,ignore
//! use lithair_core::app::{response, StatusCode};
//!
//! // In a route handler:
//! Ok(response::json(StatusCode::OK, r#"{"status":"ok"}"#))
//! ```

use super::RouteResponse;
use bytes::Bytes;
use http::StatusCode;
use http_body_util::Full;
use hyper::Response;
use serde::Serialize;
use serde_json::Value;

/// JSON response with the given status code.
///
/// Body is taken as a string — useful when you already have a
/// pre-serialized JSON payload. If you're building the body via
/// `serde_json::json!` or hold a `serde_json::Value`, prefer
/// [`json_value`] to avoid the `.to_string()` round-trip. If you're
/// serializing a typed struct, prefer [`json_serialize`].
pub fn json(status: StatusCode, body: impl Into<String>) -> RouteResponse {
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(body.into())))
        .expect("valid HTTP response")
}

/// JSON response built directly from a [`serde_json::Value`].
///
/// Eliminates the `.to_string()` boilerplate at every call site when
/// the body is already a `Value` (e.g. built with `serde_json::json!`).
/// The bytes are produced via [`serde_json::to_vec`], which cannot fail
/// for a well-formed `Value`, so this function returns a `Response`
/// directly (no `Result`).
///
/// Sets `Content-Type: application/json`, identical to [`json`].
///
/// # Example
///
/// ```rust,ignore
/// use lithair_core::app::response;
/// use http::StatusCode;
/// use serde_json::json;
///
/// let resp = response::json_value(
///     StatusCode::ACCEPTED,
///     &json!({"id": 42, "status": "queued"}),
/// );
/// ```
pub fn json_value(status: StatusCode, body: &Value) -> RouteResponse {
    // `serde_json::to_vec` only fails when a custom `Serialize` impl
    // emits an error or a map has non-string keys. A `Value` produced
    // by `serde_json::json!` or `Value` constructors can never trigger
    // either condition, so the `expect` here is unreachable in practice.
    let bytes = serde_json::to_vec(body).expect("serde_json::Value always serializes");
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(bytes)))
        .expect("valid HTTP response")
}

/// JSON response built by serializing any `Serialize` value.
///
/// Useful when you have a typed struct (`#[derive(Serialize)]`) and
/// want to ship it as a JSON body without going through a
/// `serde_json::Value` intermediate. Returns a `Result` because
/// arbitrary `Serialize` implementations can fail (e.g. maps with
/// non-string keys).
///
/// Sets `Content-Type: application/json`, identical to [`json`].
///
/// # Example
///
/// ```rust,ignore
/// use lithair_core::app::response;
/// use http::StatusCode;
/// use serde::Serialize;
///
/// #[derive(Serialize)]
/// struct Created { id: u64 }
///
/// let resp = response::json_serialize(StatusCode::CREATED, &Created { id: 42 })?;
/// # Ok::<(), serde_json::Error>(())
/// ```
pub fn json_serialize<T: Serialize + ?Sized>(
    status: StatusCode,
    body: &T,
) -> Result<RouteResponse, serde_json::Error> {
    let bytes = serde_json::to_vec(body)?;
    Ok(Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(bytes)))
        .expect("valid HTTP response"))
}

/// Plain-text response with the given status code.
pub fn text(status: StatusCode, body: impl Into<String>) -> RouteResponse {
    Response::builder()
        .status(status)
        .header("Content-Type", "text/plain; charset=utf-8")
        .body(Full::new(Bytes::from(body.into())))
        .expect("valid HTTP response")
}

/// HTML response with the given status code.
pub fn html(status: StatusCode, body: impl Into<String>) -> RouteResponse {
    Response::builder()
        .status(status)
        .header("Content-Type", "text/html; charset=utf-8")
        .body(Full::new(Bytes::from(body.into())))
        .expect("valid HTTP response")
}

/// 302 redirect to the given location.
pub fn redirect(location: &str) -> RouteResponse {
    Response::builder()
        .status(StatusCode::FOUND)
        .header("Location", location)
        .body(Full::new(Bytes::new()))
        .expect("valid HTTP response")
}

/// Empty-body response with the given status code.
pub fn empty(status: StatusCode) -> RouteResponse {
    Response::builder()
        .status(status)
        .body(Full::new(Bytes::new()))
        .expect("valid HTTP response")
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;
    use serde_json::json;

    async fn body_bytes(resp: RouteResponse) -> Vec<u8> {
        resp.into_body().collect().await.expect("collect body").to_bytes().to_vec()
    }

    #[tokio::test]
    async fn json_value_simple_object() {
        let value = json!({"id": 42, "status": "queued"});
        let resp = json_value(StatusCode::ACCEPTED, &value);

        assert_eq!(resp.status(), StatusCode::ACCEPTED);
        assert_eq!(
            resp.headers().get("Content-Type").map(|h| h.to_str().unwrap()),
            Some("application/json")
        );

        let got = body_bytes(resp).await;
        let want = serde_json::to_vec(&value).unwrap();
        assert_eq!(got, want);

        // Round-trip through Value to confirm the body is valid JSON
        // matching the input — defends against future changes that
        // might silently corrupt the body (e.g. extra trailing bytes).
        let parsed: serde_json::Value = serde_json::from_slice(&got).unwrap();
        assert_eq!(parsed, value);
    }

    #[tokio::test]
    async fn json_value_nested_structure() {
        let value = json!({
            "user": {
                "id": 7,
                "name": "Ada",
                "roles": ["admin", "engineer"],
            },
            "metadata": {
                "created_at": "2026-05-11T10:00:00Z",
                "tags": [],
            },
            "count": 0,
            "active": true,
            "deleted": null,
        });
        let resp = json_value(StatusCode::OK, &value);

        assert_eq!(resp.status(), StatusCode::OK);

        let got = body_bytes(resp).await;
        assert_eq!(got, serde_json::to_vec(&value).unwrap());

        let parsed: serde_json::Value = serde_json::from_slice(&got).unwrap();
        assert_eq!(parsed, value);
    }

    #[tokio::test]
    async fn json_serialize_typed_struct() {
        #[derive(Serialize)]
        struct Created {
            id: u64,
            name: &'static str,
        }

        let payload = Created { id: 42, name: "widget" };
        let resp =
            json_serialize(StatusCode::CREATED, &payload).expect("Serialize struct never fails");

        assert_eq!(resp.status(), StatusCode::CREATED);
        assert_eq!(
            resp.headers().get("Content-Type").map(|h| h.to_str().unwrap()),
            Some("application/json")
        );

        let got = body_bytes(resp).await;
        let parsed: serde_json::Value = serde_json::from_slice(&got).unwrap();
        assert_eq!(parsed, json!({"id": 42, "name": "widget"}));
    }
}
