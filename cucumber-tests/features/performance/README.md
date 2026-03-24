# Lithair HTTP Performance Tests

## Objective

Validate the Lithair HTTP server performance with E2E Cucumber tests:
- Throughput (req/s)
- Latency (p50, p95, p99)
- Stability under load
- Keep-Alive HTTP/1.1
- Persistence with fsync

---

## Test Scenarios

### **1. Write Throughput**
```gherkin
When I create 1000 articles in parallel with 10 workers
Then the throughput must be greater than 1000 requests per second
```

**Objective**: >= 1000 req/s
**Workers**: 10
**Validation**: Persistence + no errors

### **2. Read Throughput**
```gherkin
When I read the article list 5000 times with 20 workers
Then the throughput must be greater than 5000 requests per second
```

**Objective**: >= 5000 req/s
**Workers**: 20
**Validation**: Latency p95 < 50ms

### **3. Mixed Load 80/20**
```gherkin
When I run a mixed load for 10 seconds:
  | type  | percentage | workers |
  | read  | 80         | 16      |
  | write | 20         | 4       |
Then the total throughput must be greater than 2000 requests per second
```

**Objective**: >= 2000 req/s total
**Mix**: 80% reads / 20% writes
**Validation**: Error rate < 0.1%

### **4. Performance with fsync**
```gherkin
Given the server has fsync enabled on each write
When I create 500 articles sequentially
Then the total time must be less than 2 seconds
```

**Objective**: < 2s for 500 articles
**Validation**: Zero loss after brutal kill

### **5. Keep-Alive HTTP/1.1**
```gherkin
When I make 100 requests with the same TCP connection
Then no "Connection reset" error must occur
```

**Objective**: 1 single TCP connection
**Validation**: No "Connection reset by peer"

---

## Architecture

```
cucumber-tests/
├── features/performance/
│   ├── http_performance.feature    # Gherkin scenarios
│   └── README.md                   # This file
│
└── src/features/steps/
    └── http_performance_steps.rs   # Implementation
```

### **World State**

```rust
pub struct Metrics {
    // Performance
    pub throughput: f64,              // req/s
    pub total_duration: Duration,
    pub error_count: usize,

    // Latency
    pub latency_p50: Duration,
    pub latency_p95: Duration,
    pub latency_p99: Duration,

    // Server
    pub base_url: String,
    pub server_port: u16,
    pub persist_path: String,
}
```

---

## Running the Tests

### **All performance tests**
```bash
cargo test --features cucumber -- --tags @performance
```

### **Critical tests only**
```bash
cargo test --features cucumber -- --tags "@performance and @critical"
```

### **Specific test**
```bash
cargo test --features cucumber -- --name "Write throughput"
```

---

## Measured Metrics

### **Throughput**
- **Definition**: Number of requests/second
- **Calculation**: `total_requests / duration_seconds`
- **Objectives**:
  - Write: >= 1000 req/s
  - Read: >= 5000 req/s
  - Mixed: >= 2000 req/s

### **Latency**
- **p50 (median)**: 50% of requests
- **p95**: 95% of requests
- **p99**: 99% of requests
- **Objectives**:
  - p50 < 10ms
  - p95 < 50ms
  - p99 < 100ms

### **Error Rate**
- **Definition**: `failed_requests / total_requests * 100`
- **Objective**: < 0.1%

---

## Implementation

### **Parallel Workers**

```rust
let articles_per_worker = count / workers;
let mut handles = vec![];

for worker_id in 0..workers {
    let handle = thread::spawn(move || {
        let client = Client::new();
        for i in 0..articles_per_worker {
            // Create article
            // Measure latency
        }
    });
    handles.push(handle);
}

for handle in handles {
    handle.join().unwrap();
}
```

### **Latency Measurement**

```rust
let start = Instant::now();
let response = client.post(&url).json(&article).send();
let latency = start.elapsed();

metrics.latencies.push(latency);
```

### **Percentile Calculation**

```rust
pub fn calculate_percentile(&self, percentile: f64) -> Duration {
    let mut sorted = self.latencies.clone();
    sorted.sort();

    let index = ((percentile / 100.0) * sorted.len() as f64) as usize;
    sorted[index.min(sorted.len() - 1)]
}
```

---

## Identified Issues

### **1. Connection Reset**
**Symptom**: `ConnectionResetError(104, 'Connection reset by peer')`

**Cause**: Server closes the connection after each request

**Solution**:
```rust
// In test_server, read multiple requests on the same connection
loop {
    let mut buffer = [0; 4096];
    match stream.read(&mut buffer) {
        Ok(0) => break, // Client closed
        Ok(_) => {
            // Process request
            // Send response
            // Continue
        }
        Err(_) => break,
    }
}
```

### **2. Low Performance (133 req/s)**
**Cause**: Basic HTTP server with `std::net`

**Solutions**:
1. **Short term**: Temporarily adjust objectives
2. **Medium term**: Use tokio for async
3. **Long term**: Integrate hyper into Lithair

---

## TODO

### **Step Implementation**
- [x] Write throughput
- [x] Read throughput
- [ ] Mixed load
- [ ] Keep-Alive HTTP/1.1
- [ ] Concurrent load
- [ ] Latency under load
- [ ] Stress test
- [ ] Reference benchmark

### **Server Optimizations**
- [ ] Support HTTP/1.1 keep-alive
- [ ] Thread pool for connections
- [ ] Optimized HTTP parser
- [ ] tokio/hyper integration

### **CI/CD**
- [ ] Integrate into CI pipeline
- [ ] Automated benchmarks
- [ ] Regression alerts
- [ ] Performance reports

---

## References

- [Robot Framework Tests](../../robot-tests/) - Similar tests
- [test_server](../../examples/test_server/) - Test server
- [Lithair HTTP](../../lithair-core/src/http/) - Framework HTTP module

---

## Next Steps

1. **Fix Connection Reset** (priority 1)
2. **Implement missing steps** (mixed load, keep-alive)
3. **Optimize test_server** or integrate Lithair HTTP
4. **Validate all scenarios**
5. **Integrate into CI**

**These E2E Cucumber tests are specific to Lithair and complementary to the Robot Framework tests!**
