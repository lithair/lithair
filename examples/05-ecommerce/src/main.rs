//! E-Commerce Example
//!
//! A multi-model e-commerce API with Category, Product, and Order entities.
//! Demonstrates how DeclarativeModel handles multiple related models
//! with automatic CRUD on each.
//!
//! ## What you'll learn
//! - Multiple `#[derive(DeclarativeModel)]` structs in one server
//! - Each model gets its own REST API automatically
//! - Builder pattern with `.with_model::<T>()` per entity
//!
//! ## Run
//! ```bash
//! cargo run -p ecommerce
//! ```
//!
//! ## Test
//! ```bash
//! # Create a category
//! curl -X POST http://localhost:8081/api/categories \
//!   -H "Content-Type: application/json" \
//!   -d '{"name": "Electronics"}'
//!
//! # List categories
//! curl http://localhost:8081/api/categories
//!
//! # Create a product
//! curl -X POST http://localhost:8081/api/products \
//!   -H "Content-Type: application/json" \
//!   -d '{"name": "Laptop", "price": 999.99, "stock": 50, "category_id": "<category-id>"}'
//!
//! # List products
//! curl http://localhost:8081/api/products
//!
//! # Create an order
//! curl -X POST http://localhost:8081/api/orders \
//!   -H "Content-Type: application/json" \
//!   -d '{"product_id": "<product-id>", "quantity": 2, "status": "Pending"}'
//!
//! # List orders
//! curl http://localhost:8081/api/orders
//! ```

use anyhow::Result;
use lithair_core::app::LithairServer;
use lithair_core::logging::LoggingConfig;
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};

/// Product category.
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
struct Category {
    #[http(expose)]
    id: String,

    #[http(expose)]
    name: String,
}

/// Product with a reference to its category.
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
struct Product {
    #[http(expose)]
    id: String,

    #[http(expose)]
    name: String,

    #[http(expose)]
    price: f64,

    #[http(expose)]
    stock: i32,

    #[http(expose)]
    category_id: Option<String>,
}

/// Customer order referencing a product.
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
struct Order {
    #[http(expose)]
    id: String,

    #[http(expose)]
    product_id: String,

    #[http(expose)]
    quantity: i32,

    #[http(expose)]
    status: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("E-Commerce API");
    println!("==============");
    println!();
    println!("Models (auto-generated CRUD for each):");
    println!("  Category  http://localhost:8081/api/categories");
    println!("  Product   http://localhost:8081/api/products");
    println!("  Order     http://localhost:8081/api/orders");
    println!();

    LithairServer::new()
        .with_port(8081)
        .with_host("127.0.0.1")
        .with_logging_config(LoggingConfig::development())
        .with_model::<Category>("./data/ecommerce/categories", "/api/categories")
        .with_model::<Product>("./data/ecommerce/products", "/api/products")
        .with_model::<Order>("./data/ecommerce/orders", "/api/orders")
        .with_admin_panel(true)
        .serve()
        .await?;

    Ok(())
}
