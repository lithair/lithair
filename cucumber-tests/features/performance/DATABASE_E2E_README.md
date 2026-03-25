# Lithair E2E Database/Performance Tests

## **Philosophy**

These tests are **specific** to the Lithair database/persistence layer:
- Test the **real HttpServer** of Lithair
- Test the **real StateEngine** (event sourcing)
- Test the **real FileStorage** (persistence)
- NOT a test of the complete business application

**Focus**: Persistence integrity + performance

---

## Architecture

```
Cucumber E2E Test
    |
HttpServer (real Lithair)
    |
StateEngine<TestAppState>
    |
FileStorage
    |
events.raftlog + snapshots
```

### **Components Tested**

1. **HttpServer** - Native Lithair HTTP server
   - Keep-alive HTTP/1.1
   - Routing with `Router`
   - Custom handlers

2. **StateEngine** - Event sourcing
   - `apply_event()` - Event application
   - `get_state()` - State retrieval
   - Atomic mutations

3. **FileStorage** - Persistence
   - Writing to `events.raftlog`
   - Snapshots
   - fsync / flush

4. **TestAppState** - Minimal test state
   ```rust
   pub struct TestAppState {
       pub data: TestData,
       pub version: u64,
   }
   ```

---

## File Structure

```
cucumber-tests/
├── features/performance/
│   ├── database_performance.feature       # 19 scenarios
│   ├── DATABASE_E2E_README.md            # This file
│   └── http_performance.feature          # Pure HTTP tests
│
└── src/features/steps/
    ├── real_database_performance_steps.rs # Steps with real Lithair
    ├── http_performance_steps.rs         # HTTP steps (test_server)
    └── database_performance_steps.rs     # Legacy steps (stubs)
```

---

## Test Scenarios

### **1. Integrity Tests** (4 scenarios)

**Create 1000 articles and verify they are ALL persisted**
```gherkin
When I create 1000 articles quickly
Then the events.raftlog file must contain exactly 1000 "ArticleCreated" events
And no event must be missing
```

**Create 10000 articles with 50 threads**
```gherkin
When I create 10000 articles in parallel with 50 threads
Then the ID sequence must be continuous from 0 to 9999
And no duplicate must exist
```

### **2. Performance Tests** (3 scenarios)

**Write performance - 1000 req/s**
```gherkin
When I measure write performance for 10 seconds
Then the server must process at least 1000 requests per second
And the p95 latency must be less than 100ms
```

**Mixed performance 80/20**
```gherkin
When I run a mixed test for 30 seconds with:
  | Type  | Percentage | Concurrency |
  | Read  | 80%        | 100         |
  | Write | 20%        | 20          |
Then the total throughput must be greater than 2000 req/s
```

### **3. Persistence Under Load Tests** (3 scenarios)

**Continuous persistence under high load**
```gherkin
When I run a constant load of 500 req/s for 60 seconds
Then exactly 30000 events must be persisted
And the time sequence must be strictly increasing
```

**Restart with persisted data**
```gherkin
When I stop the server
And I restart the server on the same port
Then the 1000 articles must be present in memory
```

### **4. Advanced Integrity Tests** (2 scenarios)

- **Event order verification**
- **Data corruption detection** (CRC32)

### **5. Extreme Load Tests** (2 scenarios)

- **50000 articles**
- **1000 threads x 10 articles**

### **6. Snapshot Tests** (1 scenario)

- **Snapshot creation every 1000 events**

### **7. Durability Tests** (2 scenarios)

- **fsync durability** (SIGKILL + restart)
- **Durability without fsync** (performance mode)

---

## Implementation

### **Server Startup**

```rust
#[given(expr = "a Lithair server on port {int} with persistence {string}")]
async fn start_lithair_server(world: &mut LithairWorld, port: u16, persist_path: String) {
    // 1. Create FileStorage
    let storage = FileStorage::new(&persist_path).unwrap();
    *world.storage.lock().await = Some(storage);

    // 2. Create the Router
    let engine = world.engine.clone();
    let router = Router::new()
        .post("/api/articles", move |req, _, _| {
            handle_create_article(req, &engine)
        })
        .get("/api/articles", move |req, _, _| {
            handle_list_articles(req, &engine)
        });

    // 3. Start HttpServer
    let server = HttpServer::new().with_router(router);
    let handle = tokio::spawn(async move {
        server.serve_on_port(port).await
    });

    *world.server_handle.lock().await = Some(handle);
}
```

### **Create Handler**

```rust
fn handle_create_article(req: &HttpRequest, engine: &Arc<StateEngine<TestAppState>>) -> HttpResponse {
    // 1. Parse the request
    let article: CreateArticle = serde_json::from_str(req.body()).unwrap();

    // 2. Create the event
    let event = TestEvent::ArticleCreated {
        id: uuid::Uuid::new_v4().to_string(),
        data: json!({ "title": article.title, "content": article.content }),
    };

    // 3. Apply via StateEngine (automatically persists)
    engine.apply_event(event).unwrap();

    // 4. Response
    HttpResponse::created().json(&response_json)
}
```

### **Persistence Verification**

```rust
#[then(expr = "the events.raftlog file must contain exactly {int} events")]
async fn check_event_count(world: &mut LithairWorld, count: usize) {
    let log_file = format!("{}/events.raftlog", world.metrics.persist_path);
    let content = std::fs::read_to_string(&log_file).unwrap();

    let event_count = content.lines()
        .filter(|line| line.contains("ArticleCreated"))
        .count();

    assert_eq!(event_count, count);
}
```

---

## Running the Tests

### **All database/performance tests**
```bash
cd cucumber-tests
cargo test --features cucumber -- features/performance/database_performance.feature
```

### **Integrity tests only**
```bash
cargo test --features cucumber -- "Create 1000 articles"
```

### **Performance tests only**
```bash
cargo test --features cucumber -- "Write performance"
```

---

## Measured Metrics

### **Integrity**
- Exact number of persisted events
- Continuous ID sequence
- No duplicates
- No missing events
- Valid checksums (CRC32)

### **Performance**
- Throughput (req/s)
- Latency (p50, p95, p99)
- Error rate
- Average response time
- events.raftlog file size

### **Durability**
- Recovery after crash (SIGKILL)
- Integrity of persisted data
- Valid snapshots
- Fast restart (< 5s for 50k articles)

---

## Differences with Robot Framework

### **Robot Framework**
- Tests the **complete application**
- Keyword-driven approach
- Easy for non-devs
- Focus: business functionality

### **Cucumber E2E Database/Performance**
- Tests the **database layer** only
- Real HttpServer + StateEngine + FileStorage
- Native Rust, integrated into code
- Focus: persistence integrity + performance

**Complementary!**

---

## Current State

### **Implemented**
- Real Lithair HttpServer startup
- Handlers with StateEngine
- Article creation (sequential)
- Article creation (parallel with threads)
- events.raftlog file verification
- Event counting
- Basic integrity verification

### **To Implement**
- [ ] Performance measurement (throughput, latency)
- [ ] Read tests (GET)
- [ ] Mixed load 80/20
- [ ] Server restart
- [ ] Snapshots
- [ ] CRC32 / checksums
- [ ] Durability tests (SIGKILL)
- [ ] Event order verification

---

## Benefits

1. **Real Tests** - Real Lithair, no mocks
2. **Performance** - Precise measurement with real server
3. **Integration** - Native event sourcing + persistence
4. **Simplicity** - Everything in Cucumber
5. **Total Control** - Programmatic start/stop
6. **Easy Debug** - Direct logs, no external server

---

## Next Steps

1. **Compile the steps** (resolve errors)
2. **Implement missing steps** (perf measurement, reads)
3. **Run 1st scenario** (1000 articles)
4. **Validate integrity** (events.raftlog)
5. **Measure performance** (throughput, latency)
6. **Implement advanced scenarios** (restart, snapshots)

**The architecture is ready, the scenarios are written, we can now implement!**
