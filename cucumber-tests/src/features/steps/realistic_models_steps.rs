//! BDD step definitions for production-realistic model types (issue #105).
//!
//! These steps exercise `DeclarativeHttpHandler<T>` directly (no HTTP
//! server), mirroring the retention BDD pattern (issue #97): one fresh
//! handler + temp dir per scenario, `new_with_replay` against the same
//! path for restart scenarios, and env-var overrides set before handler
//! creation (scenarios run serially — `max_concurrent_scenarios(Some(1))`).
//!
//! Listing/filtering/paging assertions go through
//! `DeclarativeHttpHandler::list_response_json`, the production pipeline
//! extracted from `handle_list` (the HTTP entry point itself takes
//! `Request<hyper::body::Incoming>`, which has no public constructor).
//!
//! Models covered (declared in `world.rs`): `BddInvoice` (decimal money,
//! nested lines), `BddDocument` (>100KB blobs x byte-budget retention),
//! `BddUser` (timestamps, optional bio, role enum, validated unique email),
//! `BddOrder` (nested decimal lines, status enum, engine-level FK
//! auto-join via `AutoJoiner`).

use crate::features::world::{
    BddBudgetOnlyDoc, BddDocument, BddInvoice, BddInvoiceLine, BddModelSlot, BddOrder,
    BddOrderLine, BddOrderStatus, BddUser, BddUserRole, LithairWorld,
};
use chrono::{DateTime, Utc};
use cucumber::{given, then, when};
use lithair_core::consensus::ReplicatedModel;
use lithair_core::engine::{AutoJoiner, DataSource, RelationRegistry};
use lithair_core::http::{DeclarativeHttpHandler, HttpExposable};
use lithair_core::lifecycle::{LifecycleAware, RetentionAware};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

// ==================== shared helpers ====================

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap_or_else(|e| panic!("invalid decimal literal {:?}: {}", s, e))
}

fn ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .unwrap_or_else(|e| panic!("invalid RFC3339 timestamp {:?}: {}", s, e))
        .with_timezone(&Utc)
}

fn crc32(data: &str) -> u32 {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(data.as_bytes());
    hasher.finalize()
}

/// Deterministic pseudo-markdown content of exactly `kb * 1024` bytes,
/// seeded by `seed` so each document's content (and CRC32) is unique.
fn make_content(kb: usize, seed: &str) -> String {
    let line = format!("## section {} -- lorem ipsum dolor sit amet 0123456789\n", seed);
    let target = kb * 1024;
    let mut content = String::with_capacity(target + line.len());
    while content.len() < target {
        content.push_str(&line);
    }
    content.truncate(target);
    content
}

/// Remove every BDD-model retention env override so scenarios start from the
/// models' static annotations. Set BEFORE handler creation (`new()` reads
/// the env once). `set_var`/`remove_var` are unsafe since Rust 1.80 because
/// env mutation races in multi-threaded programs; the cucumber runner is
/// serialized (`max_concurrent_scenarios(Some(1))`), so this is controlled.
fn clear_bdd_env() {
    for model in ["BDDINVOICE", "BDDDOCUMENT", "BDDBUDGETONLYDOC", "BDDUSER", "BDDORDER"] {
        for dim in ["MEMORY_RETENTION", "MEMORY_DURATION", "MEMORY_MAX_MB"] {
            unsafe {
                std::env::remove_var(format!("LT_{}_{}", model, dim));
            }
        }
    }
}

/// Create a fresh handler slot backed by a brand-new temp dir.
async fn fresh_slot<T>() -> BddModelSlot<T>
where
    T: HttpExposable + LifecycleAware + ReplicatedModel + RetentionAware,
{
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let path = temp_dir.path().to_string_lossy().to_string();
    let handler = DeclarativeHttpHandler::<T>::new_with_replay(&path)
        .await
        .expect("create handler");
    BddModelSlot { handler: Some(Arc::new(handler)), temp_dir: Some(temp_dir), path: Some(path) }
}

/// Drop the slot's handler and recreate it against the same event-store
/// path, exercising restart + replay.
async fn restart_slot<T>(slot: &mut BddModelSlot<T>)
where
    T: HttpExposable + LifecycleAware + ReplicatedModel + RetentionAware,
{
    let path = slot.path.clone().expect("no event-store path recorded for restart");
    slot.handler = None;
    let handler = DeclarativeHttpHandler::<T>::new_with_replay(&path)
        .await
        .expect("recreate handler on restart");
    slot.handler = Some(Arc::new(handler));
}

fn assert_list_shape(resp: &serde_json::Value, expected_results: usize, expected_total: u64) {
    let data = resp["data"].as_array().expect("list response must carry a data array");
    assert_eq!(
        data.len(),
        expected_results,
        "expected {} results in page, got {} (response: {})",
        expected_results,
        data.len(),
        resp
    );
    let total = resp["total"].as_u64().expect("list response must carry a numeric total");
    assert_eq!(total, expected_total, "expected total {}, got {}", expected_total, total);
}

// ==================== fresh-store givens ====================

#[given("a fresh BDD invoice store")]
async fn given_invoice_store(world: &mut LithairWorld) {
    clear_bdd_env();
    world.bdd_invoices = fresh_slot::<BddInvoice>().await;
}

#[given("a fresh BDD document store")]
async fn given_document_store(world: &mut LithairWorld) {
    clear_bdd_env();
    world.bdd_content_checksums.clear();
    world.bdd_documents = fresh_slot::<BddDocument>().await;
}

#[given(expr = "a fresh BDD document store with a memory budget of {int} MB")]
async fn given_document_store_budget(world: &mut LithairWorld, mb: usize) {
    clear_bdd_env();
    world.bdd_content_checksums.clear();
    // Env override read once by DeclarativeHttpHandler::new(); the model's
    // static #[retention(memory = 999999)] activates the layer, the env
    // budget makes byte-based eviction kick in.
    unsafe {
        std::env::set_var("LT_BDDDOCUMENT_MEMORY_MAX_MB", mb.to_string());
    }
    world.bdd_documents = fresh_slot::<BddDocument>().await;
}

#[given("a fresh BDD budget-only document store")]
async fn given_budget_only_store(world: &mut LithairWorld) {
    clear_bdd_env();
    world.bdd_budget_docs = fresh_slot::<BddBudgetOnlyDoc>().await;
}

#[given("a fresh BDD user store")]
async fn given_user_store(world: &mut LithairWorld) {
    clear_bdd_env();
    world.bdd_users = fresh_slot::<BddUser>().await;
}

#[given("a fresh BDD order store")]
async fn given_order_store(world: &mut LithairWorld) {
    clear_bdd_env();
    world.bdd_orders = fresh_slot::<BddOrder>().await;
}

// ==================== restart steps ====================

#[when("the invoice store is restarted")]
async fn when_invoice_restart(world: &mut LithairWorld) {
    restart_slot(&mut world.bdd_invoices).await;
}

#[when("the document store is restarted")]
async fn when_document_restart(world: &mut LithairWorld) {
    restart_slot(&mut world.bdd_documents).await;
}

#[when("the user store is restarted")]
async fn when_user_restart(world: &mut LithairWorld) {
    restart_slot(&mut world.bdd_users).await;
}

#[when("the order store is restarted")]
async fn when_order_restart(world: &mut LithairWorld) {
    restart_slot(&mut world.bdd_orders).await;
}

// ==================== invoice steps ====================

#[when(
    expr = "I create invoice {string} in {string} with total {string}, tax {string}, due {string} and {int} lines"
)]
async fn when_create_invoice(
    world: &mut LithairWorld,
    id: String,
    currency: String,
    total: String,
    tax: String,
    due: String,
    line_count: usize,
) {
    let handler = world.bdd_invoices.handler.clone().expect("invoice store not initialized");
    let lines = (0..line_count)
        .map(|i| BddInvoiceLine {
            description: format!("Line {}", i + 1),
            quantity: (i + 1) as u32,
            unit_price: dec("19.99"),
        })
        .collect();
    let invoice = BddInvoice {
        id: id.clone(),
        invoice_number: format!("NUM-{}", id),
        currency,
        total_amount: dec(&total),
        tax_amount: dec(&tax),
        due_date: ts(&due),
        lines,
    };
    handler.apply_replicated_item(invoice).await.expect("insert invoice");
}

#[when(expr = "I create {int} invoices alternating currencies {string} and {string}")]
async fn when_create_invoices_bulk(
    world: &mut LithairWorld,
    count: usize,
    cur_a: String,
    cur_b: String,
) {
    let handler = world.bdd_invoices.handler.clone().expect("invoice store not initialized");
    for i in 0..count {
        let currency = if i % 2 == 0 { cur_a.clone() } else { cur_b.clone() };
        let invoice = BddInvoice {
            id: format!("inv-{:05}", i),
            invoice_number: format!("INV-{:05}", i),
            currency,
            total_amount: dec(&format!("{}.50", 100 + i)),
            tax_amount: dec("0.05"),
            due_date: ts("2026-07-15T00:00:00Z"),
            lines: vec![BddInvoiceLine {
                description: format!("Bulk line {}", i),
                quantity: 1,
                unit_price: dec("1.00"),
            }],
        };
        handler.apply_replicated_item(invoice).await.expect("insert invoice");
    }
}

#[when(expr = "I update invoice {string} total to {string}")]
async fn when_update_invoice_total(world: &mut LithairWorld, id: String, total: String) {
    let handler = world.bdd_invoices.handler.clone().expect("invoice store not initialized");
    let mut invoice = handler.get_by_id(&id).await.expect("invoice to update not found");
    invoice.total_amount = dec(&total);
    handler.apply_replicated_update(&id, invoice).await.expect("update invoice");
}

#[when(expr = "I delete invoice {string}")]
async fn when_delete_invoice(world: &mut LithairWorld, id: String) {
    let handler = world.bdd_invoices.handler.clone().expect("invoice store not initialized");
    let removed = handler.apply_replicated_delete(&id).await.expect("delete invoice");
    assert!(removed, "invoice {} was not present at delete time", id);
}

#[then(expr = "fetching invoice {string} returns total {string} and tax {string}")]
async fn then_invoice_amounts(world: &mut LithairWorld, id: String, total: String, tax: String) {
    let handler = world.bdd_invoices.handler.clone().expect("invoice store not initialized");
    let invoice = handler
        .get_by_id(&id)
        .await
        .unwrap_or_else(|| panic!("invoice {} not found", id));
    assert_eq!(invoice.total_amount, dec(&total), "total_amount mismatch for {}", id);
    assert_eq!(invoice.tax_amount, dec(&tax), "tax_amount mismatch for {}", id);
    // Precision check at the serialization layer too: Decimal must come back
    // as the exact same decimal string, not a float approximation.
    let json = serde_json::to_value(&invoice).expect("serialize invoice");
    assert_eq!(
        json["total_amount"],
        serde_json::Value::String(total.clone()),
        "serialized total_amount drifted for {}",
        id
    );
}

#[then(expr = "invoice {string} has {int} lines with unit price {string}")]
async fn then_invoice_lines(world: &mut LithairWorld, id: String, count: usize, price: String) {
    let handler = world.bdd_invoices.handler.clone().expect("invoice store not initialized");
    let invoice = handler
        .get_by_id(&id)
        .await
        .unwrap_or_else(|| panic!("invoice {} not found", id));
    assert_eq!(invoice.lines.len(), count, "line count mismatch for {}", id);
    for (i, line) in invoice.lines.iter().enumerate() {
        assert_eq!(line.unit_price, dec(&price), "unit_price mismatch on line {} of {}", i + 1, id);
        assert_eq!(line.quantity, (i + 1) as u32, "quantity mismatch on line {} of {}", i + 1, id);
    }
}

#[then(expr = "invoice {string} no longer exists")]
async fn then_invoice_gone(world: &mut LithairWorld, id: String) {
    let handler = world.bdd_invoices.handler.clone().expect("invoice store not initialized");
    assert!(handler.get_by_id(&id).await.is_none(), "invoice {} still exists", id);
}

#[then(expr = "the decimal sum of total and tax for invoice {string} is exactly {string}")]
async fn then_invoice_decimal_sum(world: &mut LithairWorld, id: String, expected: String) {
    let handler = world.bdd_invoices.handler.clone().expect("invoice store not initialized");
    let invoice = handler
        .get_by_id(&id)
        .await
        .unwrap_or_else(|| panic!("invoice {} not found", id));
    let sum = invoice.total_amount + invoice.tax_amount;
    assert_eq!(
        sum,
        dec(&expected),
        "decimal sum mismatch for {}: {} + {} = {} (expected {})",
        id,
        invoice.total_amount,
        invoice.tax_amount,
        sum,
        expected
    );
}

#[then(expr = "listing invoices with query {string} returns {int} results and total {int}")]
async fn then_list_invoices(world: &mut LithairWorld, query: String, results: usize, total: u64) {
    let handler = world.bdd_invoices.handler.clone().expect("invoice store not initialized");
    let resp = handler.list_response_json(&query, &[]).await;
    assert_list_shape(&resp, results, total);
    world.bdd_last_list = Some(resp);
}

#[then(expr = "every listed invoice has {string} equal to {string}")]
async fn then_listed_invoices_field(world: &mut LithairWorld, field: String, value: String) {
    let resp = world.bdd_last_list.as_ref().expect("no list response recorded");
    let data = resp["data"].as_array().expect("data array");
    assert!(!data.is_empty(), "list is empty, nothing to verify");
    for item in data {
        assert_eq!(
            item[&field].as_str(),
            Some(value.as_str()),
            "item {} has {}={:?}, expected {:?}",
            item["id"],
            field,
            item[&field],
            value
        );
    }
}

#[then(expr = "the listed invoice numbers are {string}")]
async fn then_listed_invoice_numbers(world: &mut LithairWorld, expected_csv: String) {
    let resp = world.bdd_last_list.as_ref().expect("no list response recorded");
    let data = resp["data"].as_array().expect("data array");
    let numbers: Vec<&str> =
        data.iter().filter_map(|item| item["invoice_number"].as_str()).collect();
    let expected: Vec<&str> = expected_csv.split(',').collect();
    assert_eq!(numbers, expected, "paged invoice numbers mismatch");
}

#[then(expr = "the invoice listing reports has_more {string}")]
async fn then_invoice_has_more(world: &mut LithairWorld, expected: String) {
    let resp = world.bdd_last_list.as_ref().expect("no list response recorded");
    let expected_bool = expected == "true";
    assert_eq!(
        resp["has_more"].as_bool(),
        Some(expected_bool),
        "has_more mismatch (response: {})",
        resp
    );
}

// ==================== document steps ====================

#[when(
    expr = "I create document {string} titled {string} version {int} with {int} KB of content and tags {string}"
)]
async fn when_create_document(
    world: &mut LithairWorld,
    id: String,
    title: String,
    version: u32,
    kb: usize,
    tags_csv: String,
) {
    let handler = world.bdd_documents.handler.clone().expect("document store not initialized");
    let content = make_content(kb, &id);
    world.bdd_content_checksums.insert(id.clone(), crc32(&content));
    let doc = BddDocument {
        id,
        title,
        version,
        tags: tags_csv.split(',').map(|t| t.trim().to_string()).collect(),
        content,
    };
    handler.apply_replicated_item(doc).await.expect("insert document");
}

#[when(expr = "I create {int} documents of {int} KB each")]
async fn when_create_documents_bulk(world: &mut LithairWorld, count: usize, kb: usize) {
    let handler = world.bdd_documents.handler.clone().expect("document store not initialized");
    for i in 0..count {
        let id = format!("doc-{:05}", i);
        let content = make_content(kb, &id);
        world.bdd_content_checksums.insert(id.clone(), crc32(&content));
        let doc = BddDocument {
            id: id.clone(),
            title: format!("Document {}", i),
            version: 1,
            tags: vec!["bulk".to_string()],
            content,
        };
        handler.apply_replicated_item(doc).await.expect("insert document");
    }
}

#[when(expr = "I update document {string} to version {int}")]
async fn when_update_document_version(world: &mut LithairWorld, id: String, version: u32) {
    let handler = world.bdd_documents.handler.clone().expect("document store not initialized");
    let mut doc = handler.get_by_id(&id).await.expect("document to update not found");
    doc.version = version;
    handler.apply_replicated_update(&id, doc).await.expect("update document");
}

#[when(expr = "I delete document {string}")]
async fn when_delete_document(world: &mut LithairWorld, id: String) {
    let handler = world.bdd_documents.handler.clone().expect("document store not initialized");
    let removed = handler.apply_replicated_delete(&id).await.expect("delete document");
    assert!(removed, "document {} was not present at delete time", id);
}

#[then(expr = "fetching document {string} returns title {string} and version {int}")]
async fn then_document_fields(world: &mut LithairWorld, id: String, title: String, version: u32) {
    let handler = world.bdd_documents.handler.clone().expect("document store not initialized");
    let doc = handler
        .get_by_id(&id)
        .await
        .unwrap_or_else(|| panic!("document {} not found", id));
    assert_eq!(doc.title, title, "title mismatch for {}", id);
    assert_eq!(doc.version, version, "version mismatch for {}", id);
}

#[then(expr = "the content of document {string} is intact and {int} bytes long")]
async fn then_document_content_intact(world: &mut LithairWorld, id: String, bytes: usize) {
    let handler = world.bdd_documents.handler.clone().expect("document store not initialized");
    let doc = handler
        .get_by_id(&id)
        .await
        .unwrap_or_else(|| panic!("document {} not found", id));
    assert_eq!(doc.content.len(), bytes, "content length mismatch for {}", id);
    let expected_crc =
        *world.bdd_content_checksums.get(&id).expect("no checksum recorded for document");
    assert_eq!(crc32(&doc.content), expected_crc, "content CRC32 mismatch for {}", id);
}

#[then(expr = "document {string} no longer exists")]
async fn then_document_gone(world: &mut LithairWorld, id: String) {
    let handler = world.bdd_documents.handler.clone().expect("document store not initialized");
    assert!(handler.get_by_id(&id).await.is_none(), "document {} still exists", id);
}

#[then(expr = "listing documents with query {string} returns {int} results and total {int}")]
async fn then_list_documents(world: &mut LithairWorld, query: String, results: usize, total: u64) {
    let handler = world.bdd_documents.handler.clone().expect("document store not initialized");
    let resp = handler.list_response_json(&query, &[]).await;
    assert_list_shape(&resp, results, total);
    world.bdd_last_list = Some(resp);
}

#[then(expr = "fewer than {int} documents are fully in memory")]
async fn then_documents_hot_below(world: &mut LithairWorld, limit: usize) {
    let handler = world.bdd_documents.handler.clone().expect("document store not initialized");
    let hot = handler.storage_count().await;
    assert!(hot < limit, "expected fewer than {} hot documents, found {}", limit, hot);
}

#[then(expr = "at least {int} documents are evicted")]
async fn then_documents_evicted_at_least(world: &mut LithairWorld, minimum: usize) {
    let handler = world.bdd_documents.handler.clone().expect("document store not initialized");
    let total = handler.total_item_count().await;
    let hot = handler.storage_count().await;
    let evicted = total - hot;
    assert!(
        evicted >= minimum,
        "expected at least {} evicted documents, found {} (total={}, hot={})",
        minimum,
        evicted,
        total,
        hot
    );
}

#[then("every evicted document reloads intact from the event store")]
async fn then_evicted_documents_intact(world: &mut LithairWorld) {
    let handler = world.bdd_documents.handler.clone().expect("document store not initialized");
    let hot_ids: std::collections::HashSet<String> =
        handler.get_all_items().await.into_iter().map(|d| d.id).collect();
    let mut verified = 0usize;
    for (id, expected_crc) in world.bdd_content_checksums.iter() {
        if hot_ids.contains(id) {
            continue; // not evicted
        }
        let doc = handler
            .get_by_id(id)
            .await
            .unwrap_or_else(|| panic!("evicted document {} not loadable from event store", id));
        assert_eq!(crc32(&doc.content), *expected_crc, "evicted document {} content corrupted", id);
        verified += 1;
    }
    assert!(verified > 0, "no documents were evicted — the scenario setup is wrong");
}

#[then(expr = "all {int} documents are accessible with intact content")]
async fn then_all_documents_intact(world: &mut LithairWorld, count: usize) {
    let handler = world.bdd_documents.handler.clone().expect("document store not initialized");
    assert_eq!(
        world.bdd_content_checksums.len(),
        count,
        "scenario recorded {} checksums but expects {} documents",
        world.bdd_content_checksums.len(),
        count
    );
    assert_eq!(
        handler.total_item_count().await,
        count,
        "total item count mismatch after replay"
    );
    for (id, expected_crc) in world.bdd_content_checksums.iter() {
        let doc = handler
            .get_by_id(id)
            .await
            .unwrap_or_else(|| panic!("document {} not accessible after replay", id));
        assert_eq!(crc32(&doc.content), *expected_crc, "document {} content corrupted", id);
    }
}

// Budget-only model steps (@bug @deferred scenario — see document.feature).

#[when(expr = "I create {int} budget-only documents of {int} KB each")]
async fn when_create_budget_docs(world: &mut LithairWorld, count: usize, kb: usize) {
    let handler = world
        .bdd_budget_docs
        .handler
        .clone()
        .expect("budget-only store not initialized");
    for i in 0..count {
        let id = format!("bdoc-{:05}", i);
        let doc = BddBudgetOnlyDoc { id: id.clone(), content: make_content(kb, &id) };
        handler.apply_replicated_item(doc).await.expect("insert budget-only document");
    }
}

#[then(expr = "at most {int} budget-only documents are fully in memory")]
async fn then_budget_docs_hot_at_most(world: &mut LithairWorld, limit: usize) {
    let handler = world
        .bdd_budget_docs
        .handler
        .clone()
        .expect("budget-only store not initialized");
    let hot = handler.storage_count().await;
    assert!(
        hot <= limit,
        "expected at most {} hot budget-only documents, found {} — \
         #[retention(max_mb)] without a memory count is being ignored",
        limit,
        hot
    );
}

// ==================== user steps ====================

fn parse_role(role: &str) -> BddUserRole {
    serde_json::from_value(serde_json::Value::String(role.to_string()))
        .unwrap_or_else(|e| panic!("invalid role {:?}: {}", role, e))
}

#[when(
    expr = "I create user {string} with email {string}, name {string}, role {string}, bio {string} and created at {string}"
)]
async fn when_create_user_full(
    world: &mut LithairWorld,
    id: String,
    email: String,
    name: String,
    role: String,
    bio: String,
    created_at: String,
) {
    let handler = world.bdd_users.handler.clone().expect("user store not initialized");
    let user = BddUser {
        id,
        email,
        display_name: name,
        role: parse_role(&role),
        bio: Some(bio),
        created_at: ts(&created_at),
    };
    handler.apply_replicated_item(user).await.expect("insert user");
}

#[when(expr = "I create user {string} with email {string}, name {string} and role {string}")]
async fn when_create_user_minimal(
    world: &mut LithairWorld,
    id: String,
    email: String,
    name: String,
    role: String,
) {
    let handler = world.bdd_users.handler.clone().expect("user store not initialized");
    let user = BddUser {
        id,
        email,
        display_name: name,
        role: parse_role(&role),
        bio: None,
        created_at: ts("2026-01-01T00:00:00Z"),
    };
    handler.apply_replicated_item(user).await.expect("insert user");
}

#[when(expr = "I create {int} users with roles alternating {string} and {string}")]
async fn when_create_users_bulk(
    world: &mut LithairWorld,
    count: usize,
    role_a: String,
    role_b: String,
) {
    let handler = world.bdd_users.handler.clone().expect("user store not initialized");
    for i in 0..count {
        let role = if i % 2 == 0 { parse_role(&role_a) } else { parse_role(&role_b) };
        let user = BddUser {
            id: format!("user-{:05}", i),
            email: format!("user{}@example.com", i),
            display_name: format!("User {}", i),
            role,
            bio: None,
            created_at: ts("2026-01-01T00:00:00Z"),
        };
        handler.apply_replicated_item(user).await.expect("insert user");
    }
}

#[when(expr = "I change user {string} role to {string}")]
async fn when_change_user_role(world: &mut LithairWorld, id: String, role: String) {
    let handler = world.bdd_users.handler.clone().expect("user store not initialized");
    let mut user = handler.get_by_id(&id).await.expect("user to update not found");
    user.role = parse_role(&role);
    handler.apply_replicated_update(&id, user).await.expect("update user");
}

#[when(expr = "I delete user {string}")]
async fn when_delete_user(world: &mut LithairWorld, id: String) {
    let handler = world.bdd_users.handler.clone().expect("user store not initialized");
    let removed = handler.apply_replicated_delete(&id).await.expect("delete user");
    assert!(removed, "user {} was not present at delete time", id);
}

#[when(expr = "I try to change user {string} email to {string}")]
async fn when_try_change_email(world: &mut LithairWorld, id: String, email: String) {
    let handler = world.bdd_users.handler.clone().expect("user store not initialized");
    // submit_admin_edit is the public path that runs check_unique_constraints
    // (+ validate + immutability) — the same enforcement handle_update applies.
    let result = handler.submit_admin_edit(&id, serde_json::json!({ "email": email })).await;
    world.bdd_last_error = result.err();
}

#[when(expr = "I validate a user with email {string}")]
async fn when_validate_user_email(world: &mut LithairWorld, email: String) {
    // validate() is the derive-generated HttpExposable method that the HTTP
    // create/update paths call; exercising it directly keeps the assertion
    // on the production rule (#[http(validate = "email")]).
    let user = BddUser {
        id: "validation-probe".to_string(),
        email,
        display_name: "Probe".to_string(),
        role: BddUserRole::Viewer,
        bio: None,
        created_at: ts("2026-01-01T00:00:00Z"),
    };
    world.bdd_last_error = user.validate().err();
}

#[then(expr = "the user validation fails mentioning {string}")]
async fn then_validation_fails(world: &mut LithairWorld, needle: String) {
    let err = world.bdd_last_error.as_ref().expect("expected a validation error, got success");
    assert!(
        err.to_lowercase().contains(&needle.to_lowercase()),
        "validation error {:?} does not mention {:?}",
        err,
        needle
    );
}

#[then("the user validation succeeds")]
async fn then_validation_ok(world: &mut LithairWorld) {
    assert!(
        world.bdd_last_error.is_none(),
        "expected validation success, got error: {:?}",
        world.bdd_last_error
    );
}

#[then(expr = "the user edit is rejected with a unique constraint error on {string}")]
async fn then_unique_rejected(world: &mut LithairWorld, field: String) {
    let err = world
        .bdd_last_error
        .as_ref()
        .expect("expected a unique-constraint error, got success");
    assert!(
        err.contains("must be unique"),
        "error {:?} is not a unique-constraint rejection",
        err
    );
    assert!(
        err.contains(&format!("'{}'", field)),
        "error {:?} does not name field {}",
        err,
        field
    );
}

#[then(expr = "fetching user {string} returns email {string}, role {string} and bio {string}")]
async fn then_user_fields_with_bio(
    world: &mut LithairWorld,
    id: String,
    email: String,
    role: String,
    bio: String,
) {
    let handler = world.bdd_users.handler.clone().expect("user store not initialized");
    let user = handler.get_by_id(&id).await.unwrap_or_else(|| panic!("user {} not found", id));
    assert_eq!(user.email, email, "email mismatch for {}", id);
    assert_eq!(user.role, parse_role(&role), "role mismatch for {}", id);
    assert_eq!(user.bio.as_deref(), Some(bio.as_str()), "bio mismatch for {}", id);
}

#[then(expr = "fetching user {string} returns email {string}, role {string} and no bio")]
async fn then_user_fields_no_bio(
    world: &mut LithairWorld,
    id: String,
    email: String,
    role: String,
) {
    let handler = world.bdd_users.handler.clone().expect("user store not initialized");
    let user = handler.get_by_id(&id).await.unwrap_or_else(|| panic!("user {} not found", id));
    assert_eq!(user.email, email, "email mismatch for {}", id);
    assert_eq!(user.role, parse_role(&role), "role mismatch for {}", id);
    assert_eq!(user.bio, None, "expected no bio for {}, found {:?}", id, user.bio);
}

#[then(expr = "fetching user {string} returns a null bio")]
async fn then_user_null_bio(world: &mut LithairWorld, id: String) {
    let handler = world.bdd_users.handler.clone().expect("user store not initialized");
    let user = handler.get_by_id(&id).await.unwrap_or_else(|| panic!("user {} not found", id));
    // The JSON representation must carry an explicit null, not "" or absence.
    let json = serde_json::to_value(&user).expect("serialize user");
    assert!(json["bio"].is_null(), "expected bio to be JSON null, got {:?}", json["bio"]);
}

#[then(expr = "user {string} no longer exists")]
async fn then_user_gone(world: &mut LithairWorld, id: String) {
    let handler = world.bdd_users.handler.clone().expect("user store not initialized");
    assert!(handler.get_by_id(&id).await.is_none(), "user {} still exists", id);
}

#[then(expr = "user {string} has created_at exactly {string}")]
async fn then_user_created_at(world: &mut LithairWorld, id: String, expected: String) {
    let handler = world.bdd_users.handler.clone().expect("user store not initialized");
    let user = handler.get_by_id(&id).await.unwrap_or_else(|| panic!("user {} not found", id));
    let expected_ts = ts(&expected);
    assert_eq!(
        user.created_at, expected_ts,
        "created_at drifted for {}: stored {} vs expected {}",
        id, user.created_at, expected_ts
    );
    // Sub-second fidelity: the millisecond component must survive replay.
    assert_eq!(
        user.created_at.timestamp_subsec_millis(),
        expected_ts.timestamp_subsec_millis(),
        "sub-second precision lost for {}",
        id
    );
}

#[then(expr = "listing users with query {string} returns {int} results and total {int}")]
async fn then_list_users(world: &mut LithairWorld, query: String, results: usize, total: u64) {
    let handler = world.bdd_users.handler.clone().expect("user store not initialized");
    let resp = handler.list_response_json(&query, &[]).await;
    assert_list_shape(&resp, results, total);
    world.bdd_last_list = Some(resp);
}

// ==================== order steps ====================

fn parse_status(status: &str) -> BddOrderStatus {
    serde_json::from_value(serde_json::Value::String(status.to_string()))
        .unwrap_or_else(|e| panic!("invalid order status {:?}: {}", status, e))
}

#[when(
    expr = "I create order {string} for user {string} and product {string} with status {string} and {int} lines at unit price {string}"
)]
async fn when_create_order(
    world: &mut LithairWorld,
    id: String,
    user_id: String,
    product_id: String,
    status: String,
    line_count: usize,
    price: String,
) {
    let handler = world.bdd_orders.handler.clone().expect("order store not initialized");
    let lines = (0..line_count)
        .map(|i| BddOrderLine {
            product_id: product_id.clone(),
            quantity: (i + 1) as u32,
            unit_price: dec(&price),
        })
        .collect();
    let order = BddOrder { id, user_id, product_id, status: parse_status(&status), lines };
    handler.apply_replicated_item(order).await.expect("insert order");
}

#[when(expr = "I create {int} orders alternating statuses {string} and {string}")]
async fn when_create_orders_bulk(
    world: &mut LithairWorld,
    count: usize,
    status_a: String,
    status_b: String,
) {
    let handler = world.bdd_orders.handler.clone().expect("order store not initialized");
    for i in 0..count {
        let status = if i % 2 == 0 { parse_status(&status_a) } else { parse_status(&status_b) };
        let order = BddOrder {
            id: format!("ord-{:05}", i),
            user_id: format!("user-{:05}", i),
            product_id: format!("prod-{:05}", i),
            status,
            lines: vec![BddOrderLine {
                product_id: format!("prod-{:05}", i),
                quantity: 1,
                unit_price: dec("2.50"),
            }],
        };
        handler.apply_replicated_item(order).await.expect("insert order");
    }
}

#[when(expr = "I change order {string} status to {string}")]
async fn when_change_order_status(world: &mut LithairWorld, id: String, status: String) {
    let handler = world.bdd_orders.handler.clone().expect("order store not initialized");
    let mut order = handler.get_by_id(&id).await.expect("order to update not found");
    order.status = parse_status(&status);
    handler.apply_replicated_update(&id, order).await.expect("update order");
}

#[when(expr = "I delete order {string}")]
async fn when_delete_order(world: &mut LithairWorld, id: String) {
    let handler = world.bdd_orders.handler.clone().expect("order store not initialized");
    let removed = handler.apply_replicated_delete(&id).await.expect("delete order");
    assert!(removed, "order {} was not present at delete time", id);
}

#[then(expr = "fetching order {string} returns status {string} and {int} lines")]
async fn then_order_fields(world: &mut LithairWorld, id: String, status: String, count: usize) {
    let handler = world.bdd_orders.handler.clone().expect("order store not initialized");
    let order = handler.get_by_id(&id).await.unwrap_or_else(|| panic!("order {} not found", id));
    assert_eq!(order.status, parse_status(&status), "status mismatch for {}", id);
    assert_eq!(order.lines.len(), count, "line count mismatch for {}", id);
}

#[then(expr = "order {string} line {int} has product {string} and unit price {string}")]
async fn then_order_line(
    world: &mut LithairWorld,
    id: String,
    line_no: usize,
    product: String,
    price: String,
) {
    let handler = world.bdd_orders.handler.clone().expect("order store not initialized");
    let order = handler.get_by_id(&id).await.unwrap_or_else(|| panic!("order {} not found", id));
    let line = order
        .lines
        .get(line_no - 1)
        .unwrap_or_else(|| panic!("order {} has no line {}", id, line_no));
    assert_eq!(line.product_id, product, "product mismatch on line {} of {}", line_no, id);
    assert_eq!(
        line.unit_price,
        dec(&price),
        "unit_price mismatch on line {} of {}",
        line_no,
        id
    );
}

#[then(expr = "order {string} no longer exists")]
async fn then_order_gone(world: &mut LithairWorld, id: String) {
    let handler = world.bdd_orders.handler.clone().expect("order store not initialized");
    assert!(handler.get_by_id(&id).await.is_none(), "order {} still exists", id);
}

#[then(expr = "listing orders with query {string} returns {int} results and total {int}")]
async fn then_list_orders(world: &mut LithairWorld, query: String, results: usize, total: u64) {
    let handler = world.bdd_orders.handler.clone().expect("order store not initialized");
    let resp = handler.list_response_json(&query, &[]).await;
    assert_list_shape(&resp, results, total);
    world.bdd_last_list = Some(resp);
}

// ==================== FK auto-join (engine level) ====================

/// Snapshot-backed DataSource adapter — the same shape production users
/// implement (or get from Scc2Engine). The joined payloads come from the
/// REAL BddUser handler, so the scenario covers: #[db(fk = "...")] →
/// derive-generated ModelSpec policy → AutoJoiner expansion.
struct MapSource(HashMap<String, serde_json::Value>);

impl DataSource for MapSource {
    fn fetch_by_id(&self, id: &str) -> Option<serde_json::Value> {
        self.0.get(id).cloned()
    }
}

#[then(expr = "the order model reports field {string} as a foreign key to {string}")]
async fn then_order_fk_policy(_world: &mut LithairWorld, field: String, collection: String) {
    use lithair_core::model::ModelSpec;
    // get_policy is an instance method reading the derive-generated static
    // spec — any instance will do.
    let probe = BddOrder {
        id: "policy-probe".to_string(),
        user_id: String::new(),
        product_id: String::new(),
        status: BddOrderStatus::Pending,
        lines: Vec::new(),
    };
    let policy = probe
        .get_policy(&field)
        .unwrap_or_else(|| panic!("model reports no policy at all for field {:?}", field));
    assert!(
        policy.fk,
        "field {:?} is not marked as a foreign key (policy: fk_collection={:?})",
        field, policy.fk_collection
    );
    assert_eq!(
        policy.fk_collection.as_deref(),
        Some(collection.as_str()),
        "field {:?} fk_collection mismatch",
        field
    );
}

#[when(expr = "I expand order {string} relations with the users source registered")]
async fn when_expand_order(world: &mut LithairWorld, id: String) {
    let users = world.bdd_users.handler.clone().expect("user store not initialized");
    let mut user_map = HashMap::new();
    for user in users.get_all_items().await {
        let json = serde_json::to_value(&user).expect("serialize user");
        user_map.insert(user.id.clone(), json);
    }

    let registry = RelationRegistry::new();
    registry.register("bdd_users", Arc::new(MapSource(user_map)));
    // Deliberately NOT registering "bdd_products": the scenario asserts that
    // an FK pointing at an unregistered collection is left unexpanded.

    let orders = world.bdd_orders.handler.clone().expect("order store not initialized");
    let order = orders.get_by_id(&id).await.unwrap_or_else(|| panic!("order {} not found", id));
    let expanded =
        AutoJoiner::expand(&order, &order, &registry).expect("AutoJoiner expansion failed");
    world.bdd_last_value = Some(expanded);
}

#[then(expr = "the expanded order has a joined {string} object with email {string}")]
async fn then_expanded_join(world: &mut LithairWorld, field: String, email: String) {
    let expanded = world.bdd_last_value.as_ref().expect("no expanded order recorded");
    let joined = expanded
        .get(&field)
        .unwrap_or_else(|| panic!("expanded order has no joined {:?} object: {}", field, expanded));
    assert!(joined.is_object(), "joined {:?} is not an object: {}", field, joined);
    assert_eq!(
        joined["email"].as_str(),
        Some(email.as_str()),
        "joined {} email mismatch",
        field
    );
}

#[then(expr = "the expanded order still carries {string} equal to {string}")]
async fn then_expanded_keeps_fk(world: &mut LithairWorld, field: String, value: String) {
    let expanded = world.bdd_last_value.as_ref().expect("no expanded order recorded");
    assert_eq!(
        expanded[&field].as_str(),
        Some(value.as_str()),
        "original FK field {} was not preserved",
        field
    );
}

#[then(expr = "the expanded order has no joined {string} object")]
async fn then_expanded_no_join(world: &mut LithairWorld, field: String) {
    let expanded = world.bdd_last_value.as_ref().expect("no expanded order recorded");
    assert!(
        expanded.get(&field).is_none(),
        "expected no joined {:?} object (collection not registered), found {}",
        field,
        expanded[&field]
    );
}
