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
use http_body_util::BodyExt;
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
        .body(Full::new(Bytes::from(body.into())).boxed())
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
        .body(Full::new(Bytes::from(bytes)).boxed())
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
        .body(Full::new(Bytes::from(bytes)).boxed())
        .expect("valid HTTP response"))
}

/// Plain-text response with the given status code.
pub fn text(status: StatusCode, body: impl Into<String>) -> RouteResponse {
    Response::builder()
        .status(status)
        .header("Content-Type", "text/plain; charset=utf-8")
        .body(Full::new(Bytes::from(body.into())).boxed())
        .expect("valid HTTP response")
}

/// HTML response with the given status code.
pub fn html(status: StatusCode, body: impl Into<String>) -> RouteResponse {
    Response::builder()
        .status(status)
        .header("Content-Type", "text/html; charset=utf-8")
        .body(Full::new(Bytes::from(body.into())).boxed())
        .expect("valid HTTP response")
}

/// 302 redirect to the given location.
pub fn redirect(location: &str) -> RouteResponse {
    Response::builder()
        .status(StatusCode::FOUND)
        .header("Location", location)
        .body(Full::new(Bytes::new()).boxed())
        .expect("valid HTTP response")
}

/// Empty-body response with the given status code.
pub fn empty(status: StatusCode) -> RouteResponse {
    Response::builder()
        .status(status)
        .body(Full::new(Bytes::new()).boxed())
        .expect("valid HTTP response")
}

/// Start a chained response builder for cases that need custom headers
/// (`Cache-Control`, `ETag`, `Location`, custom CORS) on top of an
/// arbitrary body — the gap left by [`json`] / [`json_value`] / [`text`]
/// / [`html`], which hard-code `Content-Type` and nothing else.
///
/// The builder wraps [`hyper::Response::builder`] internally and produces
/// a [`RouteResponse`] at the terminal step. Consumers that previously
/// dropped to direct `hyper` / `http-body-util` / `bytes` deps just to
/// set a couple of headers can keep their `Cargo.toml` lean.
///
/// # Default status
///
/// `200 OK` — matches `hyper::Response::builder()`. Override with
/// [`ResponseBuilder::status`].
///
/// # Example
///
/// ```rust,ignore
/// use lithair_core::app::{response, StatusCode};
/// use bytes::Bytes;
///
/// let asset_bytes: Bytes = Bytes::from_static(b"...");
/// let resp = response::builder()
///     .status(StatusCode::OK)
///     .header("content-type", "application/wasm")
///     .header("cache-control", "public, max-age=31536000, immutable")
///     .body(asset_bytes);
/// ```
pub fn builder() -> ResponseBuilder {
    ResponseBuilder::new()
}

/// Chained builder for [`RouteResponse`] with arbitrary headers.
///
/// Created via [`builder`]. Wraps `hyper::Response::builder()` and only
/// commits to a body at the terminal step ([`Self::body`] /
/// [`Self::json_value`]), so the intermediate methods stay infallible
/// and chainable.
///
/// See [`builder`] for usage and motivation.
pub struct ResponseBuilder {
    inner: hyper::http::response::Builder,
}

impl ResponseBuilder {
    fn new() -> Self {
        Self { inner: Response::builder() }
    }

    /// Set the HTTP status code. Defaults to `200 OK` if not called.
    pub fn status(mut self, status: StatusCode) -> Self {
        self.inner = self.inner.status(status);
        self
    }

    /// Append a header. Same semantics as `hyper::Response::builder().header(...)`:
    /// multiple calls with the same key append rather than overwrite, which
    /// matches HTTP-level multi-value header behaviour (`Set-Cookie`, etc.).
    ///
    /// Accepts anything `AsRef<str>` — `&str`, `String`, `Cow<str>`, etc. —
    /// so consumers don't have to coerce types at every call site.
    pub fn header(mut self, key: impl AsRef<str>, value: impl AsRef<str>) -> Self {
        self.inner = self.inner.header(key.as_ref(), value.as_ref());
        self
    }

    /// Terminate the chain with an explicit body and produce a
    /// [`RouteResponse`].
    ///
    /// Accepts anything that converts into [`Bytes`] (`Bytes`, `Vec<u8>`,
    /// `&'static [u8]`, `String`, `&'static str`, …) so static-asset and
    /// dynamic-payload callers share the same shape.
    pub fn body(self, body: impl Into<Bytes>) -> RouteResponse {
        self.inner.body(Full::new(body.into()).boxed()).expect("valid HTTP response")
    }

    /// Terminate the chain with a [`serde_json::Value`] body, setting
    /// `content-type: application/json` and serializing the value to
    /// bytes via [`serde_json::to_vec`].
    ///
    /// Equivalent to chaining `.header("content-type", "application/json")`
    /// before `.body(serde_json::to_vec(value).unwrap())`, but spells
    /// the common case out in one call.
    ///
    /// As with [`json_value`], serialization of a [`serde_json::Value`]
    /// produced by `serde_json::json!` cannot fail, so this returns a
    /// [`RouteResponse`] directly (no `Result`).
    pub fn json_value(self, value: &Value) -> RouteResponse {
        let bytes = serde_json::to_vec(value).expect("serde_json::Value always serializes");
        self.inner
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(bytes)).boxed())
            .expect("valid HTTP response")
    }
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

    // ------------------------------------------------------------------
    // `response::builder()` chained builder (issue #61).
    //
    // The motivating use case is serving content-addressed static assets
    // with `Cache-Control: immutable` — see kovre's `asset_response()`,
    // which today imports `bytes`, `http-body-util`, and `hyper` just to
    // set two headers. These tests pin the surface so consumers can rely
    // on it.
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn builder_default_status_is_200_ok() {
        // Skipping `.status(...)` must yield `200 OK`, matching
        // `hyper::Response::builder()` defaults — otherwise consumers
        // get a surprise when porting from direct hyper usage.
        let resp = builder().body(Bytes::from_static(b"hello"));
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn builder_terminates_with_explicit_body_bytes() {
        let resp = builder().status(StatusCode::CREATED).body(Bytes::from("hello"));

        assert_eq!(resp.status(), StatusCode::CREATED);
        let got = body_bytes(resp).await;
        assert_eq!(got, b"hello".to_vec());
    }

    #[tokio::test]
    async fn builder_with_custom_headers_emits_them() {
        // The whole point of `builder()` is letting consumers set
        // arbitrary headers (Cache-Control on static assets, Location
        // on redirects, etc.) without dropping to `hyper::Response::builder`.
        // Verify each one round-trips verbatim.
        let resp = builder()
            .status(StatusCode::OK)
            .header("content-type", "application/wasm")
            .header("cache-control", "public, max-age=31536000, immutable")
            .body(Bytes::from_static(b"\0asm"));

        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("content-type").map(|h| h.to_str().unwrap()),
            Some("application/wasm")
        );
        assert_eq!(
            resp.headers().get("cache-control").map(|h| h.to_str().unwrap()),
            Some("public, max-age=31536000, immutable")
        );

        let got = body_bytes(resp).await;
        assert_eq!(got, b"\0asm".to_vec());
    }

    #[tokio::test]
    async fn builder_json_value_sets_content_type_and_body() {
        let value = json!({"x": 1, "items": ["a", "b"]});
        let resp = builder().status(StatusCode::ACCEPTED).json_value(&value);

        assert_eq!(resp.status(), StatusCode::ACCEPTED);
        assert_eq!(
            resp.headers().get("content-type").map(|h| h.to_str().unwrap()),
            Some("application/json")
        );

        let got = body_bytes(resp).await;
        let parsed: serde_json::Value = serde_json::from_slice(&got).unwrap();
        assert_eq!(parsed, value);
    }

    #[tokio::test]
    async fn builder_accepts_string_and_static_str_bodies() {
        // `body: impl Into<Bytes>` must accept `&'static str` and `String`
        // — the two shapes consumers most often hold. If this stops
        // compiling, the bound regressed.
        let _r1 = builder().body("hello");
        let _r2 = builder().body(String::from("hello"));
        let _r3 = builder().body(Vec::<u8>::from(b"hello".as_slice()));
    }
}
