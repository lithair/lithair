# Lithair Examples

Progressive examples from "Hello World" to distributed clusters.
Each example builds on the previous one — start at 01, work your way up.

## Quick Start

```bash
# Run any example
cargo run -p hello-world
cargo run -p static-site
cargo run -p rest-api
cargo run -p blog -- --port 3000
```

## Examples

| #  | Name | What you learn | Lines |
|----|------|---------------|-------|
| 01 | [hello-world](01-hello-world/) | LithairServer basics, config, admin panel | 42 |
| 02 | [static-site](02-static-site/) | Serve static files from SCC2 memory | 45 |
| 03 | [rest-api](03-rest-api/) | DeclarativeModel, auto-generated CRUD | 88 |
| 04 | [blog](04-blog/) | RBAC, sessions, event sourcing, frontend | 333 |
| 05 | [ecommerce](05-ecommerce/) | Relations, foreign keys, multi-model | 407 |
| 06 | [auth-sessions](06-auth-sessions/) | Session-based auth, permission checker | 311 |
| 07 | [auth-rbac-mfa](07-auth-rbac-mfa/) | RBAC + MFA/TOTP, SSO patterns | 438 |
| 08 | [schema-migration](08-schema-migration/) | 4 migration modes, admin API | 1191 |
| 09 | [replication](09-replication/) | Raft consensus, multi-node cluster | — |
| 10 | [blog-distributed](10-blog-distributed/) | Blog + Raft replication combined | — |

### Advanced

| Name | Purpose |
|------|---------|
| [datatable](advanced/datatable/) | Interactive data table with pagination |
| [stress-test](advanced/stress-test/) | Load testing and benchmarks |
| [consistency-test](advanced/consistency-test/) | Distributed consistency verification |
| [playground](advanced/playground/) | Multi-model experimentation sandbox |

## Learning Path

**Building a website?** 01 → 02 → 03

**Building an API?** 01 → 03 → 04

**Need auth?** 04 → 06 → 07

**Going distributed?** 09 → 10

## For AI Agents

Each example is self-contained with:
- A single `src/main.rs` entry point
- Clear doc comments explaining every concept
- Real curl commands to test the API
- A `Cargo.toml` with minimal dependencies

To bootstrap a new Lithair project, read `03-rest-api` for the simplest CRUD pattern,
or `04-blog` for a full-featured application.
