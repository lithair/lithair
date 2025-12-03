# 🎯 Lithair Examples Philosophy

**Date:** 2025-10-01  
**Vision:** Clear separation between technical demos and complete applications

---

## 🧭 Core Principle

> **Examples demonstrate features. Applications demonstrate possibilities.**

Lithair follows a clear distinction between:
1. **Technical Examples** - Focused feature demonstrations (in this repo)
2. **Complete Applications** - Production-ready apps (separate repos)

---

## 📦 Technical Examples (This Repository)

### Purpose
Demonstrate **specific Lithair features** with minimal, focused code.

### Characteristics
- ✅ **Focused** - One feature per example
- ✅ **Minimal** - 100-500 lines of code
- ✅ **Educational** - Easy to understand in 5 minutes
- ✅ **Self-contained** - No external dependencies beyond Lithair
- ✅ **Living documentation** - Code that teaches

### Current Examples

#### 1. `scc2_server_demo/` ⭐ REFERENCE
**Feature:** High-performance HTTP server with SCC2 engine

**What it demonstrates:**
- Hyper HTTP server integration
- SCC2 lock-free operations
- Stateless performance endpoints (`/perf/*`)
- Gzip compression

**Use case:** "How fast can Lithair serve HTTP requests?"

**Run:**
```bash
task scc2:serve
```

---

#### 2. `http_firewall_demo/`
**Feature:** Web application firewall

**What it demonstrates:**
- IP filtering (allow/deny lists)
- Rate limiting (global + per-IP)
- Route-level protection
- DDoS protection

**Use case:** "How do I protect my Lithair API?"

**Run:**
```bash
cd examples/raft_replication_demo
cargo run --bin http_firewall_declarative
```

---

#### 3. `http_hardening_demo/`
**Feature:** HTTP hardening & observability

**What it demonstrates:**
- Prometheus metrics
- Performance testing endpoints
- Health checks (`/health`, `/ready`, `/info`)
- Structured logging

**Use case:** "How do I monitor my Lithair application?"

**Run:**
```bash
cd examples/raft_replication_demo
cargo run --bin http_hardening_node
```

---

#### 4. `raft_consensus_demo/`
**Feature:** Distributed consensus with Raft

**What it demonstrates:**
- Multi-node clustering
- Leader election
- Data replication
- Consensus-based writes

**Use case:** "How do I build a distributed Lithair cluster?"

**Run:**
```bash
cd examples/raft_replication_demo
cargo run --bin pure_declarative_node -- --node-id 1 --port 8001
```

---

## 🏗️ Complete Applications (Separate Repositories)

### Purpose
Demonstrate **real-world applications** built with Lithair.

### Characteristics
- ✅ **Production-ready** - Full features, error handling, tests
- ✅ **Complete stack** - Frontend + Backend + Database
- ✅ **Documented** - User guides, API docs, deployment guides
- ✅ **Maintained** - Active development, versioning, releases
- ✅ **Realistic** - Solves real problems

### Current Applications

#### 1. Lithair-Blog ✅
**Repository:** `../Lithair-Blog/`

**What it is:**
- Complete blog platform
- Astro frontend (SSG)
- Lithair backend with event sourcing
- Memory-first architecture
- RBAC with multiple roles

**Features:**
- Article management (CRUD)
- User authentication
- Comment system
- Admin dashboard
- Documentation site

**Use case:** "I want to build a blog with Lithair"

**Run:**
```bash
cd ../Lithair-Blog
task blog:dev
```

---

#### 2. Lithair-ECommerce 🎯 (Planned)
**Repository:** `Lithair-ECommerce/` (future)

**What it will be:**
- Complete e-commerce platform
- Product catalog
- Shopping cart
- Order management
- Payment integration
- Admin panel

**Use case:** "I want to build an online store with Lithair"

---

#### 3. Lithair-Dashboard 🎯 (Planned)
**Repository:** `Lithair-Dashboard/` (future)

**What it will be:**
- Real-time monitoring dashboard
- Metrics visualization
- Alert management
- Multi-tenant support

**Use case:** "I want to build a monitoring system with Lithair"

---

## 🎓 Learning Path

### For Beginners
1. **Start with:** `scc2_server_demo` - Understand the basics
2. **Then try:** `http_firewall_demo` - Add security
3. **Finally:** `Lithair-Blog` - See a complete application

### For Advanced Users
1. **Study:** `raft_consensus_demo` - Understand distributed systems
2. **Explore:** `http_hardening_demo` - Production observability
3. **Build:** Your own application using Lithair

---

## 📝 Guidelines for Contributors

### Adding a Technical Example
**Ask yourself:**
- ✅ Does it demonstrate ONE specific feature?
- ✅ Can it be understood in < 5 minutes?
- ✅ Is it < 500 lines of code?
- ✅ Does it teach something new?

**If yes:** Add it to `examples/`

**If no:** Consider creating a complete application instead

### Creating a Complete Application
**Ask yourself:**
- ✅ Is it production-ready?
- ✅ Does it solve a real problem?
- ✅ Does it showcase multiple Lithair features?
- ✅ Would users deploy this?

**If yes:** Create a separate repository

**If no:** Consider simplifying it into a technical example

---

## 🎯 Benefits of This Approach

### For Developers
- **Quick learning** - Examples teach features fast
- **Real inspiration** - Applications show what's possible
- **Clear path** - From learning to building

### For the Project
- **Focused core** - Framework stays lean
- **Flexible apps** - Applications evolve independently
- **Better maintenance** - Each repo has its own lifecycle

### For Documentation
- **Examples = Reference** - Technical documentation
- **Applications = Tutorials** - Practical guides
- **Clear separation** - No confusion about purpose

---

## 📊 Comparison Table

| Aspect | Technical Example | Complete Application |
|--------|------------------|---------------------|
| **Location** | `Lithair/examples/` | Separate repository |
| **Size** | 100-500 lines | 1000+ lines |
| **Purpose** | Teach one feature | Solve real problem |
| **Audience** | Developers learning | Users deploying |
| **Maintenance** | Framework team | App maintainers |
| **Dependencies** | Lithair only | Full stack |
| **Documentation** | Code comments | User guides |
| **Updates** | With framework | Independent |

---

## 🚀 Current Status

### Technical Examples ✅
- ✅ `scc2_server_demo` - Performance reference
- ✅ `http_firewall_demo` - Security features
- ✅ `http_hardening_demo` - Observability
- ✅ `raft_consensus_demo` - Distributed systems

### Complete Applications
- ✅ `Lithair-Blog` - Blog platform (active)
- 🎯 `Lithair-ECommerce` - E-commerce (planned)
- 🎯 `Lithair-Dashboard` - Monitoring (planned)

---

## 💡 Philosophy in Action

### Example: Adding Firewall Support

**Technical Example (in core repo):**
```rust
// examples/http_firewall_demo/
// 200 lines showing how to use firewall
let firewall = Firewall::new()
    .allow_ip("192.168.1.0/24")
    .rate_limit(100);
```

**Complete Application (separate repo):**
```rust
// Lithair-SecureAPI/
// Full API with authentication, RBAC, firewall, monitoring
// 5000+ lines, production-ready
```

### Example: Learning Path

1. **Read** `http_firewall_demo` → Understand firewall API
2. **Study** `Lithair-Blog` → See firewall in production
3. **Build** Your own app → Apply what you learned

---

## 🎯 Conclusion

This philosophy ensures:
- ✅ **Clear purpose** - Examples teach, applications inspire
- ✅ **Easy learning** - From simple to complex
- ✅ **Maintainable** - Each repo has clear scope
- ✅ **Scalable** - Add examples/apps without bloating core

**Remember:** If you're teaching a feature, write an example. If you're solving a problem, build an application.
