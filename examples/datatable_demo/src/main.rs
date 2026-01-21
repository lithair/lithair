//! Lithair DataTable Demo - Multi-Table Relational
//!
//! Demonstrates the complete Lithair stack with multiple related tables:
//! - Products: Catalog items
//! - Consumers: Customer accounts
//! - Orders: Relations between consumers and products
//!
//! Stack: DeclarativeModel + LithairServer + Scc2Engine + Event Sourcing

use anyhow::Result;
use bytes::Bytes;
use clap::Parser;
use futures::future;
use http::{Method, Response, StatusCode};
use http_body_util::Full;
use lithair_core::app::LithairServer;
use lithair_core::frontend::{FrontendEngine, FrontendServer};
use lithair_core::http::DeclarativeHttpHandler;
use lithair_macros::DeclarativeModel;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

// ============================================================================
// MODELS
// ============================================================================

/// Product model - Catalog items
#[derive(Debug, Clone, Default, Serialize, Deserialize, DeclarativeModel)]
struct Product {
    #[http(expose)]
    id: String,
    #[http(expose)]
    name: String,
    #[http(expose)]
    description: String,
    #[http(expose)]
    category: String,
    #[http(expose)]
    price: f64,
    #[http(expose)]
    stock: u32,
    #[http(expose)]
    sku: String,
    #[http(expose)]
    brand: String,
    #[http(expose)]
    rating: f32,
    #[http(expose)]
    reviews_count: u32,
    #[http(expose)]
    active: bool,
}

/// Consumer model - Customer accounts
#[derive(Debug, Clone, Default, Serialize, Deserialize, DeclarativeModel)]
struct Consumer {
    #[http(expose)]
    id: String,
    #[http(expose)]
    email: String,
    #[http(expose)]
    first_name: String,
    #[http(expose)]
    last_name: String,
    #[http(expose)]
    phone: String,
    #[http(expose)]
    address: String,
    #[http(expose)]
    city: String,
    #[http(expose)]
    country: String,
    #[http(expose)]
    created_at: String,
    #[http(expose)]
    total_orders: u32,
    #[http(expose)]
    total_spent: f64,
    #[http(expose)]
    vip: bool,
}

/// Order model - Relations between consumers and products
#[derive(Debug, Clone, Default, Serialize, Deserialize, DeclarativeModel)]
struct Order {
    #[http(expose)]
    id: String,
    #[http(expose)]
    consumer_id: String, // FK -> Consumer
    #[http(expose)]
    product_ids: Vec<String>, // FK[] -> Products
    #[http(expose)]
    quantities: Vec<u32>, // Quantity per product
    #[http(expose)]
    total_amount: f64,
    #[http(expose)]
    status: String, // pending, confirmed, shipped, delivered, cancelled
    #[http(expose)]
    created_at: String,
    #[http(expose)]
    updated_at: String,
    #[http(expose)]
    shipping_address: String,
    #[http(expose)]
    notes: String,
}

// ============================================================================
// DATA GENERATORS
// ============================================================================

const CATEGORIES: &[&str] = &[
    "Electronics",
    "Clothing",
    "Home & Garden",
    "Sports",
    "Books",
    "Toys",
    "Automotive",
    "Health",
    "Beauty",
    "Food",
];

const BRANDS: &[&str] = &[
    "TechCorp",
    "StyleMax",
    "HomeFirst",
    "SportPro",
    "BookWorld",
    "PlayTime",
    "AutoParts",
    "HealthPlus",
    "BeautyGlow",
    "FoodFresh",
];

const ADJECTIVES: &[&str] = &[
    "Premium",
    "Professional",
    "Ultra",
    "Smart",
    "Classic",
    "Modern",
    "Compact",
    "Wireless",
    "Digital",
    "Advanced",
];

const NOUNS: &[&str] = &[
    "Widget",
    "Device",
    "Tool",
    "Gadget",
    "System",
    "Kit",
    "Set",
    "Pack",
    "Bundle",
    "Collection",
];

const FIRST_NAMES: &[&str] = &[
    "Alice", "Bob", "Charlie", "Diana", "Edward", "Fiona", "George", "Hannah", "Ivan", "Julia",
    "Kevin", "Laura", "Michael", "Nina", "Oscar",
];

const LAST_NAMES: &[&str] = &[
    "Smith", "Johnson", "Williams", "Brown", "Jones", "Garcia", "Miller", "Davis", "Martinez",
    "Anderson", "Taylor", "Thomas", "Moore", "Jackson", "Martin",
];

const CITIES: &[&str] = &[
    "Paris",
    "London",
    "Berlin",
    "Madrid",
    "Rome",
    "Amsterdam",
    "Brussels",
    "Vienna",
    "Prague",
    "Lisbon",
];

const COUNTRIES: &[&str] = &[
    "France",
    "UK",
    "Germany",
    "Spain",
    "Italy",
    "Netherlands",
    "Belgium",
    "Austria",
    "Czech Republic",
    "Portugal",
];

const ORDER_STATUSES: &[&str] = &["pending", "confirmed", "shipped", "delivered", "cancelled"];

fn generate_product(rng: &mut impl Rng) -> Product {
    let adj = ADJECTIVES[rng.gen_range(0..ADJECTIVES.len())];
    let noun = NOUNS[rng.gen_range(0..NOUNS.len())];
    let category = CATEGORIES[rng.gen_range(0..CATEGORIES.len())];
    let brand = BRANDS[rng.gen_range(0..BRANDS.len())];

    Product {
        id: Uuid::new_v4().to_string(),
        name: format!("{} {} {}", brand, adj, noun),
        description: format!(
            "High-quality {} {} from {}. Perfect for your {} needs.",
            adj.to_lowercase(),
            noun.to_lowercase(),
            brand,
            category.to_lowercase()
        ),
        category: category.to_string(),
        price: (rng.gen_range(10..10000) as f64) + (rng.gen_range(0..100) as f64) / 100.0,
        stock: rng.gen_range(0..1000),
        sku: format!(
            "{}-{}-{:06}",
            &brand[0..3].to_uppercase(),
            &category[0..2].to_uppercase(),
            rng.gen_range(0..999999)
        ),
        brand: brand.to_string(),
        rating: (rng.gen_range(30..50) as f32) / 10.0,
        reviews_count: rng.gen_range(0..5000),
        active: rng.gen_bool(0.9),
    }
}

fn generate_consumer(rng: &mut impl Rng) -> Consumer {
    let first = FIRST_NAMES[rng.gen_range(0..FIRST_NAMES.len())];
    let last = LAST_NAMES[rng.gen_range(0..LAST_NAMES.len())];
    let city_idx = rng.gen_range(0..CITIES.len());

    Consumer {
        id: Uuid::new_v4().to_string(),
        email: format!("{}.{}@example.com", first.to_lowercase(), last.to_lowercase()),
        first_name: first.to_string(),
        last_name: last.to_string(),
        phone: format!(
            "+33 6 {:02} {:02} {:02} {:02}",
            rng.gen_range(10..99),
            rng.gen_range(10..99),
            rng.gen_range(10..99),
            rng.gen_range(10..99)
        ),
        address: format!(
            "{} Rue de la {}",
            rng.gen_range(1..200),
            NOUNS[rng.gen_range(0..NOUNS.len())]
        ),
        city: CITIES[city_idx].to_string(),
        country: COUNTRIES[city_idx].to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
        total_orders: rng.gen_range(0..50),
        total_spent: (rng.gen_range(0..50000) as f64) + (rng.gen_range(0..100) as f64) / 100.0,
        vip: rng.gen_bool(0.1),
    }
}

fn generate_order(
    rng: &mut impl Rng,
    consumer_ids: &[String],
    product_ids: &[String],
) -> Option<Order> {
    if consumer_ids.is_empty() || product_ids.is_empty() {
        return None;
    }

    let consumer_id = consumer_ids[rng.gen_range(0..consumer_ids.len())].clone();
    let num_products = rng.gen_range(1..=5);
    let mut order_product_ids = Vec::new();
    let mut quantities = Vec::new();
    let mut total = 0.0;

    for _ in 0..num_products {
        let pid = product_ids[rng.gen_range(0..product_ids.len())].clone();
        if !order_product_ids.contains(&pid) {
            order_product_ids.push(pid);
            let qty = rng.gen_range(1..=3);
            quantities.push(qty);
            total += (rng.gen_range(10..500) as f64) * (qty as f64);
        }
    }

    Some(Order {
        id: Uuid::new_v4().to_string(),
        consumer_id,
        product_ids: order_product_ids,
        quantities,
        total_amount: total,
        status: ORDER_STATUSES[rng.gen_range(0..ORDER_STATUSES.len())].to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
        updated_at: chrono::Utc::now().to_rfc3339(),
        shipping_address: format!(
            "{} {}",
            rng.gen_range(1..200),
            CITIES[rng.gen_range(0..CITIES.len())]
        ),
        notes: if rng.gen_bool(0.3) {
            "Gift wrapping requested".to_string()
        } else {
            String::new()
        },
    })
}

// ============================================================================
// CLI & MAIN
// ============================================================================

#[derive(Parser)]
#[command(name = "datatable_demo", about = "Lithair DataTable Demo - Multi-Table Relational")]
struct Args {
    #[arg(short, long, default_value = "3001")]
    port: u16,

    #[arg(long, default_value = "examples/datatable_demo/data")]
    data_dir: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    println!();
    println!("============================================");
    println!("   Lithair DataTable Demo - Relational");
    println!("============================================");
    println!();
    println!("Tables:");
    println!("  - Products   : Catalog items");
    println!("  - Consumers  : Customer accounts");
    println!("  - Orders     : Relations (consumer -> products)");
    println!();
    println!("Data directory: {}", args.data_dir.display());
    println!("  ├── products/   - Product events");
    println!("  ├── consumers/  - Consumer events");
    println!("  ├── orders/     - Order events");
    println!("  └── frontend/   - Static assets");
    println!();
    println!("Server: http://127.0.0.1:{}", args.port);
    println!();
    println!("API Endpoints:");
    println!("  Products:  GET|POST /api/products, GET|PUT|DELETE /api/products/:id");
    println!("  Consumers: GET|POST /api/consumers, GET|PUT|DELETE /api/consumers/:id");
    println!("  Orders:    GET|POST /api/orders, GET|PUT|DELETE /api/orders/:id");
    println!();
    println!("Batch Seed:");
    println!("  POST /api/seed/products?count=N");
    println!("  POST /api/seed/consumers?count=N");
    println!("  POST /api/seed/orders?count=N");
    println!();
    println!("Relational Queries:");
    println!("  GET /api/consumers/:id/orders  - Get orders for a consumer (FK query)");
    println!("  GET /api/orders/:id/expanded   - Get order with consumer & products (JOIN)");
    println!("  GET /api/stats                 - Get statistics");
    println!();

    // Create handlers for each table with separate data paths
    let product_handler = Arc::new(
        DeclarativeHttpHandler::<Product>::new_with_replay(&format!(
            "{}/products",
            args.data_dir.display()
        ))
        .await
        .expect("Failed to create product handler"),
    );

    let consumer_handler = Arc::new(
        DeclarativeHttpHandler::<Consumer>::new_with_replay(&format!(
            "{}/consumers",
            args.data_dir.display()
        ))
        .await
        .expect("Failed to create consumer handler"),
    );

    let order_handler = Arc::new(
        DeclarativeHttpHandler::<Order>::new_with_replay(&format!(
            "{}/orders",
            args.data_dir.display()
        ))
        .await
        .expect("Failed to create order handler"),
    );

    // Create frontend engine
    let frontend_engine = Arc::new(
        FrontendEngine::new("frontend", &format!("{}/frontend", args.data_dir.display()))
            .await
            .expect("Failed to create frontend engine"),
    );

    match frontend_engine.load_directory("examples/datatable_demo/frontend").await {
        Ok(count) => println!("Loaded {} frontend assets\n", count),
        Err(e) => println!("Warning: Could not load frontend assets: {}\n", e),
    }

    let frontend_server = Arc::new(FrontendServer::new_scc2(frontend_engine.clone()));

    // Clone handlers for custom routes (relational queries, seed, stats)
    let ph_seed = product_handler.clone();
    let ch_seed = consumer_handler.clone();
    let oh_seed = order_handler.clone();
    let oh_rel = order_handler.clone();
    let oh_exp = order_handler.clone();
    let ch_exp = consumer_handler.clone();
    let ph_exp = product_handler.clone();
    let ph_stats = product_handler.clone();
    let ch_stats = consumer_handler.clone();
    let oh_stats = order_handler.clone();

    // Build server using with_handler for automatic CRUD route registration
    LithairServer::new()
        .with_port(args.port)
        .with_host("127.0.0.1")

        // ========== CUSTOM ROUTES (must come first for proper route priority) ==========
        // FK Query: /api/consumers/:id/orders
        .with_route(Method::GET, "/api/consumers/*/orders", {
            let handler = oh_rel;
            move |req| {
                let handler = handler.clone();
                Box::pin(async move { get_consumer_orders(req, handler).await })
            }
        })
        // JOIN Query: /api/orders/:id/expanded
        .with_route(Method::GET, "/api/orders/*/expanded", {
            let oh = oh_exp;
            let ch = ch_exp;
            let ph = ph_exp;
            move |req| {
                let oh = oh.clone();
                let ch = ch.clone();
                let ph = ph.clone();
                Box::pin(async move { get_order_expanded(req, oh, ch, ph).await })
            }
        })

        // ========== AUTOMATIC CRUD ROUTES (via with_handler) ==========
        // Products: GET/POST /api/products, GET/PUT/DELETE /api/products/*
        .with_handler(product_handler.clone(), "/api/products")
        // Consumers: GET/POST /api/consumers, GET/PUT/DELETE /api/consumers/*
        .with_handler(consumer_handler.clone(), "/api/consumers")
        // Orders: GET/POST /api/orders, GET/PUT/DELETE /api/orders/*
        .with_handler(order_handler.clone(), "/api/orders")

        // ========== BATCH SEED ROUTES ==========
        .with_route(Method::POST, "/api/seed/products".to_string(), {
            let handler = ph_seed;
            move |req| {
                let handler = handler.clone();
                Box::pin(async move { seed_products(req, handler).await })
            }
        })
        .with_route(Method::POST, "/api/seed/consumers".to_string(), {
            let handler = ch_seed;
            move |req| {
                let handler = handler.clone();
                Box::pin(async move { seed_consumers(req, handler).await })
            }
        })
        .with_route(Method::POST, "/api/seed/orders".to_string(), {
            let ph = product_handler.clone();
            let ch = consumer_handler.clone();
            let handler = oh_seed;
            move |req| {
                let ph = ph.clone();
                let ch = ch.clone();
                let handler = handler.clone();
                Box::pin(async move { seed_orders(req, handler, ph, ch).await })
            }
        })

        // ========== STATS ==========
        .with_route(Method::GET, "/api/stats".to_string(), {
            move |_req| {
                let ph = ph_stats.clone();
                let ch = ch_stats.clone();
                let oh = oh_stats.clone();
                Box::pin(async move { handle_stats(ph, ch, oh).await })
            }
        })

        // ========== FRONTEND ==========
        .with_route(Method::GET, "/*".to_string(), move |req| {
            let server = frontend_server.clone();
            Box::pin(async move {
                use http_body_util::BodyExt;
                let response = server.handle_request(req).await
                    .map_err(|e| anyhow::anyhow!("{:?}", e))?;
                let (parts, body) = response.into_parts();
                let bytes = body.collect().await
                    .map_err(|_| anyhow::anyhow!("Failed to collect body"))?
                    .to_bytes();
                Ok(Response::from_parts(parts, Full::new(bytes)))
            })
        })

        .serve()
        .await?;

    Ok(())
}

// ============================================================================
// HANDLERS
// ============================================================================

/// Seed products
async fn seed_products(
    req: http::Request<hyper::body::Incoming>,
    handler: Arc<DeclarativeHttpHandler<Product>>,
) -> Result<Response<Full<Bytes>>> {
    let count = parse_count(&req, 100);
    let start = Instant::now();
    let mut rng = rand::rngs::StdRng::from_entropy();

    for _ in 0..count {
        handler.apply_replicated_item(generate_product(&mut rng)).await;
    }

    json_response(&serde_json::json!({
        "table": "products",
        "inserted": count,
        "duration_ms": start.elapsed().as_millis(),
        "throughput_per_sec": throughput(count, start.elapsed())
    }))
}

/// Seed consumers
async fn seed_consumers(
    req: http::Request<hyper::body::Incoming>,
    handler: Arc<DeclarativeHttpHandler<Consumer>>,
) -> Result<Response<Full<Bytes>>> {
    let count = parse_count(&req, 50);
    let start = Instant::now();
    let mut rng = rand::rngs::StdRng::from_entropy();

    for _ in 0..count {
        handler.apply_replicated_item(generate_consumer(&mut rng)).await;
    }

    json_response(&serde_json::json!({
        "table": "consumers",
        "inserted": count,
        "duration_ms": start.elapsed().as_millis(),
        "throughput_per_sec": throughput(count, start.elapsed())
    }))
}

/// Seed orders (needs existing consumers and products)
async fn seed_orders(
    req: http::Request<hyper::body::Incoming>,
    handler: Arc<DeclarativeHttpHandler<Order>>,
    product_handler: Arc<DeclarativeHttpHandler<Product>>,
    consumer_handler: Arc<DeclarativeHttpHandler<Consumer>>,
) -> Result<Response<Full<Bytes>>> {
    let count = parse_count(&req, 50);
    let start = Instant::now();
    let mut rng = rand::rngs::StdRng::from_entropy();

    // Get REAL IDs from storage using the new get_all_items method
    let all_products = product_handler.get_all_items().await;
    let all_consumers = consumer_handler.get_all_items().await;

    if all_products.is_empty() || all_consumers.is_empty() {
        return json_response(&serde_json::json!({
            "error": "Need products and consumers first",
            "products_count": all_products.len(),
            "consumers_count": all_consumers.len(),
            "hint": "Call /api/seed/products and /api/seed/consumers first"
        }));
    }

    // Use real IDs from storage
    let product_ids: Vec<String> = all_products.iter().map(|p| p.id.clone()).collect();
    let consumer_ids: Vec<String> = all_consumers.iter().map(|c| c.id.clone()).collect();

    let mut inserted = 0;
    for _ in 0..count {
        if let Some(order) = generate_order(&mut rng, &consumer_ids, &product_ids) {
            handler.apply_replicated_item(order).await;
            inserted += 1;
        }
    }

    json_response(&serde_json::json!({
        "table": "orders",
        "inserted": inserted,
        "duration_ms": start.elapsed().as_millis(),
        "throughput_per_sec": throughput(inserted, start.elapsed()),
        "real_consumer_ids_sample": consumer_ids.iter().take(3).collect::<Vec<_>>(),
        "real_product_ids_sample": product_ids.iter().take(3).collect::<Vec<_>>()
    }))
}

/// Get orders for a specific consumer - Real relational query!
async fn get_consumer_orders(
    req: http::Request<hyper::body::Incoming>,
    handler: Arc<DeclarativeHttpHandler<Order>>,
) -> Result<Response<Full<Bytes>>> {
    // Extract consumer_id from path: /api/consumers/{id}/orders
    let path = req.uri().path();
    let parts: Vec<&str> = path.split('/').collect();
    let consumer_id = parts.get(3).unwrap_or(&"").to_string();

    // Use the new query method for relational filtering
    let consumer_id_clone = consumer_id.clone();
    let orders = handler.query(move |order| order.consumer_id == consumer_id_clone).await;

    json_response(&serde_json::json!({
        "consumer_id": consumer_id,
        "orders_count": orders.len(),
        "orders": orders
    }))
}

/// Get expanded order with consumer and product details - JOIN operation!
async fn get_order_expanded(
    req: http::Request<hyper::body::Incoming>,
    order_handler: Arc<DeclarativeHttpHandler<Order>>,
    consumer_handler: Arc<DeclarativeHttpHandler<Consumer>>,
    product_handler: Arc<DeclarativeHttpHandler<Product>>,
) -> Result<Response<Full<Bytes>>> {
    // Extract order_id from path: /api/orders/{id}/expanded
    let path = req.uri().path();
    let parts: Vec<&str> = path.split('/').collect();
    let order_id = parts.get(3).unwrap_or(&"").to_string();

    // Get the order
    let order = match order_handler.get_by_id(&order_id).await {
        Some(o) => o,
        None => {
            return json_response(&serde_json::json!({
                "error": "Order not found",
                "order_id": order_id
            }))
        }
    };

    // Get related consumer
    let consumer = consumer_handler.get_by_id(&order.consumer_id).await;

    // Get related products
    let products: Vec<_> =
        future::join_all(order.product_ids.iter().map(|pid| product_handler.get_by_id(pid)))
            .await
            .into_iter()
            .flatten()
            .collect();

    json_response(&serde_json::json!({
        "order": order,
        "consumer": consumer,
        "products": products,
        "resolved_products_count": products.len(),
        "requested_products_count": order.product_ids.len()
    }))
}

/// Stats endpoint
async fn handle_stats(
    ph: Arc<DeclarativeHttpHandler<Product>>,
    ch: Arc<DeclarativeHttpHandler<Consumer>>,
    oh: Arc<DeclarativeHttpHandler<Order>>,
) -> Result<Response<Full<Bytes>>> {
    json_response(&serde_json::json!({
        "tables": {
            "products": ph.storage_count().await,
            "consumers": ch.storage_count().await,
            "orders": oh.storage_count().await
        },
        "engine": "Scc2Engine (lock-free, 40M+ ops/sec)",
        "persistence": "Event Sourcing with CRC32",
        "model": "DeclarativeModel with automatic CRUD"
    }))
}

// ============================================================================
// HELPERS
// ============================================================================

fn parse_count(req: &http::Request<hyper::body::Incoming>, default: usize) -> usize {
    req.uri()
        .query()
        .unwrap_or("")
        .split('&')
        .find(|p| p.starts_with("count="))
        .and_then(|p| p.strip_prefix("count="))
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
        .min(100_000)
}

fn throughput(count: usize, duration: std::time::Duration) -> u64 {
    let ms = duration.as_millis() as u64;
    if ms > 0 {
        (count as u64 * 1000) / ms
    } else {
        count as u64
    }
}

fn json_response(value: &serde_json::Value) -> Result<Response<Full<Bytes>>> {
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .header("Access-Control-Allow-Origin", "*")
        .body(Full::new(Bytes::from(value.to_string())))
        .unwrap())
}
