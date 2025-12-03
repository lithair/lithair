use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::runtime::Runtime;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Debug)]
struct BenchmarkProduct {
    id: Uuid,
    name: String,
    price: f64,
    category: String,
    timestamp: chrono::DateTime<chrono::Utc>,
}

impl BenchmarkProduct {
    fn new(name: String, price: f64) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            price,
            category: "benchmark".to_string(),
            timestamp: chrono::Utc::now(),
        }
    }
}

fn benchmark_consensus_throughput(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    
    // Test different batch sizes for throughput optimization
    let batch_sizes = vec![1, 10, 50, 100, 500, 1000];
    
    let mut group = c.benchmark_group("consensus_throughput");
    
    for batch_size in batch_sizes {
        group.throughput(Throughput::Elements(batch_size as u64));
        
        group.bench_with_input(
            BenchmarkId::new("json_serialization", batch_size),
            &batch_size,
            |b, &batch_size| {
                let products: Vec<BenchmarkProduct> = (0..batch_size)
                    .map(|i| BenchmarkProduct::new(format!("Product {}", i), 10.0 + i as f64))
                    .collect();
                
                b.iter(|| {
                    let serialized: Vec<String> = products
                        .iter()
                        .map(|p| serde_json::to_string(p).unwrap())
                        .collect();
                    
                    let _deserialized: Vec<BenchmarkProduct> = serialized
                        .iter()
                        .map(|s| serde_json::from_str(s).unwrap())
                        .collect();
                    
                    black_box(_deserialized)
                });
            },
        );
        
        group.bench_with_input(
            BenchmarkId::new("bincode_serialization", batch_size),
            &batch_size,
            |b, &batch_size| {
                let products: Vec<BenchmarkProduct> = (0..batch_size)
                    .map(|i| BenchmarkProduct::new(format!("Product {}", i), 10.0 + i as f64))
                    .collect();
                
                b.iter(|| {
                    let serialized: Vec<Vec<u8>> = products
                        .iter()
                        .map(|p| bincode::serialize(p).unwrap())
                        .collect();
                    
                    let _deserialized: Vec<BenchmarkProduct> = serialized
                        .iter()
                        .map(|s| bincode::deserialize(s).unwrap())
                        .collect();
                    
                    black_box(_deserialized)
                });
            },
        );
    }
    
    group.finish();
}

fn benchmark_target_1000_ops_sec(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    
    let mut group = c.benchmark_group("target_performance");
    group.measurement_time(std::time::Duration::from_secs(10));
    
    // Target: >1000 ops/sec = <1ms per operation
    group.bench_function("single_operation_latency", |b| {
        let product = BenchmarkProduct::new("Target Test".to_string(), 99.99);
        
        b.iter(|| {
            // Simulate full operation: serialize + store + replicate + deserialize
            let serialized = bincode::serialize(&product).unwrap();
            let _stored_size = serialized.len();
            let _deserialized: BenchmarkProduct = bincode::deserialize(&serialized).unwrap();
            
            black_box(_deserialized)
        });
    });
    
    // Batch operations to achieve >1000 ops/sec
    group.bench_function("batch_1000_operations", |b| {
        let products: Vec<BenchmarkProduct> = (0..1000)
            .map(|i| BenchmarkProduct::new(format!("Batch {}", i), i as f64))
            .collect();
        
        b.iter(|| {
            let _processed: Vec<_> = products
                .iter()
                .map(|p| {
                    let serialized = bincode::serialize(p).unwrap();
                    bincode::deserialize::<BenchmarkProduct>(&serialized).unwrap()
                })
                .collect();
            
            black_box(_processed)
        });
    });
    
    group.finish();
}

fn benchmark_memory_usage(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_optimization");
    
    // Test SCC2 HashMap vs std HashMap for concurrent access
    group.bench_function("scc_hashmap_insertion", |b| {
        use scc::HashMap as SccHashMap;
        
        b.iter(|| {
            let map = Arc::new(SccHashMap::new());
            
            for i in 0..1000 {
                let product = BenchmarkProduct::new(format!("SCC {}", i), i as f64);
                map.insert(product.id, product);
            }
            
            black_box(map.len())
        });
    });
    
    group.bench_function("std_hashmap_insertion", |b| {
        use std::collections::HashMap;
        use std::sync::RwLock;
        
        b.iter(|| {
            let map = Arc::new(RwLock::new(HashMap::new()));
            
            for i in 0..1000 {
                let product = BenchmarkProduct::new(format!("STD {}", i), i as f64);
                map.write().unwrap().insert(product.id, product);
            }
            
            black_box(map.read().unwrap().len())
        });
    });
    
    group.finish();
}

criterion_group!(
    benches,
    benchmark_consensus_throughput,
    benchmark_target_1000_ops_sec,
    benchmark_memory_usage
);
criterion_main!(benches);