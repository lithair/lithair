use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Debug)]
struct ConsensusData {
    data_id: Uuid,
    content: serde_json::Value,
    version: u64,
    checksum: String,
    node_origin: Uuid,
    replicated_to: Vec<Uuid>,
}

impl ConsensusData {
    fn new_sample(size_kb: usize) -> Self {
        // Create sample data of specified size
        let content_size = size_kb * 1024;
        let large_string = "x".repeat(content_size);
        
        Self {
            data_id: Uuid::new_v4(),
            content: serde_json::json!({
                "large_data": large_string,
                "metadata": {
                    "type": "benchmark",
                    "created": chrono::Utc::now().to_rfc3339(),
                    "size_kb": size_kb
                }
            }),
            version: 1,
            checksum: format!("sha256:{:x}", size_kb),
            node_origin: Uuid::new_v4(),
            replicated_to: vec![Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()],
        }
    }
}

fn benchmark_json_vs_bincode(c: &mut Criterion) {
    let sizes = vec![1, 10, 100, 500]; // KB sizes
    
    let mut group = c.benchmark_group("serialization_comparison");
    
    for size_kb in sizes {
        let data = ConsensusData::new_sample(size_kb);
        
        // JSON serialization
        group.bench_with_input(
            BenchmarkId::new("json_serialize", size_kb),
            &data,
            |b, data| {
                b.iter(|| {
                    let serialized = serde_json::to_string(data).unwrap();
                    black_box(serialized)
                });
            },
        );
        
        group.bench_with_input(
            BenchmarkId::new("json_deserialize", size_kb),
            &serde_json::to_string(&data).unwrap(),
            |b, serialized| {
                b.iter(|| {
                    let deserialized: ConsensusData = serde_json::from_str(serialized).unwrap();
                    black_box(deserialized)
                });
            },
        );
        
        // Bincode serialization
        group.bench_with_input(
            BenchmarkId::new("bincode_serialize", size_kb),
            &data,
            |b, data| {
                b.iter(|| {
                    let serialized = bincode::serialize(data).unwrap();
                    black_box(serialized)
                });
            },
        );
        
        group.bench_with_input(
            BenchmarkId::new("bincode_deserialize", size_kb),
            &bincode::serialize(&data).unwrap(),
            |b, serialized| {
                b.iter(|| {
                    let deserialized: ConsensusData = bincode::deserialize(serialized).unwrap();
                    black_box(deserialized)
                });
            },
        );
    }
    
    group.finish();
}

fn benchmark_compression_ratio(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression_analysis");
    
    let sizes = vec![10, 100, 500, 1000]; // KB sizes
    
    for size_kb in sizes {
        let data = ConsensusData::new_sample(size_kb);
        
        group.bench_with_input(
            BenchmarkId::new("size_comparison", size_kb),
            &data,
            |b, data| {
                b.iter(|| {
                    let json_serialized = serde_json::to_string(data).unwrap();
                    let bincode_serialized = bincode::serialize(data).unwrap();
                    
                    let json_size = json_serialized.len();
                    let bincode_size = bincode_serialized.len();
                    let compression_ratio = json_size as f64 / bincode_size as f64;
                    
                    // Log compression statistics for analysis
                    println!(
                        "Size {}KB - JSON: {}B, Bincode: {}B, Ratio: {:.2}x", 
                        size_kb, json_size, bincode_size, compression_ratio
                    );
                    
                    black_box((json_size, bincode_size, compression_ratio))
                });
            },
        );
    }
    
    group.finish();
}

fn benchmark_replication_payload(c: &mut Criterion) {
    let mut group = c.benchmark_group("replication_performance");
    
    // Simulate realistic replication scenarios
    let batch_sizes = vec![1, 10, 50, 100];
    
    for batch_size in batch_sizes {
        let batch_data: Vec<ConsensusData> = (0..batch_size)
            .map(|_| ConsensusData::new_sample(5)) // 5KB per item
            .collect();
        
        group.bench_with_input(
            BenchmarkId::new("json_batch_replication", batch_size),
            &batch_data,
            |b, batch| {
                b.iter(|| {
                    let serialized_batch: Vec<String> = batch
                        .iter()
                        .map(|item| serde_json::to_string(item).unwrap())
                        .collect();
                    
                    // Simulate network payload preparation
                    let payload = serde_json::json!({
                        "batch_size": batch.len(),
                        "data": serialized_batch,
                        "timestamp": chrono::Utc::now().to_rfc3339()
                    });
                    
                    let final_payload = serde_json::to_string(&payload).unwrap();
                    black_box(final_payload.len())
                });
            },
        );
        
        group.bench_with_input(
            BenchmarkId::new("bincode_batch_replication", batch_size),
            &batch_data,
            |b, batch| {
                b.iter(|| {
                    let serialized_batch: Vec<Vec<u8>> = batch
                        .iter()
                        .map(|item| bincode::serialize(item).unwrap())
                        .collect();
                    
                    // Simulate efficient binary payload
                    let total_size: usize = serialized_batch.iter().map(|v| v.len()).sum();
                    black_box(total_size)
                });
            },
        );
    }
    
    group.finish();
}

criterion_group!(
    benches, 
    benchmark_json_vs_bincode,
    benchmark_compression_ratio,
    benchmark_replication_payload
);
criterion_main!(benches);