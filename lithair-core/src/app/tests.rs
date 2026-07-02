//! In-module integration tests for the server dispatch (moved out of
//! mod.rs in the app-module split; behavior unchanged).

use super::*;
use super::*;

#[test]
fn test_server_creation() {
    let _server = LithairServer::default();
}

/// `init_default_tracing` must be idempotent (issue #107): both the
/// subscriber install and the `log` bridge install are try-style, so
/// a second call (or a user-installed logger/subscriber racing it)
/// must be a silent no-op — exactly the contract the historical
/// `env_logger::try_init()` had.
#[test]
fn init_default_tracing_is_idempotent() {
    init_default_tracing();
    init_default_tracing();
}

// ------------------------------------------------------------------
// X-Request-ID sanitization (issue #107). The end-to-end echo
// behavior is covered in tests/request_id_test.rs; these cover the
// pure validation rules in isolation.
// ------------------------------------------------------------------

fn headers_with_request_id(value: &[u8]) -> hyper::HeaderMap {
    let mut headers = hyper::HeaderMap::new();
    headers.insert(
        "x-request-id",
        hyper::header::HeaderValue::from_bytes(value).expect("test header value"),
    );
    headers
}

#[test]
fn request_id_accepts_sane_inbound_value() {
    let headers = headers_with_request_id(b"trace-abc.123_456");
    assert_eq!(request_id_from_headers(&headers), "trace-abc.123_456");
}

#[test]
fn request_id_generates_uuid_when_absent() {
    let id = request_id_from_headers(&hyper::HeaderMap::new());
    assert!(uuid::Uuid::parse_str(&id).is_ok(), "expected UUID, got {id:?}");
}

#[test]
fn request_id_rejects_oversized_value() {
    let oversized = vec![b'a'; 129];
    let id = request_id_from_headers(&headers_with_request_id(&oversized));
    assert!(uuid::Uuid::parse_str(&id).is_ok(), "expected fresh UUID, got {id:?}");
}

#[test]
fn request_id_rejects_non_printable_and_empty_values() {
    // All of these are *valid* HeaderValue bytes (so they can arrive
    // on the wire) but sit outside our visible-ASCII 0x21..=0x7E
    // window: space and tab are header-legal whitespace, 0xC3 0xA9 is
    // obs-text (non-ASCII, `to_str()` fails), and empty values are
    // useless as correlation IDs. DEL/CR/LF need no test — the http
    // crate rejects them before a HeaderValue can even exist.
    for bad in [&b"has space"[..], &b"\ttab"[..], &b"caf\xc3\xa9"[..], &b""[..]] {
        let id = request_id_from_headers(&headers_with_request_id(bad));
        assert_ne!(id.as_bytes(), bad);
        assert!(uuid::Uuid::parse_str(&id).is_ok(), "expected fresh UUID, got {id:?}");
    }
}

#[test]
fn request_id_accepts_max_length_boundary() {
    let max = vec![b'x'; 128];
    let id = request_id_from_headers(&headers_with_request_id(&max));
    assert_eq!(id.len(), 128);
}

// ------------------------------------------------------------------
// Built-in operations endpoints (`/health`, `/ready`, `/info`).
//
// These tests cover the LithairServer regression reported in
// lithair/lithair#40: the README claims every Lithair server
// ships with /health, /ready, /info, but `LithairServer` had no
// dispatch for them and returned 404. We spin up a `LithairServer`
// via `build()` (skipping the heavy `serve()` startup — schema
// load, model factories, system metrics — none of which are
// relevant here), bind a hyper service to a loopback ephemeral
// port, and exercise the dispatch with a real reqwest client.
// ------------------------------------------------------------------

/// Serve a `LithairServer` on a loopback ephemeral port, return
/// (base_url, abort_handle). Drop the abort_handle (or call
/// `.abort()`) to stop the server.
///
/// We deliberately *don't* call `LithairServer::serve()` here —
/// it pulls in schema validation, tracing init, frontend
/// loading, and system metrics. None of that is wired for these
/// tests, and serve() also blocks forever, which would deadlock
/// the test harness.
async fn spawn_for_test(server: LithairServer) -> (String, tokio::task::JoinHandle<()>) {
    use hyper::service::service_fn;
    use hyper_util::rt::TokioIo;
    use std::sync::Arc;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local_addr");
    let base = format!("http://{}", addr);

    let server = Arc::new(server);

    let handle = tokio::spawn(async move {
        loop {
            let (stream, _peer) = match listener.accept().await {
                Ok(s) => s,
                Err(_) => return,
            };
            let io = TokioIo::new(stream);
            let server = server.clone();
            tokio::spawn(async move {
                let service = service_fn(move |req| {
                    let server = server.clone();
                    async move {
                        match server.handle_request(req).await {
                            Ok(resp) => Ok::<_, std::convert::Infallible>(resp),
                            Err(_) => Ok(hyper::Response::builder()
                                .status(500)
                                .body(boxed_full(bytes::Bytes::from(
                                    r#"{"error":"handler error"}"#,
                                )))
                                .expect("valid HTTP response")),
                        }
                    }
                });
                let _ =
                    hyper::server::conn::http1::Builder::new().serve_connection(io, service).await;
            });
        }
    });

    (base, handle)
}

#[tokio::test]
async fn lithair_server_serves_default_health() {
    let server = LithairServer::new().build().expect("build server");
    let (base, handle) = spawn_for_test(server).await;

    let resp = reqwest::get(format!("{}/health", base)).await.expect("GET /health succeeded");
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.expect("read body");
    assert_eq!(body, r#"{"status":"healthy"}"#);

    handle.abort();
}

#[tokio::test]
async fn lithair_server_serves_default_ready() {
    let server = LithairServer::new().build().expect("build server");
    let (base, handle) = spawn_for_test(server).await;

    let resp = reqwest::get(format!("{}/ready", base)).await.expect("GET /ready succeeded");
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.expect("read body");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("ready body must be JSON");
    assert_eq!(parsed["status"], "ready");
    assert_eq!(parsed["version"], env!("CARGO_PKG_VERSION"));

    handle.abort();
}

#[tokio::test]
async fn lithair_server_serves_default_info() {
    let server = LithairServer::new().build().expect("build server");
    let (base, handle) = spawn_for_test(server).await;

    let resp = reqwest::get(format!("{}/info", base)).await.expect("GET /info succeeded");
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.expect("read body");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("info body must be JSON");
    assert_eq!(parsed["server"], "Lithair Server");
    assert_eq!(parsed["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(parsed["endpoints"]["health"], "/health");
    assert_eq!(parsed["endpoints"]["ready"], "/ready");
    assert_eq!(parsed["endpoints"]["info"], "/info");
    // No models registered, so the array must be empty.
    assert!(parsed["models"].as_array().expect("models array").is_empty());

    handle.abort();
}

#[tokio::test]
async fn lithair_server_user_with_route_overrides_default_health() {
    // A user calling `.with_route(GET, "/health", ...)` must win
    // over the built-in handler. The dispatch order in
    // handle_request places the custom_routes loop *before* the
    // ops endpoints precisely so this works.
    let server = LithairServer::new()
        .with_route(http::Method::GET, "/health", |_req| {
            Box::pin(async move {
                Ok(hyper::Response::builder()
                    .status(418)
                    .header("Content-Type", "application/json")
                    .body(boxed_full(bytes::Bytes::from(r#"{"status":"i-am-a-teapot"}"#)))
                    .expect("valid HTTP response"))
            })
        })
        .build()
        .expect("build server");
    let (base, handle) = spawn_for_test(server).await;

    let resp = reqwest::get(format!("{}/health", base)).await.expect("GET /health succeeded");
    assert_eq!(resp.status(), 418, "user override must take precedence");
    let body = resp.text().await.expect("read body");
    assert_eq!(body, r#"{"status":"i-am-a-teapot"}"#);

    handle.abort();
}

#[tokio::test]
async fn lithair_server_returns_404_for_non_get_health() {
    // The dispatch checks `method == GET`, so a POST /health
    // should still 404 (no built-in POST /health handler).
    let server = LithairServer::new().build().expect("build server");
    let (base, handle) = spawn_for_test(server).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/health", base))
        .send()
        .await
        .expect("POST /health succeeded");
    assert_eq!(resp.status(), 404);

    handle.abort();
}

// ------------------------------------------------------------------
// Static-file HEAD support (issue #56).
//
// Before the fix, the static-file dispatch in `handle_request` only
// matched `Method::GET`, and `FrontendServer::handle_request`
// returned `METHOD_NOT_ALLOWED` for anything else. The result: any
// HEAD probe on a perfectly-served static page (homepage, blog
// post, rss.xml) fell through to the default
// `{"error":"Not found"}` JSON 404 — silently breaking SEO
// crawlers, monitors (`curl -I`, Uptime Robot), and any other
// tooling that does HEAD-then-GET.
//
// RFC 7231 §4.3.2: HEAD must return the same status and headers
// as GET, with an empty body.
// ------------------------------------------------------------------

/// Build a LithairServer wired with a single in-memory frontend
/// engine serving the supplied (path, content, mime) tuples.
async fn server_with_frontend(assets: &[(&str, &[u8])]) -> (LithairServer, tempfile::TempDir) {
    // FrontendEngine::new requires an on-disk data_dir for the
    // event store; the tempdir is dropped when the test ends.
    let tmp = tempfile::tempdir().expect("tempdir for frontend event store");
    let engine = crate::frontend::FrontendEngine::new("test_static", tmp.path())
        .await
        .expect("create FrontendEngine");

    for (path, content) in assets {
        engine.update_asset(path, content.to_vec()).await.expect("insert asset");
    }

    let mut server = LithairServer::new().build().expect("build server");
    // Inject directly — bypasses the on-disk load_directory path
    // that LithairServer normally uses in `serve()` (#56 test
    // doesn't exercise that path).
    server.frontend_engines.insert("/".to_string(), std::sync::Arc::new(engine));
    (server, tmp)
}

#[tokio::test]
async fn static_file_get_returns_200_with_correct_content_type() {
    let (server, _tmp) = server_with_frontend(&[("/index.html", b"<h1>home</h1>")]).await;
    let (base, handle) = spawn_for_test(server).await;

    let resp = reqwest::get(format!("{}/index.html", base)).await.expect("GET /index.html");
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or(""),
        "text/html"
    );
    let body = resp.text().await.expect("body");
    assert_eq!(body, "<h1>home</h1>");

    handle.abort();
}

#[tokio::test]
async fn static_file_head_returns_200_with_correct_content_type_and_no_body() {
    // The original #56 reproduction: `curl -I /index.html` must
    // return 200 OK + text/html, not 404 + application/json.
    let (server, _tmp) = server_with_frontend(&[("/index.html", b"<h1>home</h1>")]).await;
    let (base, handle) = spawn_for_test(server).await;

    let client = reqwest::Client::new();
    let resp = client
        .head(format!("{}/index.html", base))
        .send()
        .await
        .expect("HEAD /index.html");

    assert_eq!(resp.status(), 200, "HEAD must mirror GET status");
    assert_eq!(
        resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or(""),
        "text/html",
        "HEAD must mirror GET content-type"
    );
    // Content-Length must describe what GET would have sent.
    assert_eq!(
        resp.headers().get("content-length").and_then(|v| v.to_str().ok()),
        Some("13"),
        "HEAD must advertise the GET payload size"
    );
    let body = resp.bytes().await.expect("body");
    assert!(body.is_empty(), "HEAD must not carry a body");

    handle.abort();
}

#[tokio::test]
async fn static_file_head_rss_xml_returns_200_with_xml_content_type() {
    // The #56 acceptance list calls out /rss.xml specifically —
    // RSS readers and feed validators issue HEAD before GET. The
    // content-type must come from the asset's MIME guess, not be
    // hard-coded to text/html.
    let xml = br#"<?xml version="1.0" encoding="UTF-8"?><rss/>"#;
    let (server, _tmp) = server_with_frontend(&[("/rss.xml", xml)]).await;
    let (base, handle) = spawn_for_test(server).await;

    let client = reqwest::Client::new();
    let resp = client.head(format!("{}/rss.xml", base)).send().await.expect("HEAD /rss.xml");

    assert_eq!(resp.status(), 200);
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    // mime_guess maps .xml to either text/xml or application/xml
    // depending on platform DB; the key invariant is "not
    // application/json" (the broken 404 default).
    assert!(ct.contains("xml"), "expected an XML content-type, got `{}`", ct);

    let body = resp.bytes().await.expect("body");
    assert!(body.is_empty(), "HEAD must not carry a body");

    handle.abort();
}

#[tokio::test]
async fn unknown_route_returns_404_with_json_error_negative_case() {
    // The negative case from #56's acceptance criteria: when no
    // frontend (and no other handler) matches, the default
    // `{"error":"Not found"}` JSON 404 must still fire — both for
    // GET and for HEAD. We deliberately do NOT register a
    // frontend here so the static dispatch is skipped entirely.
    let server = LithairServer::new().build().expect("build server");
    let (base, handle) = spawn_for_test(server).await;

    let client = reqwest::Client::new();

    // GET on a path with no handler.
    let resp = client
        .get(format!("{}/totally-unknown", base))
        .send()
        .await
        .expect("GET /totally-unknown");
    assert_eq!(resp.status(), 404);
    assert_eq!(
        resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or(""),
        "application/json"
    );
    let body = resp.text().await.expect("body");
    assert_eq!(body, r#"{"error":"Not found"}"#);

    // HEAD on the same unmatched path: hyper strips the body
    // automatically for HEAD responses on the wire, but the
    // status and content-type must still be the JSON 404 shape.
    let resp = client.head(format!("{}/totally-unknown", base)).send().await.expect("HEAD");
    assert_eq!(resp.status(), 404);
    assert_eq!(
        resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or(""),
        "application/json"
    );

    handle.abort();
}

// ------------------------------------------------------------------
// Route handler type aliases + `with_route_async` helper (issue #59).
//
// The aliases (`RouteRequest`, `RouteResponse`) and the re-exports
// (`Method`, `StatusCode`) exist so consumers can write handler
// signatures without depending on `bytes`, `http`, `http-body-util`,
// and `hyper` directly. The tests below prove:
//
// 1. The aliases are drop-in compatible with the existing
//    `with_route` signature (no behavior change).
// 2. The `with_route_async` helper accepts a plain async closure — no
//    `Box::pin` boilerplate at the call site.
// 3. Both registration paths route requests to the same dispatcher
//    and produce the same response shape, so consumers can pick
//    based on ergonomics alone.
// ------------------------------------------------------------------

#[tokio::test]
async fn with_route_alias_signature_compiles_and_serves() {
    // Prove `RouteRequest`/`RouteResponse` are drop-in replacements
    // for the long inline hyper types: register a route whose
    // closure uses *only* the public aliases plus the re-exported
    // `Method` / `StatusCode`, and dispatch a request through it.
    use super::{response, Method, RouteRequest, RouteResponse, StatusCode};

    let server = LithairServer::new()
        .with_route(Method::GET, "/issue-59-aliases", |_req: RouteRequest| {
            Box::pin(async move {
                let resp: RouteResponse = response::json(StatusCode::OK, r#"{"alias":"ok"}"#);
                Ok(resp)
            })
        })
        .build()
        .expect("build server");
    let (base, handle) = spawn_for_test(server).await;

    let resp = reqwest::get(format!("{}/issue-59-aliases", base))
        .await
        .expect("GET /issue-59-aliases");
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or(""),
        "application/json"
    );
    let body = resp.text().await.expect("body");
    assert_eq!(body, r#"{"alias":"ok"}"#);

    handle.abort();
}

#[tokio::test]
async fn with_route_async_compiles_without_box_pin_and_serves() {
    // `with_route_async` accepts a plain async closure — no manual
    // `Box::pin`, no explicit `Pin<Box<dyn Future>>` return type.
    // The dispatcher must still route the request correctly and
    // return the body the handler produced.
    use super::{response, Method, RouteRequest, StatusCode};

    let server = LithairServer::new()
        .with_route_async(Method::POST, "/issue-59-route-async", |_req: RouteRequest| async move {
            Ok(response::json(StatusCode::ACCEPTED, r#"{"status":"queued"}"#))
        })
        .build()
        .expect("build server");
    let (base, handle) = spawn_for_test(server).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/issue-59-route-async", base))
        .send()
        .await
        .expect("POST /issue-59-route-async");
    assert_eq!(resp.status(), 202);
    assert_eq!(
        resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or(""),
        "application/json"
    );
    let body = resp.text().await.expect("body");
    assert_eq!(body, r#"{"status":"queued"}"#);

    handle.abort();
}

#[tokio::test]
async fn with_route_async_and_with_route_share_dispatch_precedence() {
    // Two routes registered via the two registration paths must
    // both win against the default ops endpoints. This is a
    // regression test against `with_route_async` accidentally routing
    // through a different code path than `with_route` (which would
    // be a silent behavior split).
    use super::{response, Method, RouteRequest, StatusCode};

    let server = LithairServer::new()
        // Override the built-in /health via the *new* helper.
        .with_route_async(Method::GET, "/health", |_req: RouteRequest| async move {
            Ok(response::json(StatusCode::IM_A_TEAPOT, r#"{"status":"teapot-async"}"#))
        })
        // Override /ready via the existing `with_route` API.
        .with_route(Method::GET, "/ready", |_req: RouteRequest| {
            Box::pin(async move {
                Ok(response::json(StatusCode::IM_A_TEAPOT, r#"{"status":"teapot-with"}"#))
            })
        })
        .build()
        .expect("build server");
    let (base, handle) = spawn_for_test(server).await;

    let health = reqwest::get(format!("{}/health", base)).await.expect("GET /health succeeded");
    assert_eq!(health.status(), 418, "with_route_async override must take precedence");
    assert_eq!(health.text().await.expect("body"), r#"{"status":"teapot-async"}"#);

    let ready = reqwest::get(format!("{}/ready", base)).await.expect("GET /ready succeeded");
    assert_eq!(ready.status(), 418, "with_route override must still work");
    assert_eq!(ready.text().await.expect("body"), r#"{"status":"teapot-with"}"#);

    handle.abort();
}

// ------------------------------------------------------------------
// `with_not_found_handler_async` (issue #61).
//
// Mirrors the `with_route` → `with_route_async` pairing from v0.4.0:
// a plain async closure registers a 404 handler, no manual `Box::pin`.
// Tests:
//   1. The async variant routes through to the custom 404 path and
//      its body/status reach the wire.
//   2. The sync-pinned variant still works (regression guard).
// ------------------------------------------------------------------

#[tokio::test]
async fn with_not_found_handler_async_compiles_without_box_pin_and_serves() {
    use super::{response, RouteRequest, StatusCode};

    let server = LithairServer::new()
        .with_not_found_handler_async(|req: RouteRequest| async move {
            let path = req.uri().path().to_string();
            Ok(response::json_value(
                StatusCode::NOT_FOUND,
                &serde_json::json!({"error": "not_found", "path": path}),
            ))
        })
        .build()
        .expect("build server");
    let (base, handle) = spawn_for_test(server).await;

    let resp = reqwest::get(format!("{}/nope/missing", base)).await.expect("GET /nope/missing");
    assert_eq!(resp.status(), 404);
    assert_eq!(
        resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or(""),
        "application/json"
    );

    let body: serde_json::Value = resp.json().await.expect("json body");
    assert_eq!(body, serde_json::json!({"error": "not_found", "path": "/nope/missing"}));

    handle.abort();
}

#[tokio::test]
async fn with_not_found_handler_sync_pinned_still_works() {
    // Regression: the sync-pinned `with_not_found_handler` must keep
    // working unchanged. Adding the `_async` variant is purely additive.
    use super::{response, StatusCode};

    let server = LithairServer::new()
        .with_not_found_handler(|_req| {
            Box::pin(async { Ok(response::html(StatusCode::NOT_FOUND, "<h1>Page not found</h1>")) })
        })
        .build()
        .expect("build server");
    let (base, handle) = spawn_for_test(server).await;

    let resp = reqwest::get(format!("{}/nope", base)).await.expect("GET /nope");
    assert_eq!(resp.status(), 404);
    assert_eq!(
        resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or(""),
        "text/html; charset=utf-8"
    );
    let body = resp.text().await.expect("body");
    assert_eq!(body, "<h1>Page not found</h1>");

    handle.abort();
}

// ------------------------------------------------------------------
// `request::*` body-reading helpers (issue #63).
//
// The unit tests in `app/request.rs` exercise the helpers through
// `Request<Full<Bytes>>` because `hyper::body::Incoming` has no
// public constructor. This e2e test drives the helpers through the
// real wire path — request comes in as `Incoming`, handler calls
// the helper, response goes out — so we catch any signature
// mismatch the unit shims would miss.
// ------------------------------------------------------------------

#[tokio::test]
async fn request_read_body_as_string_drains_put_body_end_to_end() {
    use super::{request, response, Method, RouteRequest, StatusCode};

    let server = LithairServer::new()
        .with_route_async(Method::PUT, "/echo", |req: RouteRequest| async move {
            let body = request::read_body_as_string(req).await?;
            Ok(response::text(StatusCode::OK, body))
        })
        .build()
        .expect("build server");
    let (base, handle) = spawn_for_test(server).await;

    let client = reqwest::Client::new();
    let resp = client
        .put(format!("{}/echo", base))
        .body("config: ok\nname: kovre\n")
        .send()
        .await
        .expect("PUT /echo");
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.expect("body");
    assert_eq!(body, "config: ok\nname: kovre\n");

    handle.abort();
}

#[tokio::test]
async fn request_read_body_json_deserializes_put_body_end_to_end() {
    use super::{request, response, Method, RouteRequest, StatusCode};
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Payload {
        name: String,
        count: u32,
    }

    let server = LithairServer::new()
        .with_route_async(Method::POST, "/json", |req: RouteRequest| async move {
            let payload: Payload = request::read_body_json(req).await?;
            Ok(response::text(StatusCode::OK, format!("{} x{}", payload.name, payload.count)))
        })
        .build()
        .expect("build server");
    let (base, handle) = spawn_for_test(server).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/json", base))
        .header("content-type", "application/json")
        .body(r#"{"name":"widget","count":3}"#)
        .send()
        .await
        .expect("POST /json");
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.expect("body"), "widget x3");

    handle.abort();
}

#[tokio::test]
async fn request_read_body_with_limit_rejects_oversize_end_to_end() {
    // Send a 4 KiB payload to a route that caps the read at 1 KiB.
    // The handler returns 413 on rejection, mirroring how a real
    // consumer (kovre) would map the error.
    use super::{request, response, Method, RouteRequest, StatusCode};

    let server = LithairServer::new()
        .with_route_async(Method::PUT, "/limited", |req: RouteRequest| async move {
            match request::read_body_with_limit(req, 1024).await {
                Ok(_bytes) => Ok(response::text(StatusCode::OK, "ok")),
                Err(e) => Ok(response::text(StatusCode::PAYLOAD_TOO_LARGE, format!("{e}"))),
            }
        })
        .build()
        .expect("build server");
    let (base, handle) = spawn_for_test(server).await;

    let big = vec![b'a'; 4096];
    let client = reqwest::Client::new();
    let resp = client
        .put(format!("{}/limited", base))
        .body(big)
        .send()
        .await
        .expect("PUT /limited");
    assert_eq!(resp.status(), 413);
    let body = resp.text().await.expect("body");
    assert!(
        body.contains("exceeds limit"),
        "error body should describe the rejection, got: {body}"
    );

    handle.abort();
}

#[tokio::test]
async fn request_read_body_with_limit_accepts_undersize_end_to_end() {
    use super::{request, response, Method, RouteRequest, StatusCode};

    let server = LithairServer::new()
        .with_route_async(Method::PUT, "/limited", |req: RouteRequest| async move {
            let bytes = request::read_body_with_limit(req, 1024).await?;
            Ok(response::text(StatusCode::OK, format!("read {} bytes", bytes.len())))
        })
        .build()
        .expect("build server");
    let (base, handle) = spawn_for_test(server).await;

    let client = reqwest::Client::new();
    let resp = client
        .put(format!("{}/limited", base))
        .body("small payload")
        .send()
        .await
        .expect("PUT /limited");
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.expect("body"), "read 13 bytes");

    handle.abort();
}

// ------------------------------------------------------------------
// Per-model stats parallelism on `/metrics` (Gemini PR #83 round-3).
//
// Regression test for the sequential `.await` loop that previously
// computed each model's stats one after the other. With N models
// and a per-model latency of `SLEEP`, the old wall-clock cost was
// N * SLEEP. After parallelising via `futures::future::join_all`
// the cost should be roughly SLEEP (max, not sum). We bound the
// assertion at 2 * SLEEP so a true sequential regression (N=5,
// 5 * SLEEP) trips it while CI scheduler jitter stays in the green.
// ------------------------------------------------------------------

/// Test-only `ModelHandler` that sleeps for a configurable duration in
/// `get_stats` and returns trivial values elsewhere. Used to prove that
/// `handle_metrics_request` collects per-model stats concurrently — only
/// `get_stats` is invoked by `/metrics`, so the other trait methods
/// stay as minimal stubs that panic if accidentally called (which would
/// indicate the test wiring drifted from the metrics endpoint).
struct SlowStatsHandler {
    name: String,
    sleep: std::time::Duration,
}

#[async_trait::async_trait]
impl crate::app::ModelHandler for SlowStatsHandler {
    async fn handle_request(
        &self,
        _req: hyper::Request<hyper::body::Incoming>,
        _path_segments: &[&str],
    ) -> Result<
        hyper::Response<
            http_body_util::combinators::BoxBody<bytes::Bytes, std::convert::Infallible>,
        >,
        std::convert::Infallible,
    > {
        unreachable!("SlowStatsHandler::handle_request must not be called by /metrics");
    }

    async fn get_all_data_json(&self) -> serde_json::Value {
        serde_json::Value::Array(vec![])
    }

    async fn get_item_json(&self, _id: &str) -> Option<serde_json::Value> {
        None
    }

    async fn get_count(&self) -> usize {
        0
    }

    async fn export_json(&self) -> serde_json::Value {
        serde_json::json!({})
    }

    async fn get_stats(&self, _data_path: &str) -> crate::app::ModelStats {
        // Deliberate latency. join_all should fire all of these at once,
        // so total wall-clock time stays ~= `self.sleep`, not N * sleep.
        tokio::time::sleep(self.sleep).await;
        crate::app::ModelStats {
            model: self.name.clone(),
            item_count: 0,
            approx_ram_bytes: 0,
            raftlog_size_bytes: 0,
            events_since_last_compaction: None,
            last_compaction_at: None,
        }
    }

    fn model_name(&self) -> &str {
        &self.name
    }

    fn base_path(&self) -> &str {
        "/test"
    }

    async fn get_entity_history(&self, _id: &str) -> serde_json::Value {
        serde_json::Value::Array(vec![])
    }

    async fn get_entity_event_count(&self, _id: &str) -> usize {
        0
    }

    async fn submit_edit_event(
        &self,
        _id: &str,
        _changes: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        Err("not implemented in test handler".to_string())
    }

    async fn apply_replicated_item_json(
        &self,
        _item_json: serde_json::Value,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn apply_replicated_items_json(
        &self,
        _items_json: Vec<serde_json::Value>,
    ) -> Result<usize, String> {
        Ok(0)
    }

    async fn apply_replicated_update_json(
        &self,
        _id: &str,
        _item_json: serde_json::Value,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn apply_replicated_delete_json(&self, _id: &str) -> Result<bool, String> {
        Ok(false)
    }
}

#[tokio::test]
async fn metrics_endpoint_collects_per_model_stats_concurrently() {
    // 5 models × 200 ms sleep. Sequential would take ~1000 ms; parallel
    // should land near 200 ms. We assert < 400 ms (2× SLEEP) so the test
    // doesn't false-fail on slow CI runners while still catching a true
    // sequential regression (which would be ≥5× SLEEP = 1000 ms).
    const N_MODELS: usize = 5;
    const SLEEP: std::time::Duration = std::time::Duration::from_millis(200);
    let upper_bound = SLEEP * 2;

    let server = LithairServer::new().build().expect("build server");

    // Inject N SlowStatsHandler instances directly into the models
    // registry. `models` is a private field but accessible from this
    // child test module (same crate, parent `app` module).
    {
        let mut models = server.models.write().await;
        for i in 0..N_MODELS {
            models.push(ModelRegistration {
                name: format!("SlowModel{}", i),
                base_path: format!("/test/{}", i),
                data_path: "/tmp/lithair-slowmodel-test".to_string(),
                handler: Arc::new(SlowStatsHandler {
                    name: format!("SlowModel{}", i),
                    sleep: SLEEP,
                }),
                schema_extractor: None,
            });
        }
    }

    // Round-trip through the real HTTP stack via spawn_for_test rather
    // than calling `handle_metrics_request` directly — the handler takes
    // `Request<hyper::body::Incoming>` and building an `Incoming` body
    // outside hyper's connection state is non-trivial. The HTTP path is
    // also what production code exercises, so timing it is the right
    // proxy for the real cost.
    let (base, handle) = spawn_for_test(server).await;

    let start = std::time::Instant::now();
    let resp = reqwest::get(format!("{}/metrics", base)).await.expect("GET /metrics succeeded");
    let elapsed = start.elapsed();
    assert_eq!(resp.status(), 200, "/metrics should return 200");

    // Body sanity check: each slow model must appear in the output. This
    // protects against the test silently passing if the loop were
    // skipped entirely (e.g. early-return on empty models).
    let body = resp.text().await.expect("read body");
    for i in 0..N_MODELS {
        let needle = format!(r#"lithair_model_items{{model="SlowModel{}"}}"#, i);
        assert!(
            body.contains(&needle),
            "metrics body missing per-model series for SlowModel{}: body was {}",
            i,
            body
        );
    }

    assert!(
        elapsed < upper_bound,
        "metrics collection took {:?}, expected < {:?} (sequential regression: \
         {} models × {:?} = {:?} would exceed this)",
        elapsed,
        upper_bound,
        N_MODELS,
        SLEEP,
        SLEEP * N_MODELS as u32
    );

    handle.abort();
}
