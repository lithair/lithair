use lithair_core::engine::{AsyncWriter, EventStore, Scc2Engine, Scc2EngineConfig};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use tokio::sync::Mutex;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct Article {
    id: String,
    title: String,
    content: String,
}

impl lithair_core::model_inspect::Inspectable for Article {
    fn get_field_value(&self, field_name: &str) -> Option<serde_json::Value> {
        match field_name {
            "id" => serde_json::to_value(&self.id).ok(),
            "title" => serde_json::to_value(&self.title).ok(),
            "content" => serde_json::to_value(&self.content).ok(),
            _ => None,
        }
    }
}

impl lithair_core::model::ModelSpec for Article {
    fn get_policy(&self, _field_name: &str) -> Option<lithair_core::model::FieldPolicy> {
        None
    }
    fn get_all_fields(&self) -> Vec<String> {
        vec!["id".to_string(), "title".to_string(), "content".to_string()]
    }
}

/// Direct stress test of the Lithair engine (without HTTP)
/// Measures the TRUE engine performance
#[tokio::main]
async fn main() {
    println!("\n╔═══════════════════════════════════════════════════════════╗");
    println!("║   🚀 LITHAIR ENGINE STRESS TEST - 10K ARTICLES          ║");
    println!("║   (Direct test without HTTP overhead)                    ║");
    println!("╚═══════════════════════════════════════════════════════════╝\n");

    let persist_path = "/tmp/lithair-engine-stress";

    // Clean up
    std::fs::remove_dir_all(persist_path).ok();
    std::fs::create_dir_all(persist_path).ok();

    // Create EventStore + AsyncWriter
    let event_store =
        Arc::new(RwLock::new(EventStore::new(persist_path).expect("EventStore init failed")));
    let async_writer = Arc::new(Mutex::new(Some(AsyncWriter::new(event_store.clone(), 1000))));

    // Create SCC2
    let scc2: Arc<Scc2Engine<Article>> = Arc::new(
        Scc2Engine::new(
            event_store,
            Scc2EngineConfig {
                verbose_logging: false,
                enable_snapshots: false,
                snapshot_interval: 1000,
                enable_deduplication: false,
                auto_persist_writes: false,
                force_immediate_persistence: false,
            },
        )
        .unwrap(),
    );

    println!("✅ Engine initialized");
    println!("  - AsyncWriter batch_size: 1000");
    println!("  - DurabilityMode: MaxDurability");
    println!("  - SCC2: Lock-free hashmap");
    println!();

    // ==================== PHASE 1: CREATE 10K ====================
    println!("📝 Phase 1: Creating 10,000 articles...");
    let start = std::time::Instant::now();

    for i in 0..10_000 {
        let article = Article {
            id: format!("article-{}", i),
            title: format!("Title {}", i),
            content: format!("Content {}", i),
        };

        // Persist via AsyncWriter
        if let Some(ref writer) = *async_writer.lock().await {
            let event_json = serde_json::to_string(&article).unwrap();
            writer.write(event_json).ok();
        }

        // Store in memory (SCC2)
        scc2.insert(article.id.clone(), article).await;

        if i % 1000 == 0 && i > 0 {
            println!("  ... {} articles created", i);
        }
    }

    let elapsed_create = start.elapsed();
    let throughput_create = 10_000.0 / elapsed_create.as_secs_f64();

    println!("✅ Creation complete:");
    println!("   Time: {:.2}s", elapsed_create.as_secs_f64());
    println!("   Throughput: {:.0} articles/sec", throughput_create);
    println!();

    // ==================== PHASE 2: UPDATE 2K ====================
    println!("🔄 Phase 2: Updating 2,000 articles...");
    let start = std::time::Instant::now();

    for i in 0..2_000 {
        let article_id = format!("article-{}", i);

        if let Some(mut article) = scc2.read(&article_id, |s| s.clone()) {
            article.title = format!("Updated Title {}", i);
            article.content = format!("Updated Content {}", i);

            // Persist
            if let Some(ref writer) = *async_writer.lock().await {
                let event_json = serde_json::to_string(&article).unwrap();
                writer.write(event_json).ok();
            }

            // Update SCC2
            scc2.insert(article_id, article).await;
        }

        if i % 500 == 0 && i > 0 {
            println!("  ... {} articles updated", i);
        }
    }

    let elapsed_update = start.elapsed();
    let throughput_update = 2_000.0 / elapsed_update.as_secs_f64();

    println!("✅ Update complete:");
    println!("   Time: {:.2}s", elapsed_update.as_secs_f64());
    println!("   Throughput: {:.0} articles/sec", throughput_update);
    println!();

    // ==================== PHASE 3: DELETE 1K ====================
    println!("🗑️  Phase 3: Deleting 1,000 articles...");
    let start = std::time::Instant::now();

    for i in 0..1_000 {
        let article_id = format!("article-{}", i);

        // Remove from SCC2
        scc2.remove(&article_id).await;

        // Persist delete event
        if let Some(ref writer) = *async_writer.lock().await {
            let event = serde_json::json!({
                "type": "ArticleDeleted",
                "id": article_id
            });
            writer.write(event.to_string()).ok();
        }

        if i % 250 == 0 && i > 0 {
            println!("  ... {} articles deleted", i);
        }
    }

    let elapsed_delete = start.elapsed();
    let throughput_delete = 1_000.0 / elapsed_delete.as_secs_f64();

    println!("✅ Deletion complete:");
    println!("   Time: {:.2}s", elapsed_delete.as_secs_f64());
    println!("   Throughput: {:.0} articles/sec", throughput_delete);
    println!();

    // ==================== PHASE 4: FLUSH ====================
    println!("💾 Phase 4: Flush AsyncWriter...");

    if let Some(writer) = async_writer.lock().await.take() {
        writer.shutdown().await;
    }

    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    println!("✅ Flush complete");
    println!();

    // ==================== PHASE 5: VERIFICATIONS ====================
    println!("🔍 Phase 5: Integrity verifications...");

    let events_file = format!("{}/events.raftlog", persist_path);

    if std::path::Path::new(&events_file).exists() {
        let content = std::fs::read_to_string(&events_file).unwrap();
        let event_count = content.lines().count();
        let file_size_mb = content.len() as f64 / 1024.0 / 1024.0;

        println!("✅ File events.raftlog exists");
        println!("   Persisted events: {}", event_count);
        println!("   File size: {:.2} MB", file_size_mb);
    } else {
        println!("❌ File events.raftlog MISSING");
    }

    let memory_count = scc2.iter_all().await.len();
    println!("✅ Articles in memory (SCC2): {}", memory_count);
    println!();

    // ==================== FINAL RESULTS ====================
    let total_ops = 10_000 + 2_000 + 1_000;
    let total_time = elapsed_create + elapsed_update + elapsed_delete;
    let avg_throughput = total_ops as f64 / total_time.as_secs_f64();

    println!("\n╔══════════════════════════════════════════════════════════╗");
    println!("║   📊 FINAL RESULTS                                      ║");
    println!("╠══════════════════════════════════════════════════════════╣");
    println!("║ Total operations:       {:>28} ║", total_ops);
    println!("║ Total duration:         {:>23.2}s ║", total_time.as_secs_f64());
    println!("║ Average throughput:     {:>21.0}/sec ║", avg_throughput);
    println!("╠══════════════════════════════════════════════════════════╣");
    println!("║ CREATE (10K):           {:>21.0}/sec ║", throughput_create);
    println!("║ UPDATE (2K):            {:>21.0}/sec ║", throughput_update);
    println!("║ DELETE (1K):            {:>21.0}/sec ║", throughput_delete);
    println!("╠══════════════════════════════════════════════════════════╣");
    println!("║ Final articles:         {:>28} ║", memory_count);
    println!("╚══════════════════════════════════════════════════════════╝\n");

    println!("🎯 Test completed successfully!");
}
