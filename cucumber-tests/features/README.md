# 🥒 Lithair BDD Testing with Cucumber + Gherkin

This folder contains the complete **Behavior-Driven Development (BDD)** test suite for Lithair, using Cucumber and the Gherkin language.

## 📁 Feature Structure

```
features/
├── core/                    # Core framework features
│   ├── performance.feature  # Ultra-high performance tests
│   ├── security.feature     # Enterprise security tests
│   └── distribution.feature # Distribution and consensus tests
├── integration/             # Complete integration tests
│   └── web_server.feature   # Complete web server with frontend
├── persistence/             # Persistence and event sourcing
│   └── event_sourcing.feature # Event persistence tests
├── observability/           # Monitoring and metrics
│   └── monitoring.feature   # Observability tests
├── steps/                   # Gherkin step implementations
│   ├── performance_steps.rs
│   ├── security_steps.rs
│   └── mod.rs
├── world.rs                 # Shared test state
└── lib.rs                   # Public features module
```

## 🚀 How to Use

### Installation
```bash
task bdd:setup
```

### Run all tests
```bash
task bdd:run
```

### Tests by category
```bash
task bdd:performance    # Performance tests
task bdd:security       # Security tests
task bdd:distribution   # Distribution tests
task bdd:integration    # Integration tests
task bdd:persistence    # Persistence tests
task bdd:observability  # Observability tests
```

### CI/CD with BDD
```bash
task ci:bdd    # Full CI with BDD tests
task bdd:ci    # CI mode (JSON output)
```

## 📋 Covered Scenarios

### 🚀 Ultra-High Performance
- HTTP server with maximum performance
- JSON throughput benchmark
- Massive concurrency
- Performance evolution under load

### 🛡️ Enterprise Security
- DDoS attack protection
- Role-based access control (RBAC)
- JWT token validation
- Geographic IP filtering
- Rate limiting per endpoint

### 🔄 Distribution and Consensus
- Leader election
- Data replication
- Network partition and split-brain
- Joining an existing cluster
- Horizontal scalability

### 🌐 Complete Web Server
- HTML page serving
- Complete CRUD API
- CORS for external frontend
- Real-time WebSockets
- Intelligent asset caching

### 💾 Event Sourcing and Persistence
- Event persistence
- State reconstruction
- Optimized snapshots
- Event deduplication
- Recovery after corruption

### 📊 Observability and Monitoring
- Complete health checks
- Prometheus metrics
- Performance profiling
- Structured logging
- Automatic alerts

## 🔧 Technical Architecture

### Shared World
Tests use a `LithairWorld` structure that maintains:
- Server state (port, PID, running status)
- Performance metrics
- Test data (articles, users, tokens)
- Last HTTP response
- Encountered errors

### Reusable Steps
Each test category has its steps:
- **Performance**: server startup, request sending, measurements
- **Security**: authentication, authorization, rate limiting
- **Distribution**: clustering, replication, consensus
- **Integration**: CRUD APIs, CORS, WebSockets

### Dynamic Configuration
Tests can be configured with:
- Environment variables (RUST_LOG, PORT, etc.)
- External configuration files
- Command line parameters

## 📈 Reports and Results

### Standard Output
```
🥒 Cucumber Results:
✅ 45 scenarios passed
❌ 2 scenarios failed
📊 95.7% success rate
⏱️  Total time: 3m 24s
```

### JSON Report (CI)
```bash
task bdd:ci
# Generates test-results/cucumber-results.json
```

### GitHub Actions Integration
BDD tests integrate perfectly into the CI pipeline:
```yaml
- name: Run BDD Tests
  run: task ci:bdd
```

## 🎯 Benefits of BDD for Lithair

1. **Living documentation**: Features serve as technical documentation
2. **Collaboration**: Common language between developers, QA and product owners
3. **Traceability**: Each bug can be linked to a specific scenario
4. **Regression**: Complete automatic tests after each change
5. **Customer vision**: Focus on user behavior rather than implementation

## 🔄 Migration from Examples

Traditional examples are progressively migrated:
- `scc2_server_demo/` → `performance.feature`
- `http_firewall_demo/` → `security.feature`
- `raft_replication_demo/` → `distribution.feature`
- `blog_server/` → `web_server.feature`

This approach allows:
- Preserving existing functionality
- Adding a BDD validation layer
- Improving test coverage
- Facilitating maintenance

## 🚀 Next Steps

1. **Complete** missing step definitions
2. **Add** extreme load scenarios
3. **Integrate** with existing benchmarks
4. **Automate** report generation
5. **Extend** to negative testing

---

**Lithair BDD** - Transforming the way we test ultra-performant distributed systems! 🚀
