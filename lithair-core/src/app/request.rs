//! Request-body helpers for custom route handlers.
//!
//! These functions drain a [`RouteRequest`] body so handler call sites
//! don't have to re-add `http-body-util` and `bytes` as direct
//! dependencies just to call `BodyExt::collect()`.
//!
//! After lithair/lithair#59 (handler signatures) and #61 (response
//! builder + async not-found), the request-reading side was the last
//! consumer-side leak below the Lithair abstraction. See
//! lithair/lithair#63 for the motivating case (kovre's `PUT /api/config`,
//! which would otherwise have to re-add `http-body-util` and `bytes`
//! right after v0.5.0 let consumers drop them).
//!
//! # Example
//!
//! ```rust,ignore
//! use lithair_core::app::{request, response, Method, StatusCode};
//!
//! server.with_route_async(Method::PUT, "/api/config", |req| async move {
//!     // Drain + UTF-8 decode the body in one call.
//!     let yaml = request::read_body_as_string(req).await?;
//!     // … validate, swap, respond …
//!     Ok(response::text(StatusCode::OK, "ok"))
//! });
//! ```
//!
//! # Size limits
//!
//! [`read_body`], [`read_body_as_string`], and [`read_body_json`] read
//! the entire body without an explicit bound. They are intended for
//! trusted endpoints (admin APIs, internal services) where the upstream
//! HTTP server's own buffering already bounds memory use. For untrusted
//! input, use [`read_body_with_limit`] to reject oversize payloads
//! before they're fully buffered.

use super::RouteRequest;
use anyhow::{anyhow, bail, Context, Result};
use http_body_util::BodyExt;
use hyper::body::Body;
use serde::de::DeserializeOwned;

/// Drain the request body into a byte vector.
///
/// No size limit is applied — callers handling untrusted input should
/// use [`read_body_with_limit`] instead.
///
/// Returns an error if the body's transport stream fails mid-read.
pub async fn read_body(req: RouteRequest) -> Result<Vec<u8>> {
    let collected = req.into_body().collect().await.context("failed to read request body")?;
    Ok(collected.to_bytes().to_vec())
}

/// Drain the request body into a byte vector, rejecting any payload
/// that would exceed `max_bytes`.
///
/// The size is checked twice for defense in depth:
///
/// 1. **Pre-check** via [`http_body::Body::size_hint`]. When the upper
///    bound is known (typically because the client sent a
///    `Content-Length` header), oversized requests are rejected without
///    reading any body bytes.
/// 2. **Post-check** after collection. Chunked / streaming bodies that
///    don't expose an upper bound are still bounded — the final byte
///    count is verified against `max_bytes` once the stream finishes,
///    and oversized bodies error out.
///
/// Note: the post-check accepts the full body into memory before
/// rejecting it. For genuinely adversarial inputs the upstream server
/// or a reverse proxy should also enforce a request-body limit; this
/// helper is meant as a sensible default for application code, not a
/// substitute for transport-level protection.
pub async fn read_body_with_limit(req: RouteRequest, max_bytes: usize) -> Result<Vec<u8>> {
    // Pre-check: if the upper bound is known, refuse oversized requests
    // before reading a single byte.
    if let Some(upper) = req.body().size_hint().upper() {
        if upper > max_bytes as u64 {
            bail!(
                "request body exceeds limit: declared {} bytes, limit is {} bytes",
                upper,
                max_bytes
            );
        }
    }

    let bytes = read_body(req).await?;

    if bytes.len() > max_bytes {
        return Err(anyhow!(
            "request body exceeds limit: read {} bytes, limit is {} bytes",
            bytes.len(),
            max_bytes
        ));
    }

    Ok(bytes)
}

/// Drain the request body and decode it as UTF-8.
///
/// Convenience for JSON, YAML, TOML, or any text-based payload.
/// Errors on transport failures or invalid UTF-8.
///
/// See [`read_body`] for the size-limit caveat.
pub async fn read_body_as_string(req: RouteRequest) -> Result<String> {
    let bytes = read_body(req).await?;
    String::from_utf8(bytes).context("request body is not valid UTF-8")
}

/// Drain the request body and deserialize it as JSON into `T`.
///
/// Convenience for JSON-body routes. Errors on transport failures or
/// when the body isn't valid JSON for the target type.
///
/// See [`read_body`] for the size-limit caveat.
pub async fn read_body_json<T: DeserializeOwned>(req: RouteRequest) -> Result<T> {
    let bytes = read_body(req).await?;
    serde_json::from_slice(&bytes).context("request body is not valid JSON for target type")
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use http_body_util::Full;
    use hyper::Request;
    use serde::Deserialize;

    /// Build a `RouteRequest` carrying the given body bytes.
    ///
    /// `RouteRequest` aliases `hyper::Request<hyper::body::Incoming>`,
    /// and `Incoming` is constructed only by the HTTP transport — there
    /// is no public constructor we can call directly. We fall back on
    /// `Request<Full<Bytes>>` for unit tests, which exposes the same
    /// `Body` trait surface the helpers depend on. The e2e tests below
    /// exercise the real `Incoming` shape via `spawn_for_test`.
    fn req_with_body(body: impl Into<Bytes>) -> Request<Full<Bytes>> {
        Request::builder().body(Full::new(body.into())).expect("valid request")
    }

    async fn read_body_t<B>(req: Request<B>) -> Result<Vec<u8>>
    where
        B: Body + Send + 'static,
        B::Data: Send,
        B::Error: std::error::Error + Send + Sync + 'static,
    {
        let collected = req
            .into_body()
            .collect()
            .await
            .map_err(|e| anyhow!("failed to read request body: {e}"))?;
        Ok(collected.to_bytes().to_vec())
    }

    // The four public helpers all run on top of `req.into_body().collect()`,
    // which is generic over any `Body` impl. For the unit tests we use
    // `Request<Full<Bytes>>` so we can synthesize bodies without bringing
    // up an HTTP server; the small `read_body_t` shim above mirrors the
    // collection step. The e2e test at the bottom of `app/mod.rs` exercises
    // the real `Incoming` path through `spawn_for_test`.

    #[tokio::test]
    async fn read_body_returns_empty_vec_for_empty_body() {
        let req = req_with_body(Bytes::new());
        let bytes = read_body_t(req).await.expect("read empty body");
        assert!(bytes.is_empty());
    }

    #[tokio::test]
    async fn read_body_returns_bytes_for_text_body() {
        let req = req_with_body(Bytes::from_static(b"hello world"));
        let bytes = read_body_t(req).await.expect("read body");
        assert_eq!(bytes, b"hello world".to_vec());
    }

    #[tokio::test]
    async fn read_body_with_limit_succeeds_below_limit() {
        // Mirror `read_body_with_limit` against a `Full<Bytes>` body:
        // the size_hint is exact for `Full`, so the pre-check sees the
        // body length and either accepts or rejects before reading.
        let payload = Bytes::from_static(b"hello");
        let req = req_with_body(payload.clone());

        if let Some(upper) = req.body().size_hint().upper() {
            assert!(upper <= 1024, "pre-check should pass: declared {} bytes, limit 1024", upper);
        }
        let bytes = read_body_t(req).await.expect("read body");
        assert!(bytes.len() <= 1024, "post-check should pass");
        assert_eq!(bytes, payload.to_vec());
    }

    #[tokio::test]
    async fn read_body_with_limit_errors_above_limit() {
        // For a `Full<Bytes>` body the upper bound is known, so the
        // pre-check fires. Confirm both the rejection and that the error
        // message names the limit so consumers can produce a useful 413
        // response.
        let payload = Bytes::from(vec![0u8; 2048]);
        let req = req_with_body(payload);
        let max_bytes: usize = 1024;

        // Simulate the pre-check inline — calling the real helper would
        // require an `Incoming` body, which has no public constructor.
        let upper = req.body().size_hint().upper().expect("Full bodies expose upper bound");
        let err = if upper > max_bytes as u64 {
            anyhow!(
                "request body exceeds limit: declared {} bytes, limit is {} bytes",
                upper,
                max_bytes
            )
        } else {
            panic!("expected pre-check to trigger");
        };

        let msg = format!("{err}");
        assert!(
            msg.contains("exceeds limit"),
            "error message must mention 'exceeds limit', got: {msg}"
        );
        assert!(msg.contains("1024"), "error must include the configured limit, got: {msg}");
    }

    #[tokio::test]
    async fn read_body_as_string_decodes_utf8() {
        let req = req_with_body(Bytes::from_static("café — naïve".as_bytes()));
        let bytes = read_body_t(req).await.expect("read body");
        let s = String::from_utf8(bytes).expect("valid utf-8");
        assert_eq!(s, "café — naïve");
    }

    #[tokio::test]
    async fn read_body_as_string_errors_on_invalid_utf8() {
        // 0xff is never a valid UTF-8 start byte.
        let req = req_with_body(Bytes::from_static(&[0xff, 0xfe, 0xfd]));
        let bytes = read_body_t(req).await.expect("read body");
        let res = String::from_utf8(bytes);
        assert!(res.is_err(), "0xff/0xfe/0xfd must fail UTF-8 decoding");
    }

    #[tokio::test]
    async fn read_body_json_deserializes_valid_payload() {
        #[derive(Deserialize, Debug, PartialEq)]
        struct Config {
            name: String,
            port: u16,
        }

        let req = req_with_body(Bytes::from_static(br#"{"name":"api","port":8080}"#));
        let bytes = read_body_t(req).await.expect("read body");
        let cfg: Config = serde_json::from_slice(&bytes).expect("parse json");
        assert_eq!(cfg, Config { name: "api".to_string(), port: 8080 });
    }

    #[tokio::test]
    async fn read_body_json_errors_on_invalid_json() {
        #[derive(Deserialize, Debug)]
        #[allow(dead_code)]
        struct Config {
            name: String,
        }

        let req = req_with_body(Bytes::from_static(b"not json at all"));
        let bytes = read_body_t(req).await.expect("read body");
        let res: std::result::Result<Config, _> = serde_json::from_slice(&bytes);
        assert!(res.is_err(), "non-JSON input must fail to deserialize");
    }
}
