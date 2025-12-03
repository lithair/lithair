use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use scc::HashMap as SccHashMap;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::thread;
use uuid::Uuid;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
struct PerformanceMetrics {
    metric_id: Uuid,
    timestamp: chrono::DateTime<chrono::Utc>,
    operation_type: String,
    latency_ms: f64,
    throughput_ops_sec: f64,
    memory_usage_mb: f64,
    node_id: Uuid,
}

impl PerformanceMetrics {
    fn new_sample(op_type: &str, latency: f64, throughput: f64) -> Self {
        Self {
            metric_id: Uuid::new_v4(),
            timestamp: chrono::Utc::now(),
            operation_type: op_type.to_string(),
            latency_ms: latency,
            throughput_ops_sec: throughput,
            memory_usage_mb: rand::random::<f64>() * 100.0, // Simulated memory usage
            node_id: Uuid::new_v4(),
        }
    }
}

fn benchmark_scc2_concurrent_access(c: &mut Criterion) {
    let mut group = c.benchmark_group("scc2_concurrent_performance");
    
    let thread_counts = vec![1, 2, 4, 8, 16];
    let operations_per_thread = 1000;
    
    for num_threads in thread_counts {
        group.throughput(Throughput::Elements((num_threads * operations_per_thread) as u64));
        
        // SCC2 HashMap concurrent benchmark
        group.bench_with_input(
            BenchmarkId::new("scc_hashmap_concurrent", num_threads),
            &num_threads,
            |b, &num_threads| {
                b.iter(|| {
                    let map = Arc::new(SccHashMap::new());
                    let mut handles = vec![];
                    
                    for thread_id in 0..num_threads {
                        let map_clone = Arc::clone(&map);
                        
                        let handle = thread::spawn(move || {
                            for i in 0..operations_per_thread {
                                let key = Uuid::new_v4();
                                let metrics = PerformanceMetrics::new_sample(
                                    "concurrent_test",
                                    1.5 + (i as f64 * 0.1),
                                    1000.0 + (thread_id as f64 * 50.0)
                                );
                                
                                // Insert operation
                                map_clone.insert(key, metrics.clone());
                                
                                // Read operation  
                                if let Some(entry) = map_clone.get(&key) {
                                    black_box(entry.key());
                                }
                                
                                // Update operation (simulate version increment)
                                if i % 10 == 0 {
                                    let updated_metrics = PerformanceMetrics::new_sample(
                                        "updated_test",
                                        metrics.latency_ms + 0.5,
                                        metrics.throughput_ops_sec + 10.0
                                    );
                                    map_clone.insert(key, updated_metrics);
                                }
                            }
                        });
                        
                        handles.push(handle);
                    }
                    
                    for handle in handles {
                        handle.join().unwrap();
                    }
                    
                    black_box(map.len())
                });
            },
        );
        
        // Standard HashMap with RwLock for comparison
        group.bench_with_input(
            BenchmarkId::new("std_hashmap_rwlock", num_threads),
            &num_threads,
            |b, &num_threads| {
                b.iter(|| {
                    let map = Arc::new(RwLock::new(HashMap::new()));
                    let mut handles = vec![];
                    
                    for thread_id in 0..num_threads {
                        let map_clone = Arc::clone(&map);
                        
                        let handle = thread::spawn(move || {
                            for i in 0..operations_per_thread {
                                let key = Uuid::new_v4();
                                let metrics = PerformanceMetrics::new_sample(
                                    "concurrent_test",
                                    1.5 + (i as f64 * 0.1),
                                    1000.0 + (thread_id as f64 * 50.0)
                                );
                                
                                // Insert operation
                                {
                                    let mut map_guard = map_clone.write().unwrap();
                                    map_guard.insert(key, metrics.clone());
                                }
                                
                                // Read operation
                                {
                                    let map_guard = map_clone.read().unwrap();
                                    if let Some(value) = map_guard.get(&key) {
                                        black_box(value);
                                    }
                                }
                                
                                // Update operation
                                if i % 10 == 0 {
                                    let mut map_guard = map_clone.write().unwrap();
                                    let updated_metrics = PerformanceMetrics::new_sample(
                                        "updated_test",
                                        metrics.latency_ms + 0.5,
                                        metrics.throughput_ops_sec + 10.0
                                    );
                                    map_guard.insert(key, updated_metrics);
                                }
                            }
                        });
                        
                        handles.push(handle);
                    }
                    
                    for handle in handles {
                        handle.join().unwrap();
                    }
                    
                    black_box(map.read().unwrap().len())
                });
            },
        );
    }
    
    group.finish();
}

fn benchmark_memory_efficiency(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_efficiency");
    
    let data_sizes = vec![1000, 10000, 50000, 100000];
    
    for size in data_sizes {
        group.bench_with_input(
            BenchmarkId::new("scc_memory_usage", size),
            &size,
            |b, &size| {
                b.iter(|| {
                    let map = SccHashMap::new();
                    
                    // Fill map with performance metrics
                    for i in 0..size {
                        let metrics = PerformanceMetrics::new_sample(
                            "memory_test",
                            (i as f64) * 0.01,
                            1000.0 + (i as f64)
                        );
                        map.insert(Uuid::new_v4(), metrics);
                    }
                    
                    // Simulate concurrent access patterns
                    let mut sum_latency = 0.0;
                    map.scan(|_, value| {
                        sum_latency += value.latency_ms;
                    });
                    
                    black_box((map.len(), sum_latency))
                });
            },
        );
        
        group.bench_with_input(
            BenchmarkId::new("std_memory_usage", size),
            &size,
            |b, &size| {
                b.iter(|| {
                    let mut map = HashMap::new();
                    
                    // Fill map with performance metrics
                    for i in 0..size {
                        let metrics = PerformanceMetrics::new_sample(
                            "memory_test",
                            (i as f64) * 0.01,
                            1000.0 + (i as f64)
                        );
                        map.insert(Uuid::new_v4(), metrics);
                    }
                    
                    // Simulate access patterns
                    let sum_latency: f64 = map.values().map(|v| v.latency_ms).sum();
                    
                    black_box((map.len(), sum_latency))
                });
            },
        );
    }
    
    group.finish();
}

fn benchmark_target_40m_ops_sec(c: &mut Criterion) {
    let mut group = c.benchmark_group("target_40m_ops_sec");
    group.measurement_time(std::time::Duration::from_secs(5));
    
    // Target: Validate SCC2's claimed 40M+ ops/sec capability
    group.bench_function("scc2_single_thread_ops", |b| {
        let map = SccHashMap::new();
        let mut counter = 0u64;
        
        b.iter(|| {
            let key = counter;
            counter += 1;
            
            let metrics = PerformanceMetrics::new_sample(
                "speed_test",
                0.001, // 1μs latency target
                40_000_000.0 // 40M ops/sec target
            );
            
            map.insert(key, metrics);
            
            if let Some(entry) = map.get(&key) {
                black_box(entry.key());
            }
            
            black_box(key)
        });
    });
    
    // Batch operations for realistic throughput measurement
    group.bench_function("scc2_batch_operations", |b| {
        b.iter(|| {
            let map = SccHashMap::new();
            
            // Batch of 10000 operations to measure sustained throughput
            for i in 0..10000 {
                let metrics = PerformanceMetrics::new_sample(
                    "batch_test",
                    0.001,
                    40_000_000.0
                );
                map.insert(i, metrics);
            }
            
            let mut read_count = 0;
            map.scan(|_, _| {
                read_count += 1;
            });
            
            black_box((map.len(), read_count))
        });
    });
    
    group.finish();
}

criterion_group!(
    benches,
    benchmark_scc2_concurrent_access,
    benchmark_memory_efficiency,
    benchmark_target_40m_ops_sec
);
criterion_main!(benches);