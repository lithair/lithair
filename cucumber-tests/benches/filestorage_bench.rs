use lithair_core::engine::FileStorage;
use std::time::Instant;

fn main() {
    println!("🎯 BENCHMARK FILESTORAGE - Écriture pure disque\n");

    // Nettoyage
    let _ = std::fs::remove_dir_all("/tmp/lithair-bench-filestorage");
    std::fs::create_dir_all("/tmp/lithair-bench-filestorage").unwrap();

    let mut storage = FileStorage::new("/tmp/lithair-bench-filestorage").unwrap();

    // Test 1: 10K événements
    bench_write(&mut storage, 10_000, "10K");

    // Test 2: 50K événements
    bench_write(&mut storage, 50_000, "50K");

    // Test 3: 100K événements
    bench_write(&mut storage, 100_000, "100K");
}

fn bench_write(storage: &mut FileStorage, count: usize, label: &str) {
    println!("📊 Test {} événements...", label);

    let start = Instant::now();

    for i in 0..count {
        let event_json = serde_json::json!({
            "type": "ArticleCreated",
            "id": format!("article-{}", i),
            "title": format!("Article {}", i),
            "content": format!("Content {}", i),
            "timestamp": chrono::Utc::now().to_rfc3339()
        })
        .to_string();

        storage.append_event(&event_json).unwrap();

        // Flush tous les 1000 pour optimiser
        if i % 1000 == 0 {
            storage.flush_batch().unwrap();
        }
    }

    storage.flush_batch().unwrap();

    let elapsed = start.elapsed();
    let throughput = count as f64 / elapsed.as_secs_f64();

    println!("✅ {} événements en {:.2}s", label, elapsed.as_secs_f64());
    println!("   📈 Throughput: {:.0} events/sec", throughput);
    println!("   ⏱️  Latence moyenne: {:.3}ms\n", elapsed.as_millis() as f64 / count as f64);
}
