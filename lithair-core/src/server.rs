use std::sync::Arc;
use std::convert::Infallible;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper::body::Incoming;
use http_body_util::{BodyExt, Full};
use http_body_util::combinators::BoxBody;
use bytes::Bytes;
use hyper_util::server::conn::auto::Builder as AutoBuilder;
use hyper_util::rt::TokioExecutor;
use tokio::net::TcpListener;
use serde::{Deserialize, Serialize};

use crate::raft::{MemStore, Request as RaftRequest, SimpleRaft};

#[derive(Debug, Serialize, Deserialize)]
pub struct SetRequest {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetResponse {
    pub value: Option<String>,
}

#[derive(Clone)]
pub struct AppState {
    pub raft: Arc<SimpleRaft>,
}

/// Lithair HTTP Server with Hyper
pub struct LithairServer {
    state: AppState,
}

impl LithairServer {
    pub fn new(raft: Arc<SimpleRaft>) -> Self {
        Self {
            state: AppState { raft },
        }
    }

    pub async fn serve(self, addr: std::net::SocketAddr) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let state = Arc::new(self.state);
        println!("🚀 Lithair HTTP Server listening on http://{}", addr);

        let listener = TcpListener::bind(addr).await?;
        loop {
            let (stream, _) = listener.accept().await?;
            let state = Arc::clone(&state);
            tokio::spawn(async move {
                let service = service_fn(move |req: Req| {
                    let state = Arc::clone(&state);
                    handle_request(req, state)
                });
                if let Err(e) = AutoBuilder::new(TokioExecutor::new())
                    .serve_connection(stream, service)
                    .await
                {
                    eprintln!("Server connection error: {}", e);
                }
            });
        }
    }
}

type RespBody = BoxBody<Bytes, Infallible>;
type Req = Request<Incoming>;
type Resp = Response<RespBody>;

#[inline]
fn body_from<T: Into<Bytes>>(data: T) -> RespBody {
    Full::new(data.into()).map_err(|_| Infallible).boxed()
}

async fn handle_request(
    req: Req,
    state: Arc<AppState>,
) -> Result<Resp, Infallible> {
    let method = req.method();
    let path = req.uri().path();

    match (method, path) {
        (&Method::GET, "/") => {
            handle_hello_world(state).await
        }
        (&Method::GET, path) if path.starts_with("/get/") => {
            let key = path.strip_prefix("/get/").unwrap_or("");
            handle_get_value(key, state).await
        }
        (&Method::POST, "/set") => {
            handle_set_value(req, state).await
        }
        _ => {
            Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(body_from("Not Found"))
                .unwrap())
        }
    }
}

async fn handle_hello_world(state: Arc<AppState>) -> Result<Resp, Infallible> {
    let store = state.raft.get_store();
    let sm = store.state_machine.lock().await;
    
    let value = sm.data.get("hello").cloned().unwrap_or_else(|| "hello world".to_string());
    
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/plain")
        .body(body_from(value))
        .unwrap())
}

async fn handle_get_value(key: &str, state: Arc<AppState>) -> Result<Resp, Infallible> {
    let store = state.raft.get_store();
    let sm = store.state_machine.lock().await;
    let value = sm.data.get(key).cloned();
    
    let response = GetResponse { value };
    let json = serde_json::to_string(&response).unwrap_or_else(|_| "{}".to_string());
    
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(body_from(json))
        .unwrap())
}

async fn handle_set_value(
    req: Req,
    state: Arc<AppState>,
) -> Result<Resp, Infallible> {
    // Parse JSON body
    let body_bytes = match req.into_body().collect().await.map(|c| c.to_bytes()) {
        Ok(bytes) => bytes,
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(body_from("Invalid body"))
                .unwrap());
        }
    };
    
    let set_request: SetRequest = match serde_json::from_slice(&body_bytes) {
        Ok(req) => req,
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(body_from("Invalid JSON"))
                .unwrap());
        }
    };
    
    let request = RaftRequest {
        key: set_request.key.clone(),
        value: set_request.value,
    };
    
    match state.raft.client_write(request).await {
        Ok(_) => {
            let response = format!("{{\"status\":\"ok\",\"key\":\"{}\"}}", set_request.key);
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(body_from(response))
                .unwrap())
        }
        Err(_) => {
            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(body_from("{\"error\":\"Failed to set value\"}"))
                .unwrap())
        }
    }
}

pub async fn start_server(port: u16) -> Result<(), Box<dyn std::error::Error>> {
    // Initialize Raft
    let store = Arc::new(MemStore::new());
    let raft = Arc::new(SimpleRaft::new(store.clone()));
    
    // Initialize the cluster (simplified)
    let mut nodes = std::collections::BTreeMap::new();
    nodes.insert(1u64, openraft::BasicNode::new("127.0.0.1:8080"));
    raft.initialize(nodes).await?;
    
    // Initialize the database with "hello world"
    let init_request = RaftRequest {
        key: "hello".to_string(),
        value: "hello world".to_string(),
    };
    let _ = raft.client_write(init_request).await;
    
    // Start HTTP server
    let addr = ([127, 0, 0, 1], port).into();
    let server = LithairServer::new(raft);
    server.serve(addr).await
}