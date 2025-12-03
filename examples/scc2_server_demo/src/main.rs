use std::{net::SocketAddr, sync::Arc};

use bytes::Bytes;
use clap::Parser;
use flate2::write::GzEncoder;
use flate2::Compression;
use http_body_util::{BodyExt, Full};
use hyper::Method;
use hyper::{body::Incoming, http::Response, http::StatusCode, Request};
use hyper_util::rt::TokioExecutor;
use hyper_util::server::conn::auto::Builder;
use scc::HashMap as SccHashMap;
use serde::Deserialize;
use tokio::net::TcpListener;

#[derive(Parser, Debug, Clone)]
#[command(name = "scc2_server_demo", about = "Hyper + SCC2 high-performance stateless and KV demo")]
struct Args {
    /// Port to bind, e.g. 18321
    #[arg(long, default_value_t = 18321)]
    port: u16,

    /// Bind address
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
}

fn accepts_gzip(req: &Request<Incoming>) -> bool {
    if let Some(val) = req.headers().get("accept-encoding") {
        if let Ok(s) = val.to_str() {
            return s.to_ascii_lowercase().contains("gzip");
        }
    }
    false
}

fn gzip_bytes(input: &[u8]) -> Vec<u8> {
    let mut enc = GzEncoder::new(Vec::with_capacity(input.len() / 2 + 64), Compression::fast());
    use std::io::Write;
    let _ = enc.write_all(input);
    enc.finish().unwrap_or_default()
}

fn respond_bytes_gzip(want_gzip: bool, buf: Bytes, content_type: &str) -> Response<Full<Bytes>> {
    let mut builder = Response::builder().status(StatusCode::OK);
    builder = builder.header("content-type", content_type).header("vary", "accept-encoding");
    if want_gzip {
        let gz = gzip_bytes(&buf);
        builder
            .header("content-encoding", "gzip")
            .body(Full::new(Bytes::from(gz)))
            .unwrap()
    } else {
        builder.body(Full::new(buf)).unwrap()
    }
}

fn respond_bytes(req: &Request<Incoming>, buf: Bytes, content_type: &str) -> Response<Full<Bytes>> {
    let want_gzip = accepts_gzip(req);
    respond_bytes_gzip(want_gzip, buf, content_type)
}

fn parse_query(req: &Request<Incoming>) -> std::collections::HashMap<&str, &str> {
    let mut out = std::collections::HashMap::new();
    if let Some(q) = req.uri().query() {
        for pair in q.split('&') {
            if pair.is_empty() {
                continue;
            }
            let mut it = pair.splitn(2, '=');
            let k = it.next().unwrap_or("");
            let v = it.next().unwrap_or("");
            out.insert(k, v);
        }
    }
    out
}

fn ok_text(req: &Request<Incoming>, body: &str) -> Response<Full<Bytes>> {
    respond_bytes(req, Bytes::from(body.to_owned()), "text/plain; charset=utf-8")
}

async fn handle(
    req: Request<Incoming>,
    kv: Arc<SccHashMap<String, Bytes>>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let method = req.method().clone();
    let path = req.uri().path().to_string();

    // Stateless perf endpoints
    match (method.clone(), path.as_str()) {
        (Method::GET, "/status") => {
            return Ok(ok_text(&req, "OK"));
        }
        (Method::GET, "/perf/json") => {
            let q = parse_query(&req);
            let n = q.get("bytes").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1024);
            let force = q.get("gzip").and_then(|v| match *v {
                "1" => Some(true),
                "0" => Some(false),
                _ => None,
            });
            let want = force.unwrap_or_else(|| accepts_gzip(&req));
            // Build JSON body
            let s = format!("{{\"data\":\"{}\"}}", "x".repeat(n));
            return Ok(respond_bytes_gzip(want, Bytes::from(s), "application/json"));
        }
        (Method::GET, "/perf/bytes") => {
            let q = parse_query(&req);
            let n = q.get("n").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1024);
            let force = q.get("gzip").and_then(|v| match *v {
                "1" => Some(true),
                "0" => Some(false),
                _ => None,
            });
            let want = force.unwrap_or_else(|| accepts_gzip(&req));
            let buf = vec![b'x'; n];
            return Ok(respond_bytes_gzip(want, Bytes::from(buf), "application/octet-stream"));
        }
        (Method::POST, "/perf/echo") => {
            // Read whole body and echo back with optional gzip
            let want_gzip = accepts_gzip(&req);
            let body = req.into_body().collect().await.map(|c| c.to_bytes()).unwrap_or_default();
            return Ok(respond_bytes_gzip(want_gzip, body, "application/octet-stream"));
        }
        _ => {}
    }

    // SCC2 KV endpoints
    if method == Method::POST && path == "/scc2/put" {
        let q = parse_query(&req);
        let key = q.get("key").copied().unwrap_or("k");
        let n = q.get("n").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1024);
        let val = Bytes::from(vec![b'x'; n]);
        let _ = kv.insert_async(key.to_string(), val).await;
        return Ok(ok_text(&req, "OK"));
    }

    if method == Method::GET && path == "/scc2/get" {
        let q = parse_query(&req);
        let key = q.get("key").copied().unwrap_or("k");
        if let Some(entry) = kv.get_async(key).await {
            let len = entry.get().len();
            return Ok(ok_text(&req, &format!("LEN={}", len)));
        } else {
            return Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Full::new(Bytes::from_static(b"NOT_FOUND")))
                .unwrap());
        }
    }

    // =============================
    // SCC2 BULK ENDPOINTS (JSON)
    // =============================
    #[derive(Deserialize)]
    struct PutItem {
        key: String,
        #[serde(default)]
        n: Option<usize>,
        #[serde(default)]
        value: Option<String>,
    }

    if method == Method::POST && path == "/scc2/put_bulk" {
        let want_gzip = accepts_gzip(&req);
        let body = req.into_body().collect().await.map(|c| c.to_bytes()).unwrap_or_default();
        let items: Vec<PutItem> = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(_) => {
                let resp = Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Full::new(Bytes::from_static(b"invalid json")))
                    .unwrap();
                return Ok(resp);
            }
        };
        let mut count = 0usize;
        for it in items {
            let data = if let Some(v) = it.value {
                Bytes::from(v)
            } else {
                let n = it.n.unwrap_or(1024);
                Bytes::from(vec![b'x'; n])
            };
            let _ = kv.insert_async(it.key, data).await;
            count += 1;
        }
        let json = format!("{{\"status\":\"ok\",\"count\":{}}}", count);
        return Ok(respond_bytes_gzip(want_gzip, Bytes::from(json), "application/json"));
    }

    if method == Method::POST && path == "/scc2/get_bulk" {
        let want_gzip = accepts_gzip(&req);
        let body = req.into_body().collect().await.map(|c| c.to_bytes()).unwrap_or_default();
        let keys: Vec<String> = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(_) => {
                let resp = Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Full::new(Bytes::from_static(b"invalid json")))
                    .unwrap();
                return Ok(resp);
            }
        };
        let mut out = Vec::with_capacity(keys.len());
        for k in keys {
            if let Some(entry) = kv.get_async(&k).await {
                let len = entry.get().len();
                out.push(serde_json::json!({"key": k, "found": true, "len": len}));
            } else {
                out.push(serde_json::json!({"key": k, "found": false}));
            }
        }
        let json = serde_json::to_vec(&out).unwrap_or_else(|_| b"[]".to_vec());
        return Ok(respond_bytes_gzip(want_gzip, Bytes::from(json), "application/json"));
    }

    // 404
    Ok(Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header("content-type", "text/plain; charset=utf-8")
        .body(Full::new(Bytes::from_static(b"not found")))
        .unwrap())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let addr: SocketAddr = format!("{}:{}", args.host, args.port).parse()?;
    let listener = TcpListener::bind(addr).await?;
    println!("🚀 SCC2 server demo listening on http://{}", addr);

    let kv: Arc<SccHashMap<String, Bytes>> = Arc::new(SccHashMap::new());

    loop {
        let (stream, _peer) = listener.accept().await?;
        let kv = kv.clone();
        tokio::spawn(async move {
            let io = hyper_util::rt::TokioIo::new(stream);
            let service = hyper::service::service_fn(move |req| {
                let kv = kv.clone();
                async move { handle(req, kv).await }
            });
            if let Err(err) = Builder::new(TokioExecutor::new()).serve_connection(io, service).await
            {
                // Under heavy load, clients may close connections early or reset them,
                // which results in benign write errors. Suppress the noisy variants.
                let msg = err.to_string();
                if !(msg.contains("error writing a body to connection")
                    || msg.contains("broken pipe")
                    || msg.contains("connection reset")
                    || msg.contains("connection closed"))
                {
                    eprintln!("connection error: {msg}");
                }
            }
        });
    }
}
