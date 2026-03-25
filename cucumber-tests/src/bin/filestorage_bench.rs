use lithair_core::engine::FileStorage;
use std::time::Instant;

fn main() {
    println!("🎯 BENCHMARK FILESTORAGE - Pure disk write\n");
    
    // Cleanup
    let _ = std::fs::remove_dir_all("/tmp/lithair-bench-filestorage");
    std::fs::create_dir_all("/tmp/lithair-bench-filestorage").unwrap();
    
    let mut storage = FileStorage::new("/tmp/lithair-bench-filestorage").unwrap();
    
    // Test 1: 10K events
    bench_write(&mut storage, 10_000, "10K");
    
    // Test 2: 50K events
    bench_write(&mut storage, 50_000, "50K");
    
    // Test 3: 100K events
    bench_write(&mut storage, 100_000, "100K");
}

fn bench_write(storage: &mut FileStorage, count: usize, label: &str) {
    println!("📊 Test {} events...", label);
    
    let start = Instant::now();
    
    for i in 0..count {
        let event_json = serde_json::json!({
            "type": "ArticleCreated",
            "id": format!("article-{}", i),
            "title": format!("Article {}", i),
            "content": format!("Content {}", i),
            "timestamp": chrono::Utc::now().to_rfc3339()
        }).to_string();
        
        storage.append_event(&event_json).unwrap();
        
        // Flush every 1000 for optimization
        if i % 1000 == 0 {
            storage.flush_batch().unwrap();
        }
    }
    
    storage.flush_batch().unwrap();
    
    let elapsed = start.elapsed();
    let throughput = count as f64 / elapsed.as_secs_f64();
    
    println!("✅ {} events in {:.2}s", label, elapsed.as_secs_f64());
    println!("   📈 Throughput: {:.0} events/sec", throughput);
    println!("   ⏱️  Average latency: {:.3}ms\n", elapsed.as_millis() as f64 / count as f64);
}
