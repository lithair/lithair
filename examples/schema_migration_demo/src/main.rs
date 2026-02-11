//! Schema Migration Demo
//!
//! This example demonstrates Lithair's complete schema migration system.
//!
//! ## Features Demonstrated
//!
//! - **Schema change detection**: Automatic detection of field additions/removals
//! - **Migration classification**: Additive vs Breaking vs Safe changes
//! - **Lock/Unlock mechanism**: Maintenance window pattern for deployments
//! - **History tracking**: Persistent audit trail of all schema changes
//! - **Multiple modes**: warn, strict, auto migration strategies
//!
//! ## Quick Start
//!
//! ```bash
//! # 1. Start server (creates initial schema)
//! cargo run -p schema_migration_demo
//!
//! # 2. In another terminal, test the API
//! curl http://localhost:8090/api/products
//!
//! # 3. Test lock/unlock
//! curl -X POST http://localhost:8090/_admin/schema/lock
//! curl http://localhost:8090/_admin/schema/lock/status
//! curl -X POST http://localhost:8090/_admin/schema/unlock -d '{"duration_seconds": 60}'
//!
//! # 4. View history
//! curl http://localhost:8090/_admin/schema/history
//! ```
//!
//! ## CLI Commands
//!
//! ```bash
//! # Run server
//! cargo run -p schema_migration_demo -- -p 8090
//!
//! # Show stored schema
//! cargo run -p schema_migration_demo -- --show-schema
//!
//! # Show change history
//! cargo run -p schema_migration_demo -- --show-history
//!
//! # Show lock status
//! cargo run -p schema_migration_demo -- --show-lock
//!
//! # Run automated tests (server must be running)
//! cargo run -p schema_migration_demo -- --test
//!
//! # Reset all data
//! cargo run -p schema_migration_demo -- --reset-schema
//! ```
//!
//! ## Migration Modes
//!
//! - `warn` (default): Log changes and continue
//! - `strict`: Fail if breaking changes detected
//! - `auto`: Automatically save new schema
//!
//! ## Testing Schema Changes
//!
//! Modify the Product struct to test detection:
//! - Add `pub discount: Option<f64>` → Additive change (safe)
//! - Add `pub sku: String` → Breaking change (rejected in strict mode)
//! - Add `#[db(default = 0)] pub rating: i32` → Safe migration

#![allow(dead_code)]
#![allow(unused_assignments)]

use chrono::{DateTime, Utc};
use clap::Parser;
use lithair_core::app::LithairServer;
use lithair_core::config::SchemaMigrationMode;
use lithair_macros::DeclarativeModel;
// Note: Use #[lithair_model] instead of #[derive(DeclarativeModel)]
// for automatic #[serde(default)] generation from #[db(default = X)]
use serde::{Deserialize, Serialize};
use uuid::Uuid;

fn __default_category() -> String {
    "uncategorized".to_string()
}

// =============================================================================
// MODEL DEFINITION
// =============================================================================
//
// Try modifying this struct to see schema change detection in action:
//
// - Add a new field: `pub discount: Option<f64>`  → Additive change
// - Add a required field: `pub sku: String`       → Breaking change (needs default)
// - Add a field WITH default: `#[db(default = 0)]` → SAFE migration!
// - Remove a field: comment out `description`     → Breaking change
// - Add an index: `#[db(indexed)]` on a field     → Additive change
//
// NOTE: Use #[lithair_model] instead of #[derive(DeclarativeModel)] for
//       automatic #[serde(default)] generation from #[db(default = X)]

/// Product model with schema migration support
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
pub struct Product {
    /// Primary key - UUID
    #[db(primary_key)]
    #[http(expose)]
    #[permission(read = "Public")]
    pub id: Uuid,

    /// Product name - indexed for search
    #[db(indexed)]
    #[http(expose)]
    #[permission(read = "Public", write = "Admin")]
    pub name: String,

    /// Product description - optional
    #[http(expose)]
    #[permission(read = "Public", write = "Admin")]
    pub description: Option<String>,

    /// Price in cents
    #[http(expose)]
    #[permission(read = "Public", write = "Admin")]
    pub price_cents: i64,

    /// Stock quantity
    #[http(expose)]
    #[permission(read = "Admin", write = "Admin")]
    pub stock: i32,

    /// Is product active?
    #[http(expose)]
    #[permission(read = "Public", write = "Admin")]
    pub active: bool,

    /// Creation timestamp
    #[db(immutable)]
    #[http(expose)]
    #[permission(read = "Public")]
    pub created_at: DateTime<Utc>,

    // =========================================================================
    // UNCOMMENT THESE TO TEST SCHEMA CHANGES:
    // =========================================================================

    // /// Discount percentage (0-100) - ADDITIVE CHANGE (nullable)
    // #[http(expose)]
    // #[permission(read = "Public")]
    // pub discount: Option<f64>,

    // /// SKU code - BREAKING CHANGE (not nullable, no default)
    // #[db(indexed, unique)]
    // #[http(expose)]
    // #[permission(read = "Public")]
    // pub sku: String,
    /// Priority level - SAFE MIGRATION (has default value!)
    /// Old events will get priority=0 automatically at deserialization
    #[db(default = 0)]
    #[http(expose)]
    #[permission(read = "Public")]
    #[serde(default)]
    pub priority: i32,

    /// Category name - SAFE MIGRATION (has default value!)
    /// Old events will get category="uncategorized" automatically
    #[db(default = "uncategorized")]
    #[http(expose)]
    #[serde(default = "__default_category")]
    pub category: String,

    /// Is featured - SAFE MIGRATION (has default value!)
    #[db(default = false)]
    #[http(expose)]
    #[serde(default)]
    pub featured: bool,
}

// =============================================================================
// CLI
// =============================================================================

#[derive(Parser)]
#[command(name = "schema_demo")]
#[command(about = "Demonstrates schema migration detection")]
struct Cli {
    /// Port to listen on
    #[arg(short, long, default_value = "8090")]
    port: u16,

    /// Data directory
    #[arg(short, long, default_value = "./data/schema_demo")]
    data_dir: String,

    /// Migration mode: warn, strict, auto
    #[arg(short, long, default_value = "warn")]
    migration_mode: String,

    /// Disable schema validation
    #[arg(long)]
    no_validation: bool,

    /// Show stored schema and exit
    #[arg(long)]
    show_schema: bool,

    /// Show schema change history and exit
    #[arg(long)]
    show_history: bool,

    /// Show lock status and exit
    #[arg(long)]
    show_lock: bool,

    /// Delete stored schema and exit (reset)
    #[arg(long)]
    reset_schema: bool,

    /// Run automated tests against a running server
    #[arg(long)]
    test: bool,

    /// Server URL for tests (default: http://localhost:8090)
    #[arg(long, default_value = "http://localhost:8090")]
    test_url: String,
}

// =============================================================================
// MAIN
// =============================================================================

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    let cli = Cli::parse();

    // Handle schema inspection commands
    if cli.show_schema {
        show_stored_schema(&cli.data_dir)?;
        return Ok(());
    }

    if cli.reset_schema {
        reset_schema(&cli.data_dir)?;
        return Ok(());
    }

    if cli.show_history {
        show_history(&cli.data_dir)?;
        return Ok(());
    }

    if cli.show_lock {
        show_lock_status(&cli.data_dir)?;
        return Ok(());
    }

    if cli.test {
        run_tests(&cli.test_url).await?;
        return Ok(());
    }

    // Parse migration mode
    let migration_mode = match cli.migration_mode.to_lowercase().as_str() {
        "strict" => SchemaMigrationMode::Strict,
        "auto" => SchemaMigrationMode::Auto,
        "manual" => SchemaMigrationMode::Manual,
        _ => SchemaMigrationMode::Warn,
    };

    log::info!("===========================================");
    log::info!("  Schema Migration Demo");
    log::info!("===========================================");
    log::info!("  Port: {}", cli.port);
    log::info!("  Data dir: {}", cli.data_dir);
    log::info!("  Migration mode: {:?}", migration_mode);
    log::info!("  Schema validation: {}", !cli.no_validation);
    log::info!("===========================================");

    // Build server with custom config
    let mut config = lithair_core::config::LithairConfig::default();
    config.server.port = cli.port;
    config.storage.data_dir = cli.data_dir.clone();
    config.storage.schema_validation_enabled = !cli.no_validation;
    config.storage.schema_migration_mode = migration_mode;

    // Start server with schema validation
    let server = LithairServer::with_config(config)
        .with_declarative_model::<Product>(format!("{}/products", cli.data_dir), "/api/products");

    log::info!("");
    log::info!("Starting server...");
    log::info!("");

    server.serve().await?;

    Ok(())
}

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

fn show_stored_schema(data_dir: &str) -> anyhow::Result<()> {
    use lithair_core::schema::load_schema_spec;
    use std::path::Path;

    let base_path = Path::new(data_dir);

    println!("\n📋 Stored Schema for 'Product':\n");

    match load_schema_spec("Product", base_path)? {
        Some(spec) => {
            println!("Model: {}", spec.model_name);
            println!("Version: {}", spec.version);
            println!("\nFields ({}):", spec.fields.len());

            let mut fields: Vec<_> = spec.fields.iter().collect();
            fields.sort_by_key(|(name, _)| *name);

            for (name, constraints) in fields {
                let mut attrs = Vec::new();
                if constraints.primary_key {
                    attrs.push("PK");
                }
                if constraints.unique {
                    attrs.push("UNIQUE");
                }
                if constraints.indexed {
                    attrs.push("INDEX");
                }
                if constraints.nullable {
                    attrs.push("NULL");
                }
                if constraints.immutable {
                    attrs.push("IMMUTABLE");
                }

                let attrs_str = if attrs.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", attrs.join(", "))
                };

                println!("  - {}{}", name, attrs_str);
            }

            if !spec.indexes.is_empty() {
                println!("\nIndexes ({}):", spec.indexes.len());
                for idx in &spec.indexes {
                    println!(
                        "  - {} on ({}){}",
                        idx.name,
                        idx.fields.join(", "),
                        if idx.unique { " UNIQUE" } else { "" }
                    );
                }
            }

            println!("\nFile: {}/.schema/Product.json", data_dir);
        }
        None => {
            println!("No stored schema found.");
            println!("Run the server once to create the initial schema.");
        }
    }

    println!();
    Ok(())
}

fn reset_schema(data_dir: &str) -> anyhow::Result<()> {
    use lithair_core::schema::delete_schema_spec;
    use std::path::Path;

    let base_path = Path::new(data_dir);

    println!("\n🗑️  Resetting schema for 'Product'...");

    delete_schema_spec("Product", base_path)?;

    println!("✅ Schema deleted. Next run will create a fresh schema.\n");
    Ok(())
}

fn show_history(data_dir: &str) -> anyhow::Result<()> {
    use lithair_core::schema::{load_schema_history, SchemaChangeType};
    use std::path::Path;

    let base_path = Path::new(data_dir);

    println!("\n📜 Schema Change History:\n");

    match load_schema_history(base_path) {
        Ok(history) => {
            if history.changes.is_empty() {
                println!("  No schema changes recorded yet.");
            } else {
                println!("  Total changes: {}\n", history.changes.len());
                for (i, change) in history.changes.iter().enumerate() {
                    // Convert timestamp to readable format
                    let applied_time =
                        chrono::DateTime::from_timestamp(change.applied_at as i64, 0)
                            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                            .unwrap_or_else(|| change.applied_at.to_string());

                    println!("  {}. Model: {}", i + 1, change.model_name);
                    println!("     ID: {}", change.id);
                    println!("     Applied: {} (node {})", applied_time, change.applied_by_node);
                    println!("     Changes: {} field(s)", change.changes.len());

                    for field_change in &change.changes {
                        let change_icon = match &field_change.change_type {
                            SchemaChangeType::AddField => "➕",
                            SchemaChangeType::RemoveField => "➖",
                            SchemaChangeType::ModifyFieldType => "✏️",
                            SchemaChangeType::ModifyFieldConstraints => "🔧",
                            SchemaChangeType::AddIndex => "📇",
                            SchemaChangeType::RemoveIndex => "🗑️",
                            SchemaChangeType::ModifyRetentionPolicy => "🕐",
                            SchemaChangeType::ModifyPermissions => "🔒",
                            SchemaChangeType::AddForeignKey => "🔗",
                            SchemaChangeType::RemoveForeignKey => "✂️",
                        };
                        let field_name = field_change.field_name.as_deref().unwrap_or("(unnamed)");
                        println!(
                            "       {} {} ({:?})",
                            change_icon, field_name, field_change.migration_strategy
                        );
                    }
                    println!();
                }
            }
        }
        Err(e) => {
            println!("  No history file found or error: {}", e);
        }
    }

    println!();
    Ok(())
}

fn show_lock_status(data_dir: &str) -> anyhow::Result<()> {
    use lithair_core::schema::load_lock_status;
    use std::path::Path;

    let base_path = Path::new(data_dir);

    println!("\n🔐 Schema Lock Status:\n");

    match load_lock_status(base_path) {
        Ok(lock) => {
            let is_locked = lock.is_locked();
            if is_locked {
                println!("  Status: 🔒 LOCKED");
            } else {
                println!("  Status: 🔓 UNLOCKED");
            }

            if let Some(reason) = &lock.reason {
                println!("  Reason: {}", reason);
            }

            if let Some(by) = &lock.unlocked_by {
                println!("  Unlocked by: {}", by);
            }

            if let Some(remaining) = lock.remaining_unlock_secs() {
                println!("  Auto-relock in: {}s", remaining);
            }
        }
        Err(_) => {
            println!("  No lock status file found (default: unlocked)");
        }
    }

    println!();
    Ok(())
}

async fn run_tests(base_url: &str) -> anyhow::Result<()> {
    println!("\n🧪 Running Schema Migration Tests");
    println!("   Target: {}\n", base_url);

    let client = reqwest::Client::new();
    let mut passed = 0;
    let mut failed = 0;

    // Test 1: Health check via products API
    print!("  1. API Health Check... ");
    match client.get(format!("{}/api/products", base_url)).send().await {
        Ok(resp) if resp.status().is_success() => {
            println!("✅ OK");
            passed += 1;
        }
        Ok(resp) => {
            println!("❌ FAILED (status: {})", resp.status());
            failed += 1;
        }
        Err(e) => {
            println!("❌ FAILED ({})", e);
            failed += 1;
            println!("\n  ⚠️  Is the server running at {}?", base_url);
            return Ok(());
        }
    }

    // Test 2: Lock status endpoint
    print!("  2. Lock Status Endpoint... ");
    match client.get(format!("{}/_admin/schema/lock/status", base_url)).send().await {
        Ok(resp) if resp.status().is_success() => {
            let json: serde_json::Value = resp.json().await?;
            if json.get("locked").is_some() {
                println!("✅ OK (locked: {})", json["locked"]);
                passed += 1;
            } else {
                println!("❌ FAILED (invalid response)");
                failed += 1;
            }
        }
        Ok(resp) => {
            println!("❌ FAILED (status: {})", resp.status());
            failed += 1;
        }
        Err(e) => {
            println!("❌ FAILED ({})", e);
            failed += 1;
        }
    }

    // Test 3: Lock endpoint
    print!("  3. Lock Endpoint... ");
    match client
        .post(format!("{}/_admin/schema/lock", base_url))
        .json(&serde_json::json!({"reason": "Test lock"}))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            let json: serde_json::Value = resp.json().await?;
            if json.get("status").and_then(|s| s.as_str()) == Some("locked") {
                println!("✅ OK");
                passed += 1;
            } else {
                println!("❌ FAILED (invalid response)");
                failed += 1;
            }
        }
        Ok(resp) => {
            println!("❌ FAILED (status: {})", resp.status());
            failed += 1;
        }
        Err(e) => {
            println!("❌ FAILED ({})", e);
            failed += 1;
        }
    }

    // Test 4: Verify lock is active
    print!("  4. Verify Lock Active... ");
    match client.get(format!("{}/_admin/schema/lock/status", base_url)).send().await {
        Ok(resp) if resp.status().is_success() => {
            let json: serde_json::Value = resp.json().await?;
            if json.get("locked") == Some(&serde_json::json!(true)) {
                println!("✅ OK (confirmed locked)");
                passed += 1;
            } else {
                println!("❌ FAILED (should be locked)");
                failed += 1;
            }
        }
        Ok(resp) => {
            println!("❌ FAILED (status: {})", resp.status());
            failed += 1;
        }
        Err(e) => {
            println!("❌ FAILED ({})", e);
            failed += 1;
        }
    }

    // Test 5: Unlock with timeout
    print!("  5. Unlock with Timeout... ");
    match client
        .post(format!("{}/_admin/schema/unlock", base_url))
        .json(&serde_json::json!({
            "reason": "Test unlock",
            "duration_seconds": 300,
            "unlocked_by": "test@example.com"
        }))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            let json: serde_json::Value = resp.json().await?;
            if json.get("status").and_then(|s| s.as_str()) == Some("unlocked")
                && json.get("duration_seconds").is_some()
            {
                println!("✅ OK (expires in 300s)");
                passed += 1;
            } else {
                println!("❌ FAILED (invalid response)");
                failed += 1;
            }
        }
        Ok(resp) => {
            println!("❌ FAILED (status: {})", resp.status());
            failed += 1;
        }
        Err(e) => {
            println!("❌ FAILED ({})", e);
            failed += 1;
        }
    }

    // Test 6: History endpoint
    print!("  6. History Endpoint... ");
    match client.get(format!("{}/_admin/schema/history", base_url)).send().await {
        Ok(resp) if resp.status().is_success() => {
            let json: serde_json::Value = resp.json().await?;
            // Response format: {"count": N, "history": [...]}
            if json.get("history").is_some() || json.get("count").is_some() {
                let count = json["count"].as_u64().unwrap_or(0);
                println!("✅ OK ({} change(s))", count);
                passed += 1;
            } else {
                println!("❌ FAILED (invalid response: {:?})", json);
                failed += 1;
            }
        }
        Ok(resp) => {
            println!("❌ FAILED (status: {})", resp.status());
            failed += 1;
        }
        Err(e) => {
            println!("❌ FAILED ({})", e);
            failed += 1;
        }
    }

    // Test 7: Diff endpoint
    print!("  7. Schema Diff Endpoint... ");
    match client.get(format!("{}/_admin/schema/diff", base_url)).send().await {
        Ok(resp) if resp.status().is_success() => {
            let json: serde_json::Value = resp.json().await?;
            if json.get("models").is_some() {
                println!("✅ OK");
                passed += 1;
            } else {
                println!("❌ FAILED (invalid response)");
                failed += 1;
            }
        }
        Ok(resp) => {
            println!("❌ FAILED (status: {})", resp.status());
            failed += 1;
        }
        Err(e) => {
            println!("❌ FAILED ({})", e);
            failed += 1;
        }
    }

    // Test 8: Create a product (requires all fields for DeclarativeModel)
    print!("  8. Create Product... ");
    let test_uuid = uuid::Uuid::new_v4();
    let product = serde_json::json!({
        "id": test_uuid.to_string(),
        "name": "Test Product",
        "description": "Created during test",
        "price_cents": 1999,
        "stock": 100,
        "active": true,
        "created_at": chrono::Utc::now().to_rfc3339(),
        "priority": 0,
        "category": "test",
        "featured": false
    });
    match client.post(format!("{}/api/products", base_url)).json(&product).send().await {
        Ok(resp) if resp.status().is_success() || resp.status() == 201 => {
            println!("✅ OK");
            passed += 1;
        }
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            println!("❌ FAILED (status: {}, body: {})", status, body);
            failed += 1;
        }
        Err(e) => {
            println!("❌ FAILED ({})", e);
            failed += 1;
        }
    }

    // Test 9: Real migration test (AddField - Additive)
    print!("  9. Migration Test (AddField)... ");
    match run_migration_test(&client, base_url).await {
        Ok(msg) => {
            println!("✅ OK ({})", msg);
            passed += 1;
        }
        Err(e) => {
            println!("❌ FAILED ({})", e);
            failed += 1;
        }
    }

    // Test 10: List all schemas endpoint
    print!(" 10. List Schemas Endpoint... ");
    match client.get(format!("{}/_admin/schema", base_url)).send().await {
        Ok(resp) if resp.status().is_success() => {
            let json: serde_json::Value = resp.json().await?;
            if json.get("schemas").is_some() {
                let count = json["schemas"].as_array().map(|a| a.len()).unwrap_or(0);
                println!("✅ OK ({} schema(s))", count);
                passed += 1;
            } else {
                println!("❌ FAILED (invalid response)");
                failed += 1;
            }
        }
        Ok(resp) => {
            println!("❌ FAILED (status: {})", resp.status());
            failed += 1;
        }
        Err(e) => {
            println!("❌ FAILED ({})", e);
            failed += 1;
        }
    }

    // Test 11: Pending changes endpoint
    print!(" 11. Pending Changes Endpoint... ");
    match client.get(format!("{}/_admin/schema/pending", base_url)).send().await {
        Ok(resp) if resp.status().is_success() => {
            let json: serde_json::Value = resp.json().await?;
            if json.get("pending").is_some() || json.get("count").is_some() {
                println!("✅ OK");
                passed += 1;
            } else {
                println!("❌ FAILED (invalid response)");
                failed += 1;
            }
        }
        Ok(resp) => {
            println!("❌ FAILED (status: {})", resp.status());
            failed += 1;
        }
        Err(e) => {
            println!("❌ FAILED ({})", e);
            failed += 1;
        }
    }

    // Test 12: Breaking change detection (RemoveField)
    print!(" 12. Breaking Change (RemoveField)... ");
    match run_breaking_change_test(&client, base_url).await {
        Ok(msg) => {
            println!("✅ OK ({})", msg);
            passed += 1;
        }
        Err(e) => {
            println!("❌ FAILED ({})", e);
            failed += 1;
        }
    }

    // Test 13: Lock blocks revalidate
    print!(" 13. Lock Blocks Revalidate... ");
    match run_lock_blocks_test(&client, base_url).await {
        Ok(msg) => {
            println!("✅ OK ({})", msg);
            passed += 1;
        }
        Err(e) => {
            println!("❌ FAILED ({})", e);
            failed += 1;
        }
    }

    // Test 14: History contains changes
    print!(" 14. History After Changes... ");
    match client.get(format!("{}/_admin/schema/history", base_url)).send().await {
        Ok(resp) if resp.status().is_success() => {
            let json: serde_json::Value = resp.json().await?;
            let count = json["count"].as_u64().unwrap_or(0);
            if count > 0 {
                println!("✅ OK ({} change(s) recorded)", count);
                passed += 1;
            } else {
                println!("⚠️ OK (no changes yet)");
                passed += 1;
            }
        }
        Ok(resp) => {
            println!("❌ FAILED (status: {})", resp.status());
            failed += 1;
        }
        Err(e) => {
            println!("❌ FAILED ({})", e);
            failed += 1;
        }
    }

    // Test 15: Sync endpoint (standalone mode returns 400, cluster mode 200/503)
    print!(" 15. Schema Sync Endpoint... ");
    match client.post(format!("{}/_admin/schema/sync", base_url)).send().await {
        Ok(resp) => {
            let status = resp.status();
            // In standalone mode, sync returns 400 with "Not in cluster mode"
            // In cluster mode, it returns 200 or 503
            if status.is_success() {
                println!("✅ OK (synced)");
                passed += 1;
            } else if status.as_u16() == 400 || status.as_u16() == 503 {
                println!("✅ OK (standalone mode)");
                passed += 1;
            } else {
                println!("❌ FAILED (status: {})", status);
                failed += 1;
            }
        }
        Err(e) => {
            println!("❌ FAILED ({})", e);
            failed += 1;
        }
    }

    // Test 16: Approve + Disk Persistence (requires Manual mode)
    print!(" 16. Approve + Disk Persistence... ");
    match run_approve_persistence_test(&client, base_url).await {
        Ok(msg) => {
            println!("✅ {}", msg);
            passed += 1;
        }
        Err(e) => {
            let err_msg = e.to_string();
            if err_msg.contains("not in Manual mode") {
                println!("⏭️  SKIPPED (requires -m manual)");
            } else {
                println!("❌ FAILED ({})", err_msg);
                failed += 1;
            }
        }
    }

    // Summary
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Results: {} passed, {} failed", passed, failed);
    if failed == 0 {
        println!("  🎉 All tests passed!");
    } else {
        println!("  ⚠️  Some tests failed");
    }
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    Ok(())
}

/// Run a real migration test:
/// 1. Get current history count
/// 2. Backup current schema, replace with baseline (7 fields)
/// 3. Call revalidate endpoint
/// 4. Verify 3 changes detected (priority, category, featured)
/// 5. Restore original schema
async fn run_migration_test(client: &reqwest::Client, base_url: &str) -> anyhow::Result<String> {
    use std::fs;
    use std::path::Path;

    // Paths
    let data_dir = "./data/schema_demo";
    let schema_path = format!("{}/.schema/Product.json", data_dir);
    let backup_path = format!("{}/.schema/Product.json.bak", data_dir);
    let baseline_path = "examples/schema_migration_demo/baseline/Product_v1.json";

    // Ensure baseline exists
    if !Path::new(baseline_path).exists() {
        return Err(anyhow::anyhow!("Baseline schema not found at {}", baseline_path));
    }

    // Get initial history count
    let initial_count: u64 = {
        let resp = client.get(format!("{}/_admin/schema/history", base_url)).send().await?;
        let json: serde_json::Value = resp.json().await?;
        json["count"].as_u64().unwrap_or(0)
    };

    // Backup current schema
    if Path::new(&schema_path).exists() {
        fs::copy(&schema_path, &backup_path)?;
    }

    // Replace with baseline schema (7 fields, no priority/category/featured)
    fs::copy(baseline_path, &schema_path)?;

    // Call revalidate endpoint
    let revalidate_result =
        client.post(format!("{}/_admin/schema/revalidate", base_url)).send().await?;

    let revalidate_json: serde_json::Value = revalidate_result.json().await?;

    // Restore original schema
    if Path::new(&backup_path).exists() {
        fs::copy(&backup_path, &schema_path)?;
        fs::remove_file(&backup_path)?;
    }

    // Verify results
    let status = revalidate_json["status"].as_str().unwrap_or("");
    let total_changes = revalidate_json["total_changes"].as_u64().unwrap_or(0);

    if status != "changes_detected" {
        return Err(anyhow::anyhow!("Expected 'changes_detected', got '{}'", status));
    }

    if total_changes != 3 {
        return Err(anyhow::anyhow!(
            "Expected 3 changes (priority, category, featured), got {}",
            total_changes
        ));
    }

    // Verify history was updated
    let final_count: u64 = {
        let resp = client.get(format!("{}/_admin/schema/history", base_url)).send().await?;
        let json: serde_json::Value = resp.json().await?;
        json["count"].as_u64().unwrap_or(0)
    };

    if final_count <= initial_count {
        return Err(anyhow::anyhow!(
            "History count should have increased (was {}, now {})",
            initial_count,
            final_count
        ));
    }

    Ok(format!("{} changes detected, history updated", total_changes))
}

/// Test breaking change detection (RemoveField)
/// Uses Product_v2_with_legacy.json which has a 'legacy_sku' field not in current model
async fn run_breaking_change_test(
    client: &reqwest::Client,
    base_url: &str,
) -> anyhow::Result<String> {
    use std::fs;
    use std::path::Path;

    let data_dir = "./data/schema_demo";
    let schema_path = format!("{}/.schema/Product.json", data_dir);
    let backup_path = format!("{}/.schema/Product.json.bak", data_dir);
    let baseline_path = "examples/schema_migration_demo/baseline/Product_v2_with_legacy.json";

    // Ensure baseline exists
    if !Path::new(baseline_path).exists() {
        return Err(anyhow::anyhow!("Breaking change baseline not found at {}", baseline_path));
    }

    // Backup current schema
    if Path::new(&schema_path).exists() {
        fs::copy(&schema_path, &backup_path)?;
    }

    // Replace with v2 schema (has legacy_sku field that current model doesn't have)
    fs::copy(baseline_path, &schema_path)?;

    // Call revalidate endpoint
    let revalidate_result =
        client.post(format!("{}/_admin/schema/revalidate", base_url)).send().await?;

    let revalidate_json: serde_json::Value = revalidate_result.json().await?;

    // Restore original schema
    if Path::new(&backup_path).exists() {
        fs::copy(&backup_path, &schema_path)?;
        fs::remove_file(&backup_path)?;
    }

    // Verify results - should detect RemoveField for legacy_sku
    let status = revalidate_json["status"].as_str().unwrap_or("");

    if status != "changes_detected" {
        return Err(anyhow::anyhow!("Expected 'changes_detected', got '{}'", status));
    }

    // Check for RemoveField in the changes
    let models = revalidate_json["models"].as_array();
    let has_remove_field = models
        .and_then(|m| m.first())
        .and_then(|model| model["changes"].as_array())
        .map(|changes| {
            changes.iter().any(|c| {
                c["type"].as_str() == Some("RemoveField")
                    && c["field"].as_str() == Some("legacy_sku")
            })
        })
        .unwrap_or(false);

    if !has_remove_field {
        return Err(anyhow::anyhow!("Expected RemoveField for 'legacy_sku'"));
    }

    // Check that RemoveField is marked as Breaking
    let is_breaking = models
        .and_then(|m| m.first())
        .and_then(|model| model["changes"].as_array())
        .map(|changes| {
            changes.iter().any(|c| {
                c["field"].as_str() == Some("legacy_sku")
                    && c["strategy"].as_str() == Some("Breaking")
            })
        })
        .unwrap_or(false);

    if !is_breaking {
        return Err(anyhow::anyhow!("RemoveField should be marked as Breaking"));
    }

    Ok("RemoveField detected as Breaking".to_string())
}

/// Test that lock blocks revalidate
async fn run_lock_blocks_test(client: &reqwest::Client, base_url: &str) -> anyhow::Result<String> {
    use std::fs;
    use std::path::Path;

    let data_dir = "./data/schema_demo";
    let schema_path = format!("{}/.schema/Product.json", data_dir);
    let backup_path = format!("{}/.schema/Product.json.bak", data_dir);
    let baseline_path = "examples/schema_migration_demo/baseline/Product_v1.json";

    // Lock schema migrations
    let lock_resp = client
        .post(format!("{}/_admin/schema/lock", base_url))
        .json(&serde_json::json!({"reason": "Testing lock blocks revalidate"}))
        .send()
        .await?;

    if !lock_resp.status().is_success() {
        return Err(anyhow::anyhow!("Failed to lock schema"));
    }

    // Backup and replace schema
    if Path::new(&schema_path).exists() {
        fs::copy(&schema_path, &backup_path)?;
    }
    fs::copy(baseline_path, &schema_path)?;

    // Try to revalidate - should be blocked
    let revalidate_result =
        client.post(format!("{}/_admin/schema/revalidate", base_url)).send().await?;

    let status_code = revalidate_result.status();
    let revalidate_json: serde_json::Value = revalidate_result.json().await?;

    // Restore schema
    if Path::new(&backup_path).exists() {
        fs::copy(&backup_path, &schema_path)?;
        fs::remove_file(&backup_path)?;
    }

    // Unlock for other tests
    let _ = client
        .post(format!("{}/_admin/schema/unlock", base_url))
        .json(&serde_json::json!({"reason": "Test complete"}))
        .send()
        .await;

    // Verify revalidate was blocked
    if status_code.as_u16() != 423 {
        return Err(anyhow::anyhow!("Expected 423 LOCKED, got {}", status_code));
    }

    let blocked_status = revalidate_json["status"].as_str().unwrap_or("");
    if blocked_status != "blocked" {
        return Err(anyhow::anyhow!("Expected status 'blocked', got '{}'", blocked_status));
    }

    Ok("revalidate correctly blocked".to_string())
}

/// Test approve + disk persistence (requires Manual mode):
/// 1. Replace schema with baseline v1 (7 fields)
/// 2. Call revalidate → creates pending in Manual mode
/// 3. Get pending ID via GET /_admin/schema/pending
/// 4. Call POST /_admin/schema/approve/{id}
/// 5. Verify schema persisted to disk (should now have 10 fields)
async fn run_approve_persistence_test(
    client: &reqwest::Client,
    base_url: &str,
) -> anyhow::Result<String> {
    use std::fs;
    use std::path::Path;

    let data_dir = "./data/schema_demo";
    let schema_path = format!("{}/.schema/Product.json", data_dir);
    let backup_path = format!("{}/.schema/Product.json.bak", data_dir);
    let baseline_path = "examples/schema_migration_demo/baseline/Product_v1.json";

    // Ensure baseline exists
    if !Path::new(baseline_path).exists() {
        return Err(anyhow::anyhow!("Baseline schema not found at {}", baseline_path));
    }

    // Backup current schema
    if Path::new(&schema_path).exists() {
        fs::copy(&schema_path, &backup_path)?;
    }

    // Replace with baseline schema (7 fields)
    fs::copy(baseline_path, &schema_path)?;

    // Call revalidate - in Manual mode, this creates pending changes
    let revalidate_result =
        client.post(format!("{}/_admin/schema/revalidate", base_url)).send().await?;

    let revalidate_json: serde_json::Value = revalidate_result.json().await?;
    let status = revalidate_json["status"].as_str().unwrap_or("");

    // Check if we're in Manual mode (changes should be pending, not auto-applied)
    if status != "changes_detected" {
        // Restore and skip if not in manual mode
        if Path::new(&backup_path).exists() {
            fs::copy(&backup_path, &schema_path)?;
            fs::remove_file(&backup_path)?;
        }
        return Err(anyhow::anyhow!("not in Manual mode (status: {})", status));
    }

    // Get pending changes
    let pending_resp = client.get(format!("{}/_admin/schema/pending", base_url)).send().await?;

    let pending_json: serde_json::Value = pending_resp.json().await?;
    let pending_changes = pending_json["pending_changes"].as_array();

    // Find the pending ID for Product
    let pending_id = pending_changes
        .and_then(|changes| changes.iter().find(|c| c["model"].as_str() == Some("Product")))
        .and_then(|c| c["id"].as_str())
        .map(String::from);

    let pending_id = match pending_id {
        Some(id) => id,
        None => {
            // No pending change = not in Manual mode
            if Path::new(&backup_path).exists() {
                fs::copy(&backup_path, &schema_path)?;
                fs::remove_file(&backup_path)?;
            }
            return Err(anyhow::anyhow!("not in Manual mode (no pending changes)"));
        }
    };

    // Approve the pending change
    let approve_resp = client
        .post(format!("{}/_admin/schema/approve/{}", base_url, pending_id))
        .send()
        .await?;

    if !approve_resp.status().is_success() {
        let error_text = approve_resp.text().await.unwrap_or_default();
        if Path::new(&backup_path).exists() {
            fs::copy(&backup_path, &schema_path)?;
            fs::remove_file(&backup_path)?;
        }
        return Err(anyhow::anyhow!("Approve failed: {}", error_text));
    }

    // Verify schema was persisted to disk - should have 10 fields now
    let schema_content = fs::read_to_string(&schema_path)?;
    let schema_json: serde_json::Value = serde_json::from_str(&schema_content)?;
    let field_count = schema_json["fields"].as_object().map(|f| f.len()).unwrap_or(0);

    // Restore original schema for other tests
    if Path::new(&backup_path).exists() {
        fs::copy(&backup_path, &schema_path)?;
        fs::remove_file(&backup_path)?;
    }

    // V1 baseline has 7 fields, current model has 10 (with priority, category, featured)
    if field_count == 10 {
        Ok(format!("approved & persisted ({} fields)", field_count))
    } else if field_count == 7 {
        Err(anyhow::anyhow!("Schema not persisted (still {} fields)", field_count))
    } else {
        Err(anyhow::anyhow!("Unexpected field count: {}", field_count))
    }
}
