# 🚀 Lithair Examples

> **Philosophy:** Examples demonstrate features. Applications demonstrate possibilities.  
> See [EXAMPLES_PHILOSOPHY.md](../EXAMPLES_PHILOSOPHY.md) for our complete approach.

This directory contains **focused technical examples** that demonstrate specific Lithair features. For complete production-ready applications, see [Lithair-Blog](../../Lithair-Blog/).

---

## 📦 Technical Examples (Feature Demos)

### ⭐ Reference Demo

#### `scc2_server_demo/` — High-Performance HTTP Server
**Feature:** Pure performance with SCC2 lock-free engine

**What it demonstrates:**
- Hyper HTTP server integration
- SCC2 lock-free concurrent operations
- Stateless performance endpoints (`/perf/json`, `/perf/echo`, `/perf/bytes`)
- Gzip compression support

**Quick start:**
```bash
# Using Taskfile (recommended)
task examples:scc2

# Or directly
cd examples/scc2_server_demo
cargo run -- --port 18321 --host 127.0.0.1
```

**Benchmarks:**
```bash
task scc2:demo              # Full demo with benchmarks
task loadgen:json           # JSON throughput test
task loadgen:echo           # Echo latency test
```

---

### 🛡️ Security & Hardening

#### `http_firewall_demo/` — Web Application Firewall
**Feature:** IP filtering, rate limiting, DDoS protection

**What it demonstrates:**
- IP allow/deny lists
- Global and per-IP rate limiting
- Route-level protection
- Anti-DDoS configuration

**Quick start:**
```bash
task examples:firewall

# Or directly
cd examples/raft_replication_demo
cargo run --bin http_firewall_declarative -- --port 8081
```

---

#### `http_hardening_demo/` — Observability & Monitoring
**Feature:** Production observability endpoints

**What it demonstrates:**
- Prometheus metrics (`/observe/metrics`)
- Health checks (`/health`, `/ready`, `/info`)
- Performance testing endpoints (`/observe/perf/*`)
- Structured logging

**Quick start:**
```bash
task examples:hardening

# Or directly
cd examples/raft_replication_demo
cargo run --bin http_hardening_node -- --port 8082
```

---

### 🔄 Distributed Systems

#### `raft_consensus_demo/` — Multi-Node Clustering
**Feature:** Distributed consensus with Raft

**What it demonstrates:**
- Multi-node cluster setup
- Leader election
- Data replication
- Consensus-based writes

**Quick start:**
```bash
# Node 1 (leader)
cargo run -p raft_replication_demo --bin pure_declarative_node -- --node-id 1 --port 8001

# Node 2 (follower)
cargo run -p raft_replication_demo --bin pure_declarative_node -- --node-id 2 --port 8002

# Node 3 (follower)
cargo run -p raft_replication_demo --bin pure_declarative_node -- --node-id 3 --port 8003
```

---

## 🎓 Learning Path

### For Beginners
1. **Start:** `scc2_server_demo` - Understand basic HTTP server
2. **Add security:** `http_firewall_demo` - Protect your API
3. **Add monitoring:** `http_hardening_demo` - Observe your system

### For Advanced Users
1. **Distribution:** `raft_consensus_demo` - Build distributed clusters
2. **Complete app:** [Lithair-Blog](../../Lithair-Blog/) - See everything together

## 🎯 Recommended by Use Case

### Beginner
1. `simple_working_demo.rs` — First contact
2. `http_firewall_demo/` — HTTP Protection
3. `raft_replication_demo/` — Understand replication

### Performance
1. `http_hardening_demo/` — Stateless `/perf/*` endpoints
2. `raft_replication_demo/` — Distributed CRUD bench
3. `scc2_server_demo/` — Hyper + SCC2 max perf

### Production
1. `raft_replication_demo/` — Distributed HTTP cluster
2. `http_firewall_demo/` — Hardening & policies

## 🧹 Cleanup Summary

### Removed (obsolete APIs)
- `blog_platform/` — removed obsolete page API
- `concurrent_crates_benchmark/` — broken deps
- `declarative_*` — obsolete declarative APIs
- `ecommerce_*` — obsolete security APIs
- `rbac_demo.rs` — obsolete RBAC API
- `iot_timeseries/` — incompatible APIs
- and 15+ additional obsolete examples

### Workspace
Top-level `Cargo.toml` references functional projects only:
- `lithair-core`
- `lithair-macros`
- `examples/raft_replication_demo`

## 📊 Stats

- Before cleanup: 33 examples (13 projects + 20 files)
- After cleanup: 9 examples (3 projects + 6 files)
- Kept: 27% (quality > quantity)
- Functional: 100% compile and run

## 🚀 Quick Start for Examples

To get started quickly, use Taskfile commands from the repo root:

```bash
# Show common tasks
task help

# Run SCC2 demo
task scc2:demo

# Run a stateless JSON benchmark
task loadgen:json LEADER=http://127.0.0.1:18321 BYTES=65536 CONC=1024
```

All examples are maintained and tested regularly.
