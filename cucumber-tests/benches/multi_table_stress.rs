use lithair_core::engine::{AsyncWriter, EventStore, Scc2Engine, Scc2EngineConfig};
use rand::prelude::*;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use std::time::Instant;

// --- Data Models ---

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct User {
    id: String,
    username: String,
    email: String,
}

impl lithair_core::model_inspect::Inspectable for User {
    fn get_field_value(&self, field_name: &str) -> Option<serde_json::Value> {
        match field_name {
            "id" => serde_json::to_value(&self.id).ok(),
            "username" => serde_json::to_value(&self.username).ok(),
            "email" => serde_json::to_value(&self.email).ok(),
            _ => None,
        }
    }
}

impl lithair_core::model::ModelSpec for User {
    fn get_policy(&self, _field_name: &str) -> Option<lithair_core::model::FieldPolicy> {
        None
    }
    fn get_all_fields(&self) -> Vec<String> {
        vec!["id".to_string(), "username".to_string(), "email".to_string()]
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct Product {
    id: String,
    sku: String,
    price: f64,
}

impl lithair_core::model_inspect::Inspectable for Product {
    fn get_field_value(&self, field_name: &str) -> Option<serde_json::Value> {
        match field_name {
            "id" => serde_json::to_value(&self.id).ok(),
            "sku" => serde_json::to_value(&self.sku).ok(),
            "price" => serde_json::to_value(&self.price).ok(),
            _ => None,
        }
    }
}

impl lithair_core::model::ModelSpec for Product {
    fn get_policy(&self, _field_name: &str) -> Option<lithair_core::model::FieldPolicy> {
        None
    }
    fn get_all_fields(&self) -> Vec<String> {
        vec!["id".to_string(), "sku".to_string(), "price".to_string()]
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct Order {
    id: String,
    user_id: String,
    product_ids: Vec<String>,
    total: f64,
}

impl lithair_core::model_inspect::Inspectable for Order {
    fn get_field_value(&self, field_name: &str) -> Option<serde_json::Value> {
        match field_name {
            "id" => serde_json::to_value(&self.id).ok(),
            "user_id" => serde_json::to_value(&self.user_id).ok(),
            "product_ids" => serde_json::to_value(&self.product_ids).ok(),
            "total" => serde_json::to_value(&self.total).ok(),
            _ => None,
        }
    }
}

impl lithair_core::model::ModelSpec for Order {
    fn get_policy(&self, _field_name: &str) -> Option<lithair_core::model::FieldPolicy> {
        None
    }
    fn get_all_fields(&self) -> Vec<String> {
        vec![
            "id".to_string(),
            "user_id".to_string(),
            "product_ids".to_string(),
            "total".to_string(),
        ]
    }
}

// --- Constants ---
const ITEM_COUNT: usize = 1_000_000; // 1 Million items
const READ_COUNT: usize = 500_000; // 500k Random reads

#[tokio::main]
async fn main() {
    println!("\n╔═══════════════════════════════════════════════════════════╗");
    println!("║   🚀 MULTI-TABLE STRESS TEST - 1 MILLION ITEMS           ║");
    println!("║   (Comparison: Single Log vs Multi File)                 ║");
    println!("╚═══════════════════════════════════════════════════════════╝\n");

    // Run Single Log Test (Global Log) - The Recommended Pattern
    run_single_log_test().await;

    println!("\n-------------------------------------------------------------\n");

    // Run Multi File Test (Sharded Logs) - For Comparison
    run_multi_file_test().await;
}

fn create_scc2_config() -> Scc2EngineConfig {
    Scc2EngineConfig {
        verbose_logging: false,
        enable_snapshots: false,
        snapshot_interval: 1000,
        enable_deduplication: false,
        auto_persist_writes: false,
        force_immediate_persistence: false,
    }
}

async fn run_single_log_test() {
    println!("🔵 TEST 1: GLOBAL LOG (Single File)");
    println!("   Strategy: All events (Users, Products, Orders) -> events.raftlog");

    let persist_path = "/tmp/lithair-stress-single";
    std::fs::remove_dir_all(persist_path).ok();
    std::fs::create_dir_all(persist_path).unwrap();

    // 1. Persistence
    let event_store =
        Arc::new(RwLock::new(EventStore::new(persist_path).expect("EventStore failed")));
    // AsyncWriter is Sync, so we can wrap it in Arc directly without Mutex
    // This avoids lock contention on the 'write' method which just sends to a channel
    let writer = Arc::new(AsyncWriter::new(event_store.clone(), 2000));

    // 2. Memory (SCC2)
    let users =
        Arc::new(Scc2Engine::<User>::new(event_store.clone(), create_scc2_config()).unwrap());
    let products =
        Arc::new(Scc2Engine::<Product>::new(event_store.clone(), create_scc2_config()).unwrap());
    let orders =
        Arc::new(Scc2Engine::<Order>::new(event_store.clone(), create_scc2_config()).unwrap());

    // --- WRITE PHASE ---
    println!("📝 Writing 1,000,000 items...");
    let start = Instant::now();

    let mut tasks = Vec::new();
    let chunk_size = 100_000;

    // Divide work into chunks to avoid creating 1M tasks at once
    for chunk in 0..(ITEM_COUNT / chunk_size) {
        let w = writer.clone();
        let u = users.clone();
        let p = products.clone();
        let o = orders.clone();

        tasks.push(tokio::spawn(async move {
            let start_idx = chunk * chunk_size;
            for i in 0..chunk_size {
                let abs_i = start_idx + i;
                let type_mod = abs_i % 3;

                let event_json;
                match type_mod {
                    0 => {
                        let user = User {
                            id: format!("u-{}", abs_i),
                            username: format!("User{}", abs_i),
                            email: format!("user{}@example.com", abs_i),
                        };
                        event_json = serde_json::to_string(&user).unwrap();
                        u.insert(user.id.clone(), user).await;
                    }
                    1 => {
                        let product = Product {
                            id: format!("p-{}", abs_i),
                            sku: format!("SKU-{}", abs_i),
                            price: (abs_i as f64) / 100.0,
                        };
                        event_json = serde_json::to_string(&product).unwrap();
                        p.insert(product.id.clone(), product).await;
                    }
                    _ => {
                        let order = Order {
                            id: format!("o-{}", abs_i),
                            user_id: format!("u-{}", abs_i - 2),
                            product_ids: vec![format!("p-{}", abs_i - 1)],
                            total: 99.99,
                        };
                        event_json = serde_json::to_string(&order).unwrap();
                        o.insert(order.id.clone(), order).await;
                    }
                };

                // Write to Global Log
                w.write(event_json).ok();
            }
        }));
    }

    for t in tasks {
        t.await.unwrap();
    }

    // Wait for flush (shutdown writer to guarantee persistence)
    if let Ok(arc_writer) = Arc::try_unwrap(writer) {
        arc_writer.shutdown().await;
    } else {
        println!("⚠️ Could not shutdown writer cleanly, waiting 5s...");
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }

    let elapsed = start.elapsed();
    let throughput = ITEM_COUNT as f64 / elapsed.as_secs_f64();

    println!("✅ Write Complete:");
    println!("   Time: {:.2}s", elapsed.as_secs_f64());
    println!("   Throughput: {:.0} ops/sec", throughput);

    // --- READ PHASE ---
    println!("\n🔍 Reading {} random items (SCC2)...", READ_COUNT);
    let start_read = Instant::now();

    let mut read_tasks = Vec::new();
    let read_chunk = 50_000;

    for _ in 0..(READ_COUNT / read_chunk) {
        let u = users.clone();
        let p = products.clone();
        let o = orders.clone();

        read_tasks.push(tokio::spawn(async move {
            let mut rng = StdRng::from_entropy();
            let mut hits = 0;
            for _ in 0..read_chunk {
                let target_id = rng.gen_range(0..ITEM_COUNT);
                let type_mod = target_id % 3;
                match type_mod {
                    0 => {
                        if u.read(&format!("u-{}", target_id), |_| ()).is_some() {
                            hits += 1;
                        }
                    }
                    1 => {
                        if p.read(&format!("p-{}", target_id), |_| ()).is_some() {
                            hits += 1;
                        }
                    }
                    _ => {
                        if o.read(&format!("o-{}", target_id), |_| ()).is_some() {
                            hits += 1;
                        }
                    }
                }
            }
            hits
        }));
    }

    let mut total_hits = 0;
    for t in read_tasks {
        total_hits += t.await.unwrap();
    }

    let elapsed_read = start_read.elapsed();
    let rps = READ_COUNT as f64 / elapsed_read.as_secs_f64();

    println!("✅ Read Complete (Hits: {}):", total_hits);
    println!("   Time: {:.2}s", elapsed_read.as_secs_f64());
    println!("   Throughput: {:.0} reads/sec", rps);
}

async fn run_multi_file_test() {
    println!("🟠 TEST 2: MULTI FILE (Sharded Logs)");
    println!("   Strategy: Users->users.log, Products->products.log, Orders->orders.log");

    let persist_path = "/tmp/lithair-stress-multi";
    std::fs::remove_dir_all(persist_path).ok();
    std::fs::create_dir_all(persist_path).unwrap();

    // 1. Persistence - One writer per type
    // Note: We need to manually create "EventStore" pointing to different files/dirs or use prefixes
    // EventStore takes a base path and assumes events.raftlog.
    // To allow multi-file, we simulate it by giving different directories.

    let store_users =
        Arc::new(RwLock::new(EventStore::new(&format!("{}/users", persist_path)).unwrap()));
    let store_products =
        Arc::new(RwLock::new(EventStore::new(&format!("{}/products", persist_path)).unwrap()));
    let store_orders =
        Arc::new(RwLock::new(EventStore::new(&format!("{}/orders", persist_path)).unwrap()));

    let w_users = Arc::new(AsyncWriter::new(store_users.clone(), 2000));
    let w_products = Arc::new(AsyncWriter::new(store_products.clone(), 2000));
    let w_orders = Arc::new(AsyncWriter::new(store_orders.clone(), 2000));

    // 2. Memory (SCC2)
    let users = Arc::new(Scc2Engine::<User>::new(store_users, create_scc2_config()).unwrap());
    let products =
        Arc::new(Scc2Engine::<Product>::new(store_products, create_scc2_config()).unwrap());
    let orders = Arc::new(Scc2Engine::<Order>::new(store_orders, create_scc2_config()).unwrap());

    // --- WRITE PHASE ---
    println!("📝 Writing 1,000,000 items...");
    let start = Instant::now();

    let mut tasks = Vec::new();
    let chunk_size = 100_000;

    for chunk in 0..(ITEM_COUNT / chunk_size) {
        let wu = w_users.clone();
        let wp = w_products.clone();
        let wo = w_orders.clone();
        let u = users.clone();
        let p = products.clone();
        let o = orders.clone();

        tasks.push(tokio::spawn(async move {
            let start_idx = chunk * chunk_size;
            for i in 0..chunk_size {
                let abs_i = start_idx + i;
                let type_mod = abs_i % 3;

                match type_mod {
                    0 => {
                        let user = User {
                            id: format!("u-{}", abs_i),
                            username: format!("User{}", abs_i),
                            email: format!("user{}@example.com", abs_i),
                        };
                        let json = serde_json::to_string(&user).unwrap();
                        u.insert(user.id.clone(), user).await;
                        wu.write(json).ok();
                    }
                    1 => {
                        let product = Product {
                            id: format!("p-{}", abs_i),
                            sku: format!("SKU-{}", abs_i),
                            price: (abs_i as f64) / 100.0,
                        };
                        let json = serde_json::to_string(&product).unwrap();
                        p.insert(product.id.clone(), product).await;
                        wp.write(json).ok();
                    }
                    _ => {
                        let order = Order {
                            id: format!("o-{}", abs_i),
                            user_id: format!("u-{}", abs_i - 2),
                            product_ids: vec![format!("p-{}", abs_i - 1)],
                            total: 99.99,
                        };
                        let json = serde_json::to_string(&order).unwrap();
                        o.insert(order.id.clone(), order).await;
                        wo.write(json).ok();
                    }
                };
            }
        }));
    }

    for t in tasks {
        t.await.unwrap();
    }

    // Flush all by shutting down writers
    // We need to drop the clones in tasks first (done by t.await)

    let writers = vec![w_users, w_products, w_orders];
    for w in writers {
        if let Ok(arc_w) = Arc::try_unwrap(w) {
            arc_w.shutdown().await;
        }
    }

    let elapsed = start.elapsed();
    let throughput = ITEM_COUNT as f64 / elapsed.as_secs_f64();

    println!("✅ Write Complete:");
    println!("   Time: {:.2}s", elapsed.as_secs_f64());
    println!("   Throughput: {:.0} ops/sec", throughput);

    // --- READ PHASE ---
    // Read phase is essentially identical because SCC2 is in-memory regardless of disk structure
    // But we run it to verify consistency
    println!("\n🔍 Reading {} random items...", READ_COUNT);
    let start_read = Instant::now();

    let mut read_tasks = Vec::new();
    let read_chunk = 50_000;

    for _ in 0..(READ_COUNT / read_chunk) {
        let u = users.clone();
        let p = products.clone();
        let o = orders.clone();

        read_tasks.push(tokio::spawn(async move {
            let mut rng = StdRng::from_entropy();
            let mut hits = 0;
            for _ in 0..read_chunk {
                let target_id = rng.gen_range(0..ITEM_COUNT);
                let type_mod = target_id % 3;
                match type_mod {
                    0 => {
                        if u.read(&format!("u-{}", target_id), |_| ()).is_some() {
                            hits += 1;
                        }
                    }
                    1 => {
                        if p.read(&format!("p-{}", target_id), |_| ()).is_some() {
                            hits += 1;
                        }
                    }
                    _ => {
                        if o.read(&format!("o-{}", target_id), |_| ()).is_some() {
                            hits += 1;
                        }
                    }
                }
            }
            hits
        }));
    }

    let mut total_hits = 0;
    for t in read_tasks {
        total_hits += t.await.unwrap();
    }

    let elapsed_read = start_read.elapsed();
    let rps = READ_COUNT as f64 / elapsed_read.as_secs_f64();

    println!("✅ Read Complete (Hits: {}):", total_hits);
    println!("   Time: {:.2}s", elapsed_read.as_secs_f64());
    println!("   Throughput: {:.0} reads/sec", rps);
}
