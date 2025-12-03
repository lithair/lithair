//! Async HTTP server using Hyper
//!
//! Full async server implementation for Lithair with native Hyper support

use bytes::Bytes;
use http_body_util::Full;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{body::Incoming, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;

use super::{HttpRequest as LithairRequest, HttpResponse as LithairResponse, Router};

/// Async HTTP server using Hyper
pub struct AsyncHttpServer<S = ()> {
    router: Arc<Router<S>>,
    state: Arc<S>,
}

impl<S: Send + Sync + 'static> AsyncHttpServer<S> {
    /// Create a new async server with a router and state
    pub fn new(router: Router<S>, state: S) -> Self {
        Self { router: Arc::new(router), state: Arc::new(state) }
    }

    /// Start the server on the given address
    pub async fn serve(self, addr: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let addr: SocketAddr = addr.parse()?;
        let listener = TcpListener::bind(addr).await?;

        println!("🚀 Async HTTP server listening on {}", addr);

        loop {
            let (stream, _) = listener.accept().await?;
            let io = TokioIo::new(stream);

            let router = self.router.clone();
            let state = self.state.clone();

            tokio::task::spawn(async move {
                if let Err(err) = http1::Builder::new()
                    .serve_connection(
                        io,
                        service_fn(move |req| {
                            let router = router.clone();
                            let state = state.clone();
                            async move { handle_request(req, router, state).await }
                        }),
                    )
                    .await
                {
                    eprintln!("Error serving connection: {:?}", err);
                }
            });
        }
    }
}

/// Convert Hyper request to Lithair request and handle it
async fn handle_request<S>(
    req: Request<Incoming>,
    router: Arc<Router<S>>,
    state: Arc<S>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    // Convert Hyper request to Lithair request
    let lithair_req = match convert_request(req).await {
        Ok(r) => r,
        Err(e) => {
            let error_response = LithairResponse::internal_server_error()
                .text(&format!("Failed to parse request: {}", e));
            return Ok(convert_response(&error_response));
        }
    };

    // Handle with async router
    let lithair_resp = router.handle_request_async(&lithair_req, &*state).await;

    // Convert Lithair response to Hyper response
    Ok(convert_response(&lithair_resp))
}

/// Convert Hyper request to Lithair request
async fn convert_request(
    req: Request<Incoming>,
) -> Result<LithairRequest, Box<dyn std::error::Error + Send + Sync>> {
    use http_body_util::BodyExt;

    let method = match req.method() {
        &hyper::Method::GET => super::HttpMethod::GET,
        &hyper::Method::POST => super::HttpMethod::POST,
        &hyper::Method::PUT => super::HttpMethod::PUT,
        &hyper::Method::DELETE => super::HttpMethod::DELETE,
        &hyper::Method::PATCH => super::HttpMethod::PATCH,
        &hyper::Method::HEAD => super::HttpMethod::HEAD,
        &hyper::Method::OPTIONS => super::HttpMethod::OPTIONS,
        _ => super::HttpMethod::GET,
    };

    let uri = req.uri();
    let path = uri.path().to_string();
    let _query = uri.query().unwrap_or("").to_string();

    // Read body
    let whole_body = req.collect().await?.to_bytes();
    let body = String::from_utf8_lossy(&whole_body).to_string();

    // Build Lithair request
    let raw_request = format!("{} {} HTTP/1.1\r\n\r\n{}", method.as_str(), path, body);

    LithairRequest::parse(raw_request.as_bytes()).map_err(|e| e.into())
}

/// Convert Lithair response to Hyper response
fn convert_response(resp: &LithairResponse) -> Response<Full<Bytes>> {
    let status = match resp.status() {
        super::StatusCode::Ok => StatusCode::OK,
        super::StatusCode::Created => StatusCode::CREATED,
        super::StatusCode::NoContent => StatusCode::NO_CONTENT,
        super::StatusCode::BadRequest => StatusCode::BAD_REQUEST,
        super::StatusCode::Unauthorized => StatusCode::UNAUTHORIZED,
        super::StatusCode::Forbidden => StatusCode::FORBIDDEN,
        super::StatusCode::NotFound => StatusCode::NOT_FOUND,
        super::StatusCode::InternalServerError => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::OK,
    };

    let body = resp.body_bytes().to_vec();

    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap()
}
