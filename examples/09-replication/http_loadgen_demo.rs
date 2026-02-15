use clap::Parser;
use rand::prelude::IndexedRandom;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT_ENCODING};
use reqwest::{Client, ClientBuilder};
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex, Semaphore};
use tokio::time::Duration;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "http_loadgen_demo",
    about = "High-performance HTTP load generator for Lithair demo"
)]
struct Args {
    /// Leader base URL, e.g. http://127.0.0.1:8080
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    leader: String,

    /// Total operations to perform (CREATE operations)
    #[arg(long, default_value_t = 10000)]
    total: usize,

    /// Concurrency (number of in-flight requests)
    #[arg(long, default_value_t = 512)]
    concurrency: usize,

    /// Bulk size for /_bulk endpoint (items per request)
    #[arg(long, default_value_t = 100)]
    bulk_size: usize,

    /// Mode: single or bulk
    #[arg(long, default_value = "bulk")]
    mode: String,

    /// Percentage of CREATE operations (for mode=random)
    #[arg(long, default_value_t = 80)]
    create_pct: u32,

    /// Percentage of READ operations (for mode=random)
    #[arg(long, default_value_t = 15)]
    read_pct: u32,

    /// Percentage of UPDATE operations (for mode=random)
    #[arg(long, default_value_t = 5)]
    update_pct: u32,

    /// Percentage of DELETE operations (for mode=random) - reserved
    #[arg(long, default_value_t = 0)]
    delete_pct: u32,

    /// Comma-separated read target base URLs (e.g. http://127.0.0.1:8080,http://127.0.0.1:8081)
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    read_targets: String,

    /// Path to request for READ operations (for mode=random). Example: /api/products or /status
    #[arg(long, default_value = "/api/products")]
    read_path: String,

    /// Timeout in seconds per HTTP request
    #[arg(long, default_value_t = 10)]
    timeout_s: u64,

    /// Stateless perf path (for perf-* modes), e.g. /status, /perf/json, /perf/bytes, /perf/echo
    #[arg(long, default_value = "/status")]
    perf_path: String,

    /// Payload bytes (for perf-* modes). For perf-json/bytes: size of payload. For perf-echo: POST body size.
    #[arg(long, default_value_t = 1024)]
    perf_bytes: usize,

    /// Optional Accept-Encoding header to send with requests (e.g. "gzip")
    #[arg(long, default_value = "")]
    accept_encoding: String,
}

struct Metrics {
    create_ms: Arc<Mutex<Vec<f64>>>,
    read_ms: Arc<Mutex<Vec<f64>>>,
    update_ms: Arc<Mutex<Vec<f64>>>,
    delete_ms: Arc<Mutex<Vec<f64>>>,
}

impl Metrics {
    fn new() -> Self {
        Self {
            create_ms: Arc::new(Mutex::new(Vec::with_capacity(4096))),
            read_ms: Arc::new(Mutex::new(Vec::with_capacity(4096))),
            update_ms: Arc::new(Mutex::new(Vec::with_capacity(4096))),
            delete_ms: Arc::new(Mutex::new(Vec::with_capacity(4096))),
        }
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let n = sorted.len();
    let rank = p.clamp(0.0, 1.0) * (n as f64 - 1.0);
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    if lo == hi {
        sorted[lo]
    } else {
        let w = rank - lo as f64;
        sorted[lo] * (1.0 - w) + sorted[hi] * w
    }
}

fn summarize(label: &str, mut samples: Vec<f64>) {
    if samples.is_empty() {
        return;
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let count = samples.len();
    let mean = samples.iter().copied().sum::<f64>() / count as f64;
    let p50 = percentile(&samples, 0.50);
    let p95 = percentile(&samples, 0.95);
    let p99 = percentile(&samples, 0.99);
    println!(
        "  {:>8} (count={}): p50={:.2}ms p95={:.2}ms p99={:.2}ms mean={:.2}ms",
        label, count, p50, p95, p99, mean
    );
}

fn build_client(timeout_s: u64, accept_encoding: &str) -> Client {
    let mut builder = ClientBuilder::new()
        .pool_idle_timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(256)
        .tcp_keepalive(Duration::from_secs(30))
        .http1_only()
        .tcp_nodelay(true)
        .timeout(Duration::from_secs(timeout_s));

    if !accept_encoding.trim().is_empty() {
        let mut headers = HeaderMap::new();
        let hv = HeaderValue::from_str(accept_encoding).unwrap_or(HeaderValue::from_static("gzip"));
        headers.insert(ACCEPT_ENCODING, hv);
        builder = builder.default_headers(headers);
    }

    builder.build().expect("failed to build client")
}

fn random_category<R: rand::Rng + ?Sized>(rng: &mut R) -> &'static str {
    const CATS: &[&str] = &[
        "Electronics",
        "Books",
        "Clothing",
        "Sports",
        "Home",
        "Beauty",
        "Toys",
        "Games",
        "Health",
        "Automotive",
    ];
    CATS.choose(rng).copied().unwrap_or("Misc")
}

fn make_product_payloads(start_idx: usize, count: usize) -> Vec<serde_json::Value> {
    let mut rng = rand::rng();
    (0..count)
        .map(|i| {
            let name = format!("lg_{}_{}", start_idx + i, rand::random::<u32>());
            let price: f64 = (rand::random_range(100..5000) as f64) + 0.99;
            let category = random_category(&mut rng);
            json!({
                "name": name,
                "price": price,
                "category": category,
            })
        })
        .collect()
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let client = build_client(args.timeout_s, &args.accept_encoding);
    let read_targets: Vec<String> = args
        .read_targets
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string())
        .collect();

    let start = Instant::now();
    let sem = Arc::new(Semaphore::new(args.concurrency));
    let mut handles = Vec::new();
    let metrics = Metrics::new();

    match args.mode.as_str() {
        "perf-status" => {
            for _ in 0..args.total {
                let permit = sem.clone().acquire_owned().await.unwrap();
                let client = client.clone();
                let url = format!("{}{}", args.leader, args.perf_path);
                let m_read = metrics.read_ms.clone();
                handles.push(tokio::spawn(async move {
                    let _p = permit;
                    let t0 = Instant::now();
                    let _ = client.get(&url).send().await;
                    let ms = t0.elapsed().as_secs_f64() * 1000.0;
                    let mut v = m_read.lock().await;
                    v.push(ms);
                }));
            }
        }
        "perf-json" => {
            let query = format!("{}?bytes={}", args.perf_path, args.perf_bytes);
            for _ in 0..args.total {
                let permit = sem.clone().acquire_owned().await.unwrap();
                let client = client.clone();
                let url = format!("{}{}", args.leader, query);
                let m_read = metrics.read_ms.clone();
                handles.push(tokio::spawn(async move {
                    let _p = permit;
                    let t0 = Instant::now();
                    let _ = client.get(&url).send().await;
                    let ms = t0.elapsed().as_secs_f64() * 1000.0;
                    let mut v = m_read.lock().await;
                    v.push(ms);
                }));
            }
        }
        "perf-bytes" => {
            let query = format!("{}?n={}", args.perf_path, args.perf_bytes);
            for _ in 0..args.total {
                let permit = sem.clone().acquire_owned().await.unwrap();
                let client = client.clone();
                let url = format!("{}{}", args.leader, query);
                let m_read = metrics.read_ms.clone();
                handles.push(tokio::spawn(async move {
                    let _p = permit;
                    let t0 = Instant::now();
                    let _ = client.get(&url).send().await;
                    let ms = t0.elapsed().as_secs_f64() * 1000.0;
                    let mut v = m_read.lock().await;
                    v.push(ms);
                }));
            }
        }
        "perf-echo" => {
            // POST body of perf_bytes 'x'
            let body = "x".repeat(args.perf_bytes);
            for _ in 0..args.total {
                let permit = sem.clone().acquire_owned().await.unwrap();
                let client = client.clone();
                let url = format!("{}{}", args.leader, args.perf_path);
                let m_update = metrics.update_ms.clone();
                let body_clone = body.clone();
                handles.push(tokio::spawn(async move {
                    let _p = permit;
                    let t0 = Instant::now();
                    let _ = client.post(&url).body(body_clone.clone()).send().await;
                    let ms = t0.elapsed().as_secs_f64() * 1000.0;
                    let mut v = m_update.lock().await;
                    v.push(ms);
                }));
            }
        }
        "single" => {
            for i in 0..args.total {
                let permit = sem.clone().acquire_owned().await.unwrap();
                let client = client.clone();
                let url = format!("{}/api/products", args.leader);
                let m_create = metrics.create_ms.clone();
                handles.push(tokio::spawn(async move {
                    let _p = permit;
                    let t0 = Instant::now();
                    let payloads = make_product_payloads(i, 1);
                    let body = &payloads[0];
                    let _ = client.post(&url).json(body).send().await;
                    let ms = t0.elapsed().as_secs_f64() * 1000.0;
                    let mut v = m_create.lock().await;
                    v.push(ms);
                }));
            }
        }
        "random" => {
            // Mix of CREATE/READ/UPDATE based on percentages
            let c = args.create_pct.min(100);
            let r = args.read_pct.min(100);
            let u = args.update_pct.min(100);
            let d = args.delete_pct.min(100);
            let total_pct = c + r + u + d;
            let c_norm = c as f64 / total_pct.max(1) as f64;
            let r_norm = r as f64 / total_pct.max(1) as f64;
            let u_norm = u as f64 / total_pct.max(1) as f64;
            // let d_norm = d as f64 / total_pct.max(1) as f64;

            // Shared pool of IDs captured from CREATE responses to speed up UPDATE
            let ids: Arc<Mutex<Vec<String>>> =
                Arc::new(Mutex::new(Vec::with_capacity(args.total.min(10_000))));
            let read_path = args.read_path.clone();

            for i in 0..args.total {
                let permit = sem.clone().acquire_owned().await.unwrap();
                let client = client.clone();
                let leader = args.leader.clone();
                let read_targets = read_targets.clone();
                let ids = ids.clone();
                let read_path = read_path.clone();
                let m_create = metrics.create_ms.clone();
                let m_read = metrics.read_ms.clone();
                let m_update = metrics.update_ms.clone();
                let m_delete = metrics.delete_ms.clone();
                handles.push(tokio::spawn(async move {
                    let _p = permit;
                    // Randomly choose operation
                    let x: f64 = rand::random();
                    if x < c_norm {
                        // CREATE (single)
                        let url = format!("{}/api/products", leader);
                        let t0 = Instant::now();
                        let payloads = make_product_payloads(i, 1);
                        let body = &payloads[0];
                        if let Ok(resp) = client.post(&url).json(body).send().await {
                            if let Ok(v) = resp.json::<serde_json::Value>().await {
                                if let Some(id) = v.get("id").and_then(|vv| vv.as_str()) {
                                    let mut guard = ids.lock().await;
                                    guard.push(id.to_string());
                                }
                            }
                        }
                        let ms = t0.elapsed().as_secs_f64() * 1000.0;
                        let mut v = m_create.lock().await;
                        v.push(ms);
                    } else if x < c_norm + r_norm {
                        // READ (GET on a random node)
                        let tgt = if !read_targets.is_empty() {
                            read_targets.choose(&mut rand::rng()).cloned().unwrap_or(leader.clone())
                        } else {
                            leader.clone()
                        };
                        let mut path = String::new();
                        path.push_str(&read_path);
                        if !path.starts_with('/') {
                            path.insert(0, '/');
                        }
                        let url = format!("{}{}", tgt, path);
                        let t0 = Instant::now();
                        let _ = client.get(&url).send().await;
                        let ms = t0.elapsed().as_secs_f64() * 1000.0;
                        let mut v = m_read.lock().await;
                        v.push(ms);
                    } else if x < c_norm + r_norm + u_norm {
                        // UPDATE: prefer an ID from the pool; if empty, try lightweight random-id endpoint
                        let maybe_id = {
                            let guard = ids.lock().await;
                            if guard.is_empty() {
                                None
                            } else {
                                let idx = rand::random_range(0..guard.len());
                                guard.get(idx).cloned()
                            }
                        };
                        let id_to_use = if let Some(id) = maybe_id {
                            Some(id)
                        } else {
                            // Fetch a random existing id (lightweight)
                            let url = format!("{}/api/products/random-id", leader);
                            match client.get(&url).send().await {
                                Ok(resp) => {
                                    if resp.status().is_success() {
                                        match resp.json::<serde_json::Value>().await {
                                            Ok(val) => val
                                                .get("id")
                                                .and_then(|v| v.as_str())
                                                .map(|s| s.to_string()),
                                            Err(_) => None,
                                        }
                                    } else {
                                        None
                                    }
                                }
                                Err(_) => None,
                            }
                        };
                        if let Some(id) = id_to_use {
                            let url = format!("{}/api/products/{}", leader, id);
                            let t0 = Instant::now();
                            let payloads = make_product_payloads(i, 1);
                            let body = &payloads[0];
                            let _ = client.put(&url).json(body).send().await;
                            let ms = t0.elapsed().as_secs_f64() * 1000.0;
                            let mut v = m_update.lock().await;
                            v.push(ms);
                        } else {
                            // Still no id available; skip to keep workload lightweight
                        }
                    } else {
                        // DELETE: pick an ID (pool or lightweight endpoint) and delete it
                        let maybe_id = {
                            let guard = ids.lock().await;
                            if guard.is_empty() {
                                None
                            } else {
                                let idx = rand::random_range(0..guard.len());
                                guard.get(idx).cloned()
                            }
                        };
                        let id_to_use = if let Some(id) = maybe_id {
                            Some(id)
                        } else {
                            // Fetch a random existing id (lightweight)
                            let url = format!("{}/api/products/random-id", leader);
                            match client.get(&url).send().await {
                                Ok(resp) => {
                                    if resp.status().is_success() {
                                        match resp.json::<serde_json::Value>().await {
                                            Ok(val) => val
                                                .get("id")
                                                .and_then(|v| v.as_str())
                                                .map(|s| s.to_string()),
                                            Err(_) => None,
                                        }
                                    } else {
                                        None
                                    }
                                }
                                Err(_) => None,
                            }
                        };
                        if let Some(id) = id_to_use {
                            let url = format!("{}/api/products/{}", leader, id);
                            let t0 = Instant::now();
                            if let Ok(resp) = client.delete(&url).send().await {
                                if resp.status().is_success() {
                                    // Remove the id from the pool to avoid repeated deletes
                                    let mut guard = ids.lock().await;
                                    if let Some(pos) = guard.iter().position(|x| x == &id) {
                                        guard.swap_remove(pos);
                                    }
                                    let ms = t0.elapsed().as_secs_f64() * 1000.0;
                                    let mut v = m_delete.lock().await;
                                    v.push(ms);
                                }
                            }
                        } else {
                            // No id to delete; skip to keep workload lightweight
                        }
                    }
                }));
            }
        }
        _ => {
            // bulk
            let bulk = args.bulk_size.max(1);
            let mut sent = 0usize;
            while sent < args.total {
                let this = bulk.min(args.total - sent);
                let offset = sent;
                sent += this;

                let permit = sem.clone().acquire_owned().await.unwrap();
                let client = client.clone();
                let url = format!("{}/api/products/_bulk", args.leader);
                let m_create = metrics.create_ms.clone();
                handles.push(tokio::spawn(async move {
                    let _p = permit;
                    let t0 = Instant::now();
                    let payloads = make_product_payloads(offset, this);
                    let _ = client.post(&url).json(&payloads).send().await;
                    let ms = t0.elapsed().as_secs_f64() * 1000.0;
                    let mut v = m_create.lock().await;
                    v.push(ms);
                }));
            }
        }
    }

    for h in handles {
        let _ = h.await;
    }

    let dur = start.elapsed();
    let eps = (args.total as f64) / dur.as_secs_f64();
    println!("\nLoadgen (demo) completed: total={} dur={:.2?} throughput={:.2} ops/s mode={} bulk_size={} concurrency={} leader={}",
        args.total, dur, eps, args.mode, args.bulk_size, args.concurrency, args.leader);

    // Print latency percentiles per operation type
    println!("Latency percentiles (milliseconds):");
    {
        let c = metrics.create_ms.lock().await.clone();
        summarize("CREATE", c);
        let r = metrics.read_ms.lock().await.clone();
        summarize("READ", r);
        let u = metrics.update_ms.lock().await.clone();
        summarize("UPDATE", u);
        let d = metrics.delete_ms.lock().await.clone();
        summarize("DELETE", d);
    }
}
