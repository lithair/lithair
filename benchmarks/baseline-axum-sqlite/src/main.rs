//! Baseline for the Lithair benchmarks (v1.0 gate G4): Axum + SQLite, the
//! closest "single binary, no DB server" comparable. Exposes the same CRUD
//! shape `tools/loadgen` drives against Lithair's products model, so one load
//! harness benchmarks both fairly:
//!   POST /api/products  {name, price, category} -> {"id": "<n>"}
//!   GET  /api/products  -> [ ... ]
//!
//! SQLite is a single-writer engine, so a single connection behind a Mutex is
//! both the lazy and the honest model (WAL + synchronous=NORMAL = the standard
//! performant-but-durable config). The DB is FILE-backed (BASELINE_DB, default
//! ./baseline.db) so the write comparison is durable-vs-durable — Lithair fsyncs
//! its event log, so an in-memory baseline would be an unfair write win.
//! Port via BASELINE_PORT (default 8090).

use axum::{extract::State, routing::get, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

type Db = Arc<Mutex<rusqlite::Connection>>;

#[derive(Deserialize)]
struct NewProduct {
    name: String,
    price: f64,
    category: String,
}

#[tokio::main]
async fn main() {
    let db_path = std::env::var("BASELINE_DB").unwrap_or_else(|_| "baseline.db".to_string());
    let conn = rusqlite::Connection::open(&db_path).expect("open sqlite");
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;
         CREATE TABLE products (id INTEGER PRIMARY KEY, name TEXT, price REAL, category TEXT);",
    )
    .expect("init schema");
    let db: Db = Arc::new(Mutex::new(conn));

    let app = Router::new()
        .route("/api/products", get(list).post(create))
        .route("/health", get(|| async { "ok" }))
        .with_state(db);

    let port: u16 =
        std::env::var("BASELINE_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8090);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.expect("bind");
    eprintln!("baseline-axum-sqlite listening on 127.0.0.1:{port}");
    axum::serve(listener, app).await.expect("serve");
}

async fn create(State(db): State<Db>, Json(p): Json<NewProduct>) -> Json<Value> {
    // rusqlite is sync; the guard is never held across an await.
    let conn = db.lock().expect("db lock");
    conn.execute(
        "INSERT INTO products (name, price, category) VALUES (?1, ?2, ?3)",
        rusqlite::params![p.name, p.price, p.category],
    )
    .expect("insert");
    let id = conn.last_insert_rowid();
    Json(json!({ "id": id.to_string() }))
}

async fn list(State(db): State<Db>) -> Json<Value> {
    let conn = db.lock().expect("db lock");
    let mut stmt = conn.prepare("SELECT id, name, price, category FROM products").expect("prepare");
    let rows = stmt
        .query_map([], |r| {
            Ok(json!({
                "id": r.get::<_, i64>(0)?.to_string(),
                "name": r.get::<_, String>(1)?,
                "price": r.get::<_, f64>(2)?,
                "category": r.get::<_, String>(3)?,
            }))
        })
        .expect("query")
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    Json(Value::Array(rows))
}
