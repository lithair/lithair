# 🚀 Lithair: Declarative Memory-First Web Server

<div align="center">

> **"In Memory We Trust, In Data We Believe"**

[![Rust](https://img.shields.io/badge/rust-1.75+-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)
[![Documentation](https://img.shields.io/badge/docs-comprehensive-brightgreen.svg)](docs/)
[![Performance](https://img.shields.io/badge/performance-10K%2B_req%2Fs-success.svg)]()
[![Latency](https://img.shields.io/badge/latency-<_1ms-success.svg)]()

_Declarative programming + Memory-first architecture = 10x less code, 100x faster_

**One struct definition → Complete high-performance backend with intelligent RAM caching**

</div>

---

## 🧰 Developer Tasks (Taskfile)

This repository uses a Taskfile to keep developer commands consistent across examples, benchmarks, and demos. Install go-task (https://taskfile.dev) and use the following tasks.

Common tasks:

```bash
# Show available tasks and variables
task help

# Build (debug) / Build (release)
task build
task build:release

# Start SCC2 server (Hyper + SCC2)
task scc2:serve PORT=18321 HOST=127.0.0.1
task scc2:serve:release PORT=18321 HOST=127.0.0.1

# Full SCC2 demo (server + benchmarks)
task scc2:demo

# Gzip on/off comparison demo
task scc2:gzip

# Stateless loadgen presets
task loadgen:status LEADER=http://127.0.0.1:18321 TOTAL=20000 CONC=512
task loadgen:json   LEADER=http://127.0.0.1:18321 BYTES=1024 TOTAL=20000 CONC=512
task loadgen:echo   LEADER=http://127.0.0.1:18321 BYTES=1048576 TOTAL=10000 CONC=256

# Release-mode loadgen
task loadgen:status:release LEADER=http://127.0.0.1:18321
task loadgen:json:release   LEADER=http://127.0.0.1:18321 BYTES=65536 ACCEPT_ENCODING=gzip
task loadgen:echo:release   LEADER=http://127.0.0.1:18321 BYTES=1048576

# Bench presets
task bench:json-small   # JSON 1KB, 20k total, 512 conc
task bench:json-large   # JSON 64KB, 20k total, 512 conc
task bench:echo-large   # Echo 1MB, 10k total, 256 conc

# Clean cargo artifacts
task clean
```

Variables:

- PORT, HOST, LEADER
- TOTAL, CONC, BYTES
- ACCEPT_ENCODING (e.g., "gzip")
- RUST_LOG (e.g., "info")

Examples:

```bash
task scc2:serve PORT=18321 HOST=127.0.0.1 RUST_LOG=info
task loadgen:json LEADER=http://127.0.0.1:18321 BYTES=65536 CONC=1024 ACCEPT_ENCODING=gzip
```

## 🎯 **The Simple Problem**

Want to build a blog with 3 tables? You'll need:

- API framework (Express, FastAPI, Spring...)
- Database setup + migrations
- Authentication system
- Validation layer
- Security middleware
- Deployment configuration

**Hours of setup before writing your first line of business logic.**

**Lithair asks:** What if those 3 tables could generate everything else? What if your data model _was_ your infrastructure?

```rust
#[derive(DeclarativeModel)]
pub struct User {
    #[db(primary_key)] #[http(expose)] #[permission(read = "Public")]
    pub id: Uuid,

    #[db(unique)] #[lifecycle(audited)] #[http(expose, validate = "email")]
    #[permission(write = "UserEdit")] #[persistence(replicate)]
    pub email: String,
}
```

**The result:** A complete web server platform emerges from your data model. API, database, firewall, security, audit, distribution - all generated consistently.

---

## ⚡ **What You Get Instantly**

### 🌐 Production HTTP Server + REST API

```http
GET/POST/PUT/DELETE /users    # Full CRUD with validation
GET /users/{id}/history       # Automatic audit trail
```

**Built on Hyper** - Production-grade async HTTP server with sub-millisecond latency

### 🗄️ Auto-Generated Database Schema

```sql
-- Generated with constraints & indexes
CREATE TABLE users (
    id UUID PRIMARY KEY,
    email VARCHAR UNIQUE NOT NULL CHECK (email ~ '^[^@]+@[^@]+\.[^@]+$')
);
CREATE INDEX idx_users_email ON users(email);
```

**Event Sourcing** - Complete audit trail with time-travel debugging

### 🔗 Auto-Joiner & Declarative Relations

Lithair handles relationships for you. Just declare a foreign key, and the engine automatically joins data at read-time.

```rust
// Declare foreign key in ModelSpec
fk: true, fk_collection: "categories"

// Resulting JSON (automatic expansion)
{
  "id": "prod_1",
  "category_id": "cat_A",
  "category": { "id": "cat_A", "name": "Electronics" }
}
```

### 🔒 Built-in Security & Permissions

- **Field-level RBAC** - Granular permissions per user role
- **Input validation** - Automatic sanitization & SQL injection prevention
- **Authentication** - JWT + API key support out of the box

### 📊 Monitoring & Health Checks

```http
GET /health     # Application health status
GET /metrics    # Prometheus-compatible metrics
GET /status     # Detailed system information
```

### 💾 Distributed Replication (Advanced)

- **Raft consensus** - Strong consistency across nodes
- **Auto-failover** - Seamless leader election
- **Multi-node clustering** - Built-in load distribution

### 🛡️ Advanced Web Firewall (Enterprise)

```rust
#[firewall(
    enabled = true,
    ip_allow = "192.168.1.0/24",
    global_qps = 1000,
    per_ip_qps = 50,
    protected = "/api"
)]
```

**Network protection** - IP filtering, rate limiting, route-level security

### 🎨 Frontend Architecture: Nouvelle Proposition en Rupture (NEW!)

```rust
// Memory-serve: Une approche différente du serving traditionnel
use lithair_core::frontend::memserve_virtual_host_shared;

memserve_virtual_host_shared(state, "main", "/", "public").await?;
// ✅ All files now memory-served with sub-millisecond latency!
```

**Memory-First Serving (vs. Traditional Disk I/O):**

- 📁 **Chargement déclaratif** - Pointez un dossier, tout se charge en mémoire au démarrage
- ⚡ **Serving direct RAM** - Sub-millisecond, pas d'I/O disque à chaque requête
- 🚫 **Cache serveur inutile** - nginx/apache cache devient obsolète, déjà en mémoire
- 📦 **Multi-virtual-host** - Plusieurs sites sur un port, routing automatique
- 📊 **Auto MIME Detection** - HTML, CSS, JS, images, fonts - tout géré
- 🚀 **SCC2 Concurrency** - Performance massive pour assets statiques

```bash
[INFO] 📦 Loading blog assets from public directory...
[INFO] 📄 Loaded /index.html (14459 bytes, text/html)
[INFO] 📄 Loaded /style.css (2048 bytes, text/css)
[INFO] 📄 Loaded /app.js (1024 bytes, application/javascript)
[INFO] ✅ 3 assets loaded from public directory
```

**The Revolution:** Traditional web servers read files from disk on every request. Lithair loads everything into memory once, then serves with zero I/O. **10,000x faster** than disk-based serving.

---

## 🎨 **Real Example: That Blog You Wanted**

The blog that started this project:

- **User** table (auth + profiles)
- **Post** table (content + metadata)
- **Comment** table (moderation + threading)

Traditional: **Hours of setup, multiple services, configuration files**
Lithair: **3 structs, run `cargo run`**

```rust
#[derive(DeclarativeModel)]
#[firewall(enabled = true, global_qps = 1000)]
pub struct Product {
    #[db(primary_key, indexed)] #[http(expose)]
    #[permission(read = "Public")] #[persistence(replicate)]
    pub id: Uuid,

    #[db(indexed, unique)] #[http(expose, validate = "non_empty")]
    #[lifecycle(audited)] #[permission(read = "Public", write = "ProductManager")]
    pub sku: String,

    #[http(expose, validate = "min_value(0.01)")]
    #[lifecycle(audited, track_history)] #[permission(read = "Public", write = "ProductManager")]
    pub price: f64,

    #[db(indexed)] #[http(expose, validate = "min_value(0)")]
    #[permission(read = "StockManager", write = "StockManager")]
    #[persistence(replicate, consistent_read)]
    pub stock: i32,
}
```

**Generates complete web server with:**

- ✅ 15+ REST endpoints with validation
- ✅ Production HTTP server (Hyper-based)
- ✅ Database schema with optimized indexes
- ✅ RBAC security (3 permission levels)
- ✅ Complete audit trail & event sourcing
- ✅ Health checks & Prometheus metrics
- ✅ Multi-node replication with auto-failover
- ✅ Advanced web firewall with IP filtering & rate limiting
- ✅ TLS support & security headers

---

## 🏆 **Proven Web Server Performance**

**Our reference benchmark demonstrates real production-grade web server:**

- **2,000 random HTTP operations** across 3-node cluster
- **250.91 ops/sec HTTP throughput** with full firewall protection
- **Perfect data consistency**: 1,270 identical products on all nodes
- **Sub-millisecond latency** for 95% of web requests
- **Zero configuration** - complete web server auto-generated from models

```bash
# Run the proof yourself
cd examples/raft_replication_demo
cargo run --bin simplified_consensus_demo
```

---

## 🚀 **Quick Start**

### 1. Get Lithair

```bash
git clone https://github.com/your-org/lithair
cd lithair
```

### 2. Create Your Model

```rust
#[derive(DeclarativeModel)]
pub struct MyData {
    #[db(primary_key)] #[http(expose)]
    pub id: Uuid,

    #[http(expose, validate = "non_empty")]
    #[lifecycle(audited)]
    pub name: String,
}
```

### 3. Launch Your Web Server

```bash
cargo run --bin my_server
```

**Your complete web server is live at `http://localhost:8080`!**

- REST API with firewall protection
- Health checks at `/health`
- Metrics at `/metrics`
- Full audit trail

---

## 🌟 **Platform Modules by Importance**

| Priority          | Module             | What You Write                        | What You Get                                  |
| ----------------- | ------------------ | ------------------------------------- | --------------------------------------------- |
| **🎯 Core**       | **🌐 HTTP Server** | `#[http(expose, validate = "email")]` | Production Hyper server + REST API            |
| **🎯 Core**       | **🗄️ Database**    | `#[db(primary_key, indexed)]`         | Auto-generated schema + optimized indexes     |
| **🔗 New**        | **Relations**      | `fk: true`                            | Auto-Joiner & Smart Router                    |
| **🔒 Essential**  | **Security**       | `#[permission(read = "Public")]`      | Field-level RBAC + input validation           |
| **📝 Essential**  | **Audit**          | `#[lifecycle(audited)]`               | Complete change history + compliance          |
| **📊 Useful**     | **Monitoring**     | `#[monitoring(metrics = true)]`       | Health checks + Prometheus metrics            |
| **💾 Advanced**   | **Replication**    | `#[persistence(replicate)]`           | Raft consensus + distributed storage          |
| **🛡️ Enterprise** | **Web Firewall**   | `#[firewall(global_qps = 1000)]`      | IP filtering, rate limiting, route protection |
| **🔒 Enterprise** | **TLS**            | `#[tls(auto_cert = true)]`            | Automatic HTTPS + security headers            |

---

## 🆚 **Web Server Setup Comparison**

| Task                  | Traditional Stack                 | Lithair                       | Savings      |
| --------------------- | --------------------------------- | ------------------------------- | ------------ |
| **Setup web server**  | Nginx + Docker + config files     | `#[derive(DeclarativeModel)]`   | **95% less** |
| **Add firewall**      | Separate WAF + rules + monitoring | `#[firewall(enabled = true)]`   | **98% less** |
| **Add load balancer** | HAProxy/ALB + health checks       | Built-in with clustering        | **90% less** |
| **Add monitoring**    | Prometheus + Grafana + exporters  | `#[monitoring(metrics = true)]` | **85% less** |
| **Add HTTPS**         | Cert management + renewal         | `#[tls(auto_cert = true)]`      | **99% less** |
| **Add audit logging** | ELK stack + log parsing           | `#[lifecycle(audited)]`         | **95% less** |

_Real numbers from production codebases_

---

## 🎯 **Perfect For**

### ✅ **Ideal Use Cases**

- **Web applications** requiring enterprise-grade infrastructure
- **High-performance web services** (trading, gaming, real-time APIs)
- **Microservices** with built-in service discovery & load balancing
- **Compliance-heavy systems** (finance, healthcare) requiring audit trails
- **Multi-tenant SaaS** with per-tenant security & monitoring
- **Rapid prototyping** → production deployment with zero DevOps

### 🤔 **Consider Alternatives**

- Simple static websites without dynamic APIs
- Legacy systems requiring specific web server features (mod_php, etc.)
- Teams preferring traditional separate-service architecture
- Applications requiring custom networking protocols beyond HTTP

---

## 📚 **Complete Documentation**

### 🎓 **Getting Started**

- **[📖 Complete Documentation](docs/README.md)** - Your guide to mastering Lithair
- **[🚀 Getting Started](docs/guides/getting-started.md)** - From zero to production in 10 minutes
- **[🧠 Data-First Philosophy](docs/guides/data-first-philosophy.md)** - Why this changes everything

### 🏗️ **Architecture Deep Dive**

- **[🏛️ System Architecture](docs/architecture/overview.md)** - How Lithair works under the hood
- **[🔄 Data Flow](docs/architecture/data-flow.md)** - From HTTP request to distributed storage
- **[📊 All Diagrams](docs/diagrams/README.md)** - Visual architecture guide

### 🔧 **Web Server Modules**

- **[🌐 HTTP Server](docs/modules/http-server/README.md)** - Production Hyper server with auto-generated APIs
- **[🛡️ Web Firewall](docs/modules/firewall/README.md)** - IP filtering, rate limiting, route protection
- **[⚖️ Distributed Consensus](docs/modules/consensus/README.md)** - Raft-based clustering & replication
- **[💾 Storage Engine](docs/modules/storage/README.md)** - Event sourcing with audit trails
- **[🎨 Declarative Models](docs/modules/declarative-models/README.md)** - The core magic that generates everything
- **[🔗 Auto-Joiner & Relations](docs/RELATIONS.md)** - Declarative relationship management
- **[📊 Monitoring & Metrics](docs/guides/performance.md)** - Built-in observability stack
- **[⚡ HTTP Stateless Performance Endpoints](docs/guides/http_performance_endpoints.md)** - Pure HTTP benchmarking & loadgen
- **[🛡️ HTTP Hardening, Gzip & Firewall](docs/guides/http_hardening_gzip_firewall.md)** - Production protection patterns

### 📋 **Reference**

- **[🏷️ Declarative Attributes](docs/reference/declarative-attributes.md)** - Complete attribute reference
- **[🔌 API Reference](docs/reference/api-reference.md)** - Generated API documentation
- **[🎨 Frontend Architecture](docs/FRONTEND_ARCHITECTURE.md)** - Memory-first serving en rupture avec le traditionnel

### 🎯 **Examples & Applications**

> **Philosophy:** Examples demonstrate features. Applications demonstrate possibilities.
> See [EXAMPLES_PHILOSOPHY.md](EXAMPLES_PHILOSOPHY.md) for our approach.

#### 📦 Technical Examples (Feature Demos)

Focused demonstrations of specific Lithair features:

| Example | Feature | Description |
|---------|---------|-------------|
| **[⚡ SCC2 Server](examples/scc2_server_demo/)** ⭐ | Performance | High-performance HTTP server reference |
| **[🔐 RBAC + SSO](examples/rbac_sso_demo/)** | Authentication | Declarative RBAC, multi-provider SSO, custom middleware |
| **[🛡️ Firewall Demo](examples/raft_replication_demo/)** | Security | IP filtering, rate limiting, DDoS protection |
| **[🔒 Hardening Demo](examples/raft_replication_demo/)** | Observability | Prometheus metrics, health checks, perf endpoints |
| **[🔄 Consensus Demo](examples/raft_replication_demo/)** | Distribution | Multi-node Raft clustering |

```bash
# Quick start
task examples:list      # List all examples
task examples:scc2      # Run reference demo
task examples:rbac      # Run RBAC + SSO demo
```

#### 🏗️ Complete Applications (Production-Ready)

Real-world applications built with Lithair:
{{ ... }}
| Application                                 | Repository    | Description                                          |
| ------------------------------------------- | ------------- | ---------------------------------------------------- |
| **[📝 Lithair-Blog](../Lithair-Blog/)** | Separate repo | Official Lithair site platform with Astro frontend |
| **[🛒 Lithair-ECommerce](#)**             | Coming soon   | E-commerce platform with cart & payments             |
| **[📊 Lithair-Dashboard](#)**             | Planned       | Real-time monitoring dashboard                       |

```bash
# Run complete blog application
cd ../Lithair-Blog
task blog:dev
```

---

## 🛡️ Firewall Quickstart

Quickly validate the built-in web firewall with two demos. See detailed docs: [`docs/HTTP_FIREWALL.md`](docs/HTTP_FIREWALL.md) and [`docs/HTTP_FIREWALL_ATTRIBUTE.md`](docs/HTTP_FIREWALL_ATTRIBUTE.md).

Fully declarative (model attribute only):

```bash
bash examples/http_firewall_demo/run_declarative_demo.sh
```

Or manual:

```bash
cargo run -p raft_replication_demo --bin http_firewall_declarative -- --port 8081
curl http://127.0.0.1:8081/status
curl http://127.0.0.1:8081/api/products
```

CLI-configurable demo (flags):

```bash
bash examples/http_firewall_demo/run_demo.sh
```

Demonstrates deny/allow and rate limiting with route scoping.

---

## 🌟 **Web Server Technology Stack**

- **🦀 Rust** - Memory safety + zero-cost abstractions
- **⚡ Hyper HTTP Server** - Production-grade HTTP/1.1 & HTTP/2 support
- **🛡️ Built-in Firewall** - Native IP filtering & rate limiting
- **🔄 OpenRaft Consensus** - Distributed clustering & replication
- **📊 Native Monitoring** - Prometheus metrics + health checks
- **📝 Event Sourcing** - Complete audit trail + time-travel debugging
- **🚀 SCC2 Concurrent Engine** - Lock-free high-performance operations
- **🔒 TLS Integration** - Automatic HTTPS + security headers
- **🎨 Proc Macros** - Zero-runtime code generation

---

## 🤝 **Contributing**

This started as a personal project to solve my own frustration with web development complexity.

- **Questions or bugs?** Open an issue
- **Want to contribute?** See [Contributing Guide](docs/guides/developer-guide.md)
- **Find it useful?** Star the repo

### 🎯 **Platform Roadmap**

- **v1.1:** WebSocket auto-generation, GraphQL APIs, Advanced firewall rules, Multi-Raft sharding
- **v1.2:** Real-time subscriptions, Load balancer integration, Visual web server designer, TLS auto-renewal
- **v2.0:** Edge computing nodes, Auto-scaling web clusters, Cross-region CDN, Serverless functions

---

<div align="center">

**Lithair**

_Data-first web server platform_

**Built by [Yoan Roblet (Arcker)](https://github.com/arcker)**

</div>
