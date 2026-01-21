//! Step definitions for Hash Chain Integrity tests
//!
//! These steps test the SHA256 hash chain implementation for tamper-evident
//! event storage in Lithair.

use crate::features::world::LithairWorld;
use cucumber::{given, then, when};
use lithair_core::engine::{EventEnvelope, EventStore};
use std::fs;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::Mutex;

// ==================== SETUP STEPS ====================

#[given("a Lithair server with hash chain enabled")]
async fn given_hash_chain_enabled(world: &mut LithairWorld) {
    // Ensure hash chain is enabled (default behavior)
    std::env::remove_var("RS_DISABLE_HASH_CHAIN");

    // Initialize temp directory for this test
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    *world.temp_dir.lock().await = Some(temp_dir);

    println!("✅ Hash chain enabled for test");
}

#[given("event persistence is configured")]
async fn given_persistence_configured(world: &mut LithairWorld) {
    // Persistence is configured via temp_dir
    let temp_guard = world.temp_dir.lock().await;
    assert!(temp_guard.is_some(), "Temp dir should be set");
    println!("✅ Event persistence configured");
}

#[given("I have created an initial article (genesis event)")]
async fn given_genesis_event(world: &mut LithairWorld) {
    let temp_guard = world.temp_dir.lock().await;
    let temp_path = temp_guard.as_ref().expect("Temp dir required").path();
    let mut event_store =
        EventStore::new(temp_path.to_str().unwrap()).expect("Failed to create event store");

    let envelope = EventEnvelope::new(
        "ArticleCreated".to_string(),
        "article-genesis-001".to_string(),
        chrono::Utc::now().timestamp() as u64,
        r#"{"id": "001", "title": "Genesis Article"}"#.to_string(),
        Some("article-001".to_string()),
        None, // genesis has no previous_hash
    );

    event_store.append_envelope(&envelope).expect("Failed to append genesis event");
    event_store.flush().expect("Failed to flush");

    // Store count in last_response as JSON
    world.last_response = Some(r#"{"events_created": 1}"#.to_string());
    println!("✅ Genesis event created with hash chain");
}

// ==================== ACTION STEPS ====================

#[when("I create an article via the API")]
async fn when_create_article(world: &mut LithairWorld) {
    let temp_guard = world.temp_dir.lock().await;
    let temp_path = temp_guard.as_ref().expect("Temp dir required").path();

    let mut event_store =
        EventStore::new(temp_path.to_str().unwrap()).expect("Failed to create event store");

    // Get last hash for chain linking
    let last_hash = event_store.get_last_event_hash().cloned();

    let envelope = EventEnvelope::new(
        "ArticleCreated".to_string(),
        format!("article-{}", uuid::Uuid::new_v4()),
        chrono::Utc::now().timestamp() as u64,
        r#"{"id": "test", "title": "Test Article"}"#.to_string(),
        Some("article-test".to_string()),
        last_hash,
    );

    event_store.append_envelope(&envelope).expect("Failed to append event");
    event_store.flush().expect("Failed to flush");

    // Update event count
    let current_count = get_events_created_from_response(&world.last_response);
    world.last_response = Some(format!(r#"{{"events_created": {}}}"#, current_count + 1));
    println!("✅ Article created via API");
}

#[when("I create a second article")]
async fn when_create_second_article(world: &mut LithairWorld) {
    let temp_guard = world.temp_dir.lock().await;
    let temp_path = temp_guard.as_ref().expect("Temp dir required").path();

    let mut event_store =
        EventStore::new(temp_path.to_str().unwrap()).expect("Failed to create event store");

    let last_hash = event_store.get_last_event_hash().cloned();

    let envelope = EventEnvelope::new(
        "ArticleCreated".to_string(),
        "article-second-002".to_string(),
        chrono::Utc::now().timestamp() as u64,
        r#"{"id": "002", "title": "Second Article"}"#.to_string(),
        Some("article-002".to_string()),
        last_hash,
    );

    event_store.append_envelope(&envelope).expect("Failed to append second event");
    event_store.flush().expect("Failed to flush");

    let current_count = get_events_created_from_response(&world.last_response);
    world.last_response = Some(format!(r#"{{"events_created": {}}}"#, current_count + 1));
    println!("✅ Second article created");
}

#[when(regex = r"I create (\d+) articles sequentially")]
async fn when_create_n_articles(world: &mut LithairWorld, count: u32) {
    let temp_guard = world.temp_dir.lock().await;
    let temp_path = temp_guard.as_ref().expect("Temp dir required").path();

    let mut event_store =
        EventStore::new(temp_path.to_str().unwrap()).expect("Failed to create event store");

    for i in 0..count {
        let last_hash = event_store.get_last_event_hash().cloned();

        let envelope = EventEnvelope::new(
            "ArticleCreated".to_string(),
            format!("article-seq-{:04}", i),
            chrono::Utc::now().timestamp() as u64 + i as u64,
            format!(r#"{{"id": "{}", "title": "Article {}"}}"#, i, i),
            Some(format!("article-{:04}", i)),
            last_hash,
        );

        event_store.append_envelope(&envelope).expect("Failed to append event");
    }

    event_store.flush().expect("Failed to flush");
    world.last_response = Some(format!(r#"{{"events_created": {}}}"#, count));
    println!("✅ Created {} articles sequentially", count);
}

#[when("I verify the chain integrity")]
async fn when_verify_chain(world: &mut LithairWorld) {
    let temp_guard = world.temp_dir.lock().await;
    let temp_path = temp_guard.as_ref().expect("Temp dir required").path();

    let event_store =
        EventStore::new(temp_path.to_str().unwrap()).expect("Failed to create event store");

    let result = event_store.verify_chain().expect("Failed to verify chain");

    // Store verification result as JSON
    let result_json = serde_json::json!({
        "total_events": result.total_events,
        "verified_events": result.verified_events,
        "legacy_events": result.legacy_events,
        "is_valid": result.is_valid,
        "invalid_hashes": result.invalid_hashes.iter().map(|e| {
            serde_json::json!({
                "event_index": e.event_index,
                "event_id": e.event_id,
                "error": e.error
            })
        }).collect::<Vec<_>>(),
        "broken_links": result.broken_links.iter().map(|e| {
            serde_json::json!({
                "event_index": e.event_index,
                "event_id": e.event_id,
                "error": e.error
            })
        }).collect::<Vec<_>>()
    });

    world.last_response = Some(result_json.to_string());
    println!("✅ Chain verification completed");
}

#[when("someone manually modifies the event payload in the log file")]
async fn when_tamper_event_payload(world: &mut LithairWorld) {
    use lithair_core::engine::persistence::calculate_crc32;

    let temp_guard = world.temp_dir.lock().await;
    let temp_path = temp_guard.as_ref().expect("Temp dir required").path();
    let events_file = temp_path.join("events.raftlog");

    if events_file.exists() {
        let content = fs::read_to_string(&events_file).expect("Failed to read events file");

        // Process each line - tamper the payload but recalculate CRC32 to bypass CRC check
        // This tests hash chain specifically (not CRC32 validation)
        let tampered_lines: Vec<String> = content
            .lines()
            .map(|line| {
                if line.contains("Original Title")
                    && line.len() > 9
                    && line.chars().nth(8) == Some(':')
                {
                    // Extract JSON part after CRC32 prefix
                    let json_data = &line[9..];
                    // Tamper the payload
                    let tampered_json = json_data.replace("Original Title", "HACKED Title");
                    // Recalculate CRC32 for the tampered data
                    let new_crc = calculate_crc32(tampered_json.as_bytes());
                    format!("{:08x}:{}", new_crc, tampered_json)
                } else {
                    line.to_string()
                }
            })
            .collect();

        let tampered = tampered_lines.join("\n");
        fs::write(&events_file, tampered).expect("Failed to write tampered file");
        println!("⚠️ Event payload tampered in log file (CRC32 updated, hash chain should detect)");
    }
}

#[when("someone deletes the 3rd event from the log file")]
async fn when_delete_third_event(world: &mut LithairWorld) {
    let temp_guard = world.temp_dir.lock().await;
    let temp_path = temp_guard.as_ref().expect("Temp dir required").path();
    let events_file = temp_path.join("events.raftlog");

    if events_file.exists() {
        let content = fs::read_to_string(&events_file).expect("Failed to read events file");
        let lines: Vec<&str> = content.lines().collect();

        // Remove the 3rd line (index 2)
        if lines.len() >= 3 {
            let mut new_lines: Vec<&str> = Vec::new();
            for (i, line) in lines.iter().enumerate() {
                if i != 2 {
                    new_lines.push(line);
                }
            }
            fs::write(&events_file, new_lines.join("\n") + "\n")
                .expect("Failed to write modified file");
            println!("⚠️ 3rd event deleted from log file");
        }
    }
}

#[when("I have created an article with title \"Original Title\"")]
async fn when_create_article_with_title(world: &mut LithairWorld) {
    let temp_guard = world.temp_dir.lock().await;
    let temp_path = temp_guard.as_ref().expect("Temp dir required").path();

    let mut event_store =
        EventStore::new(temp_path.to_str().unwrap()).expect("Failed to create event store");

    let last_hash = event_store.get_last_event_hash().cloned();

    let envelope = EventEnvelope::new(
        "ArticleCreated".to_string(),
        "article-original-title".to_string(),
        chrono::Utc::now().timestamp() as u64,
        r#"{"id": "orig", "title": "Original Title"}"#.to_string(),
        Some("article-orig".to_string()),
        last_hash,
    );

    event_store.append_envelope(&envelope).expect("Failed to append event");
    event_store.flush().expect("Failed to flush");

    world.last_response = Some(r#"{"events_created": 1}"#.to_string());
    println!("✅ Article with title 'Original Title' created");
}

#[given(regex = r#"I have created an article with title "([^"]+)""#)]
async fn given_create_article_with_title(world: &mut LithairWorld, title: String) {
    let temp_guard = world.temp_dir.lock().await;
    let temp_path = temp_guard.as_ref().expect("Temp dir required").path();

    let mut event_store =
        EventStore::new(temp_path.to_str().unwrap()).expect("Failed to create event store");

    let last_hash = event_store.get_last_event_hash().cloned();

    let envelope = EventEnvelope::new(
        "ArticleCreated".to_string(),
        format!("article-{}", uuid::Uuid::new_v4()),
        chrono::Utc::now().timestamp() as u64,
        format!(r#"{{"id": "test", "title": "{}"}}"#, title),
        Some("article-test".to_string()),
        last_hash,
    );

    event_store.append_envelope(&envelope).expect("Failed to append event");
    event_store.flush().expect("Failed to flush");

    world.last_response = Some(r#"{"events_created": 1}"#.to_string());
    println!("✅ Article with title '{}' created", title);
}

#[given(regex = r"I have created (\d+) articles forming a hash chain")]
async fn given_articles_forming_chain(world: &mut LithairWorld, count: u32) {
    when_create_n_articles(world, count).await;
}

// ==================== VERIFICATION STEPS ====================

#[then("the event envelope should contain an event_hash field")]
async fn then_event_has_hash(world: &mut LithairWorld) {
    let temp_guard = world.temp_dir.lock().await;
    let temp_path = temp_guard.as_ref().expect("Temp dir required").path();

    let event_store =
        EventStore::new(temp_path.to_str().unwrap()).expect("Failed to create event store");

    let envelopes = event_store.get_all_envelopes().expect("Failed to get envelopes");
    assert!(!envelopes.is_empty(), "Should have at least one envelope");

    let last = envelopes.last().unwrap();
    assert!(last.event_hash.is_some(), "Event should have event_hash");
    println!("✅ Event envelope contains event_hash field");
}

#[then(regex = r"the event_hash should be a valid SHA256 hex string \((\d+) characters\)")]
async fn then_hash_is_valid_sha256(world: &mut LithairWorld, expected_len: usize) {
    let temp_guard = world.temp_dir.lock().await;
    let temp_path = temp_guard.as_ref().expect("Temp dir required").path();

    let event_store =
        EventStore::new(temp_path.to_str().unwrap()).expect("Failed to create event store");

    let envelopes = event_store.get_all_envelopes().expect("Failed to get envelopes");
    let last = envelopes.last().unwrap();

    let hash = last.event_hash.as_ref().expect("Should have hash");
    assert_eq!(hash.len(), expected_len, "SHA256 hex should be {} chars", expected_len);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()), "Hash should be valid hex");
    println!("✅ Hash is valid SHA256 hex string ({} chars)", expected_len);
}

#[then("the hash should be computed from event content")]
async fn then_hash_from_content(world: &mut LithairWorld) {
    let temp_guard = world.temp_dir.lock().await;
    let temp_path = temp_guard.as_ref().expect("Temp dir required").path();

    let event_store =
        EventStore::new(temp_path.to_str().unwrap()).expect("Failed to create event store");

    let envelopes = event_store.get_all_envelopes().expect("Failed to get envelopes");
    let last = envelopes.last().unwrap();

    // Verify by recomputing the hash
    assert!(last.verify_hash(), "Hash should be verifiable");
    println!("✅ Hash is correctly computed from event content");
}

#[then("the second event should have a previous_hash field")]
async fn then_second_has_previous_hash(world: &mut LithairWorld) {
    let temp_guard = world.temp_dir.lock().await;
    let temp_path = temp_guard.as_ref().expect("Temp dir required").path();

    let event_store =
        EventStore::new(temp_path.to_str().unwrap()).expect("Failed to create event store");

    let envelopes = event_store.get_all_envelopes().expect("Failed to get envelopes");
    assert!(envelopes.len() >= 2, "Should have at least 2 events");

    let second = &envelopes[1];
    assert!(second.previous_hash.is_some(), "Second event should have previous_hash");
    println!("✅ Second event has previous_hash field");
}

#[then("the previous_hash should match the genesis event's event_hash")]
async fn then_previous_matches_genesis(world: &mut LithairWorld) {
    let temp_guard = world.temp_dir.lock().await;
    let temp_path = temp_guard.as_ref().expect("Temp dir required").path();

    let event_store =
        EventStore::new(temp_path.to_str().unwrap()).expect("Failed to create event store");

    let envelopes = event_store.get_all_envelopes().expect("Failed to get envelopes");
    let genesis = &envelopes[0];
    let second = &envelopes[1];

    assert_eq!(
        second.previous_hash.as_ref(),
        genesis.event_hash.as_ref(),
        "previous_hash should match genesis event_hash"
    );
    println!("✅ previous_hash matches genesis event_hash");
}

#[then("both events should form a valid chain")]
async fn then_valid_chain(world: &mut LithairWorld) {
    let temp_guard = world.temp_dir.lock().await;
    let temp_path = temp_guard.as_ref().expect("Temp dir required").path();

    let event_store =
        EventStore::new(temp_path.to_str().unwrap()).expect("Failed to create event store");

    let result = event_store.verify_chain().expect("Failed to verify chain");
    assert!(result.is_valid, "Chain should be valid");
    println!("✅ Both events form a valid chain");
}

#[then("each event should reference the hash of the previous event")]
async fn then_each_references_previous(world: &mut LithairWorld) {
    let temp_guard = world.temp_dir.lock().await;
    let temp_path = temp_guard.as_ref().expect("Temp dir required").path();

    let event_store =
        EventStore::new(temp_path.to_str().unwrap()).expect("Failed to create event store");

    let envelopes = event_store.get_all_envelopes().expect("Failed to get envelopes");

    for i in 1..envelopes.len() {
        let current = &envelopes[i];
        let previous = &envelopes[i - 1];

        assert!(current.links_to(previous), "Event {} should link to event {}", i, i - 1);
    }
    println!("✅ All events reference the hash of the previous event");
}

#[then("the first event should have no previous_hash (genesis)")]
async fn then_first_no_previous(world: &mut LithairWorld) {
    let temp_guard = world.temp_dir.lock().await;
    let temp_path = temp_guard.as_ref().expect("Temp dir required").path();

    let event_store =
        EventStore::new(temp_path.to_str().unwrap()).expect("Failed to create event store");

    let envelopes = event_store.get_all_envelopes().expect("Failed to get envelopes");
    let first = &envelopes[0];

    assert!(first.previous_hash.is_none(), "Genesis event should have no previous_hash");
    println!("✅ First event has no previous_hash (genesis)");
}

#[then("the chain should be verifiable from start to end")]
async fn then_chain_verifiable(world: &mut LithairWorld) {
    let temp_guard = world.temp_dir.lock().await;
    let temp_path = temp_guard.as_ref().expect("Temp dir required").path();

    let event_store =
        EventStore::new(temp_path.to_str().unwrap()).expect("Failed to create event store");

    let result = event_store.verify_chain().expect("Failed to verify chain");
    let events_created = get_events_created_from_response(&world.last_response);

    assert!(result.is_valid, "Chain should be verifiable");
    assert_eq!(result.verified_events, events_created, "All events should be verified");
    println!("✅ Chain is verifiable from start to end");
}

#[then("the verification should report an invalid hash")]
async fn then_verification_invalid_hash(world: &mut LithairWorld) {
    let result = get_verification_result_from_response(&world.last_response)
        .expect("Chain verification should have been run");

    assert!(!result.is_valid, "Chain should be invalid after tampering");
    assert!(!result.invalid_hashes.is_empty(), "Should have invalid hash errors");
    println!("✅ Verification reported invalid hash");
}

#[then("the tampered event should be identified by index and event_id")]
async fn then_tampered_event_identified(world: &mut LithairWorld) {
    let result = get_verification_result_from_response(&world.last_response)
        .expect("Chain verification should have been run");

    assert!(!result.invalid_hashes.is_empty(), "Should have identified tampered event");
    let error = &result.invalid_hashes[0];
    println!(
        "✅ Tampered event identified: index={}, event_id={}",
        error.event_index, error.event_id
    );
}

#[then("the chain should be marked as INVALID")]
async fn then_chain_invalid(world: &mut LithairWorld) {
    let result = get_verification_result_from_response(&world.last_response)
        .expect("Chain verification should have been run");

    assert!(!result.is_valid, "Chain should be marked as INVALID");
    println!("✅ Chain marked as INVALID");
}

#[then("the verification should detect a broken chain link")]
async fn then_broken_link_detected(world: &mut LithairWorld) {
    let result = get_verification_result_from_response(&world.last_response)
        .expect("Chain verification should have been run");

    // After deletion, either invalid_hashes or broken_links should be non-empty
    let has_errors = !result.invalid_hashes.is_empty() || !result.broken_links.is_empty();
    assert!(has_errors, "Should detect broken chain after deletion");
    println!("✅ Broken chain link detected");
}

// ==================== LEGACY COMPATIBILITY STEPS ====================

#[given("an existing events.raftlog file with legacy events (no hashes)")]
async fn given_legacy_events(world: &mut LithairWorld) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let events_file = temp_dir.path().join("events.raftlog");

    // Write legacy events without hash fields
    let legacy_events = r#"{"event_type":"ArticleCreated","event_id":"legacy-001","timestamp":1234567890,"payload":"{}","aggregate_id":"art-1"}
{"event_type":"ArticleCreated","event_id":"legacy-002","timestamp":1234567891,"payload":"{}","aggregate_id":"art-2"}
{"event_type":"ArticleCreated","event_id":"legacy-003","timestamp":1234567892,"payload":"{}","aggregate_id":"art-3"}
"#;

    fs::write(&events_file, legacy_events).expect("Failed to write legacy events");
    *world.temp_dir.lock().await = Some(temp_dir);
    println!("✅ Legacy events file created");
}

#[then("the legacy events should be loaded successfully")]
async fn then_legacy_loaded(world: &mut LithairWorld) {
    let temp_guard = world.temp_dir.lock().await;
    let temp_path = temp_guard.as_ref().expect("Temp dir required").path();

    let event_store =
        EventStore::new(temp_path.to_str().unwrap()).expect("Failed to create event store");

    let envelopes = event_store.get_all_envelopes().expect("Failed to get envelopes");
    assert!(!envelopes.is_empty(), "Legacy events should be loaded");
    println!("✅ Legacy events loaded successfully: {} events", envelopes.len());
}

#[then("chain verification should report them as legacy events")]
async fn then_legacy_reported(world: &mut LithairWorld) {
    let temp_guard = world.temp_dir.lock().await;
    let temp_path = temp_guard.as_ref().expect("Temp dir required").path();

    let event_store =
        EventStore::new(temp_path.to_str().unwrap()).expect("Failed to create event store");

    let result = event_store.verify_chain().expect("Failed to verify chain");
    assert!(result.legacy_events > 0, "Should report legacy events");
    println!("✅ Chain verification reports {} legacy events", result.legacy_events);
}

#[then("new events should start a fresh hash chain")]
async fn then_fresh_chain(world: &mut LithairWorld) {
    let temp_guard = world.temp_dir.lock().await;
    let temp_path = temp_guard.as_ref().expect("Temp dir required").path();

    let mut event_store =
        EventStore::new(temp_path.to_str().unwrap()).expect("Failed to create event store");

    // Get last hash (may be None if all legacy)
    let last_hash = event_store.get_last_event_hash().cloned();

    // Add a new event
    let envelope = EventEnvelope::new(
        "ArticleCreated".to_string(),
        "new-after-legacy".to_string(),
        chrono::Utc::now().timestamp() as u64,
        "{}".to_string(),
        None,
        last_hash,
    );

    event_store.append_envelope(&envelope).expect("Failed to append");
    event_store.flush().expect("Failed to flush");

    // Verify the new event has a hash
    let envelopes = event_store.get_all_envelopes().expect("Failed to get envelopes");
    let last = envelopes.last().unwrap();
    assert!(last.event_hash.is_some(), "New event should have hash");
    println!("✅ New events start fresh hash chain");
}

// ==================== HELPER FUNCTIONS ====================

fn get_events_created_from_response(response: &Option<String>) -> usize {
    response
        .as_ref()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .and_then(|v| v.get("events_created").and_then(|e| e.as_u64()))
        .unwrap_or(0) as usize
}

#[derive(Debug)]
struct VerificationResult {
    is_valid: bool,
    invalid_hashes: Vec<ChainErrorInfo>,
    broken_links: Vec<ChainErrorInfo>,
}

#[derive(Debug)]
struct ChainErrorInfo {
    event_index: usize,
    event_id: String,
}

fn get_verification_result_from_response(response: &Option<String>) -> Option<VerificationResult> {
    let json: serde_json::Value = response.as_ref().and_then(|s| serde_json::from_str(s).ok())?;

    let is_valid = json.get("is_valid")?.as_bool()?;

    let invalid_hashes = json
        .get("invalid_hashes")?
        .as_array()?
        .iter()
        .filter_map(|v| {
            Some(ChainErrorInfo {
                event_index: v.get("event_index")?.as_u64()? as usize,
                event_id: v.get("event_id")?.as_str()?.to_string(),
            })
        })
        .collect();

    let broken_links = json
        .get("broken_links")?
        .as_array()?
        .iter()
        .filter_map(|v| {
            Some(ChainErrorInfo {
                event_index: v.get("event_index")?.as_u64()? as usize,
                event_id: v.get("event_id")?.as_str()?.to_string(),
            })
        })
        .collect();

    Some(VerificationResult { is_valid, invalid_hashes, broken_links })
}
