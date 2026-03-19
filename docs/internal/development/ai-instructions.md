# Lithair : Declarative Memory-First Web Server

> **"In Memory We Trust, In Data We Believe"**

## 🚀 What This Project Is

Lithair is a Rust framework centered on **declarative models** and a
**memory-first runtime**.

The goal is not to replace every backend stack in every situation. The goal is
to reduce avoidable coordination work between persistence, HTTP exposure,
validation, permissions, and operational tooling when those concerns can be
described from the same model surface.

**Core philosophy:**

- **In Memory We Trust**: keep hot paths simple and fast
- **In Data We Believe**: let the data model carry more intent

## 🧭 Practical Mental Model

### Traditional layered approach

```text
Controller ──► Service ──► Database
    ▲           ▲           ▲
  Routes     Business    Tables
Validation    Logic     Queries
Permissions   Audit     Triggers
```

### Lithair declarative approach

```rust
#[derive(DeclarativeModel)]
pub struct User {
    #[db(primary_key)]
    #[lifecycle(immutable)]
    #[http(expose)]
    #[persistence(replicate)]
    #[permission(read = "UserRead")]
    pub id: Uuid,
}
```

This does not eliminate all complexity, but it can concentrate a meaningful
part of it in one place.

## 📁 Project Structure

```text
lithair-core/       # Framework implementation
lithair-macros/     # Declarative model macros
examples/           # Public runnable examples
docs/               # Guides, references, internals
cucumber-tests/     # BDD coverage
```

### Current public examples model

- `examples/01-*` to `examples/15-*` form the main learning catalog
- `examples/advanced/*` contains advanced validation and operational scenarios
- `examples/09-replication` is the main public surface for clustering and
  benchmark scripts

## ⚡ Key Commands

### Build & test

```bash
cargo build --release
cargo test
cargo clippy
cargo fmt
```

### Run current examples

```bash
# Smallest server
cargo run -p hello-world

# Auth + sessions example
cargo run -p auth-sessions

# Distributed replication example
cargo run -p replication --bin replication-declarative-node -- \
  --node-id 1 --port 8080

# Load generator
cargo run --release -p replication --bin replication-loadgen -- \
  --leader http://127.0.0.1:8080 \
  --total 3000 \
  --concurrency 256 \
  --mode random
```

## 🏗️ Core Technologies

- **Rust** as the implementation language
- **Hyper** for HTTP
- **Event sourcing** for persistence and auditability
- **Declarative models** with attribute-driven behavior
- **OpenRaft** for distributed consensus scenarios
- **RBAC** and validation integrated into model-driven flows

## 🎨 Representative Declarative Model

```rust
#[derive(DeclarativeModel)]
pub struct Product {
    #[db(primary_key, indexed)]
    #[http(expose)]
    #[persistence(replicate)]
    pub id: Uuid,

    #[db(indexed, unique)]
    #[http(expose, validate = "non_empty")]
    #[permission(read = "ProductRead", write = "ProductWrite")]
    pub name: String,

    #[db(indexed)]
    #[http(expose, validate = "min_value(0.01)")]
    #[permission(read = "ProductRead", write = "ProductWrite")]
    pub price: f64,
}
```

Representative outcomes of this style:

- REST exposure can be generated from the model surface
- validation rules stay close to the fields they govern
- permission rules remain attached to the data they protect
- persistence and replication behavior can be configured from the same place

## 🔥 Development Philosophy

### In Memory We Trust 💾

Memory-first design is useful when it reduces hot-path overhead and keeps the
runtime simple to reason about.

### In Data We Believe 📊

The struct should carry as much durable intent as possible:

1. **Declarative over imperative**
2. **Data as a source of truth**
3. **Less boilerplate where possible**
4. **Type safety by default**
5. **Security and validation close to the model**

## 📊 Contributor Guidance

When describing Lithair internally or externally:

- avoid claiming that one benchmark universally proves the framework
- prefer workload-specific language over absolute performance claims
- present examples from the current public catalog, not deleted historical
  demos
- keep room for hybrid architectures and pragmatic trade-offs

## 📚 Essential Documentation

- `examples/README.md` – public examples index
- `docs/examples/overview.md` – examples catalog overview
- `docs/reference/proven-benchmarks.md` – benchmark framing and entry points
- `docs/reference/http-loadgen.md` – load generator reference
- `docs/guides/data-first-philosophy.md` – conceptual framing

## 🎭 Mental Model Shift

**Traditional question:** "How do I implement this feature?"

**Lithair question:** "What properties does this data have, and which ones can
be declared once?"

That shift is where Lithair is usually most helpful.

# Lithair Rust Guidelines (Clippy + Idiomatic Best Practices)

This document defines the Rust quality standards for Lithair. All contributors
(humans and agents) must follow these rules. The goal is to keep code safe,
idiomatic, and performant, with zero warnings on `cargo check` and clean,
actionable `cargo clippy` output.

## Policy

- Always build with stable Rust and keep code compatible with our
  `rust-toolchain.toml`.
- Run `cargo check` frequently and keep it warning-free.
- Run `cargo clippy` regularly. Fix actionable lints; justify or suppress only
  when necessary.
- Prefer small, focused PRs with clear commit messages.

## Required Clippy/Idiomatic Fixes

- Unwrap patterns
  - Do not `unwrap()` after checking an `Option`/`Result` with `is_some`/`is_ok`.
  - Use `if let`, `match`, or combinators (`map`, `and_then`, `ok_or_else`) instead.

- Default implementations
  - If a type provides a reasonable empty/default state and has a `new()` constructor, implement/derive `Default`.
  - Prefer `#[derive(Default)]` for enums with a clear default variant (mark with `#[default]`).

- Option/Result combinators
  - Replace manual pattern matching for simple transformations with `.map(..)`, `.and_then(..)`, `.ok_or_else(..)`.
  - Avoid manual `split().last()` on `DoubleEndedIterator`s when the intent is the last segment of a delimited string. Use `rsplit(delim).next()`.

- Collections conveniences
  - Prefer `or_default()` over `or_insert_with(HashMap::new)` / `or_insert_with(HashSet::new)`.

- String/prefix handling
  - Avoid manual prefix stripping like `s[1..]` after `starts_with`. Use `strip_prefix()`.

- Control-flow clarity
  - Avoid obfuscated chains like `condition.then(|| val).unwrap_or(..)`. Prefer clear `if/else`.

- Large error variants
  - Do not return `Result<(), hyper::Response<...>>` with large error types directly.
  - Box large error variants: use an alias like `type RespErr = Box<Response<BoxBody<Bytes, Infallible>>>` and return `Result<(), RespErr>`.

- Type complexity
  - If function types become too verbose (e.g., nested `Arc<dyn Fn..>`), introduce type aliases to improve readability.

## HTTP/Hyper-specific Conventions

- Response body type aliases
  - Use `type RespBody = BoxBody<Bytes, Infallible>` and `type Resp = Response<RespBody>`.
- JSON helpers
  - Provide helpers like `body_from<T: Into<Bytes>>(data: T) -> RespBody`.
- Router patterns
  - Use `strip_prefix(':')` to parse path parameters.
  - Keep handler and router signatures consistent using `type` aliases (`RouteHandler`, `CommandRouteHandler`, `ErrorHandler`).

## Examples in Codebase

- `lithair-core/src/cluster/mod.rs`
  - Removed unwrap-after-is_some anti-pattern by delegating to `DeclarativeHttpHandler::handle_request()`.
- `lithair-core/src/engine/events.rs`
  - Implemented `Default` for `EventStream`.
- `lithair-core/src/engine/scc2_engine.rs`
  - Replaced `split().last()` with `rsplit(':').next()`; used `map(..)` to simplify option handling.
- `lithair-core/src/engine/lockfree_engine.rs`
  - Replaced `bool::then(..)` chains with clear `if/else`.
- `lithair-core/src/http/router.rs`
  - Added `ErrorHandler` alias; used `strip_prefix(':')` for parameters.
- `lithair-core/src/http/firewall.rs`
  - Reduced large `Err` variant size by boxing the `Response` (alias `RespErr`).

## Testing & CI

- **Development:** Use `task ci:full` for fast code quality checks (~2-3min)
- **Pre-commit:** Use `task ci:github` for complete validation (~10-15min)
- **Guarantee:** If `task ci:github` passes locally → GitHub Actions will pass
- **See:** [CI Workflow Guide](docs/CI_WORKFLOW.md) for detailed workflow

## When to allow Clippy

- If a lint conflicts with a critical performance path and the idiomatic change regresses throughput, explain and add a narrow `#[allow(...)]` with justification.
- Do not add crate-wide `allow` unless absolutely necessary.

## Commit Checklist

- `task ci:github` (includes all quality checks + functional validation)
- Tests updated/added if applicable

By following these guidelines, we keep Lithair robust, maintainable, and performant.

## 🔧 VS Code Workspace Configuration

### Fichiers de Configuration Créés

- **`.vscode/settings.json`** - Configuration optimisée pour Rust avec rust-analyzer
- **`.vscode/extensions.json`** - Extensions recommandées pour le développement Lithair
- **`.vscode/tasks.json`** - Intégration complète avec Taskfile.yml
- **`.vscode/launch.json`** - Configurations de debug pour tous les binaires
- **`Lithair.code-workspace`** - Workspace multi-dossiers pour une navigation optimale

### Commandes Rapides VS Code

```
Ctrl+Shift+P → "Tasks: Run Task" → Lithair: CI Full
Ctrl+Shift+P → "Tasks: Run Task" → Lithair: Build Release
F5 → Debug SCC2 Server Demo
Ctrl+Shift+F5 → Debug Simplified Consensus Demo
```

### Extensions Essentielles Installées Automatiquement

- **rust-lang.rust-analyzer** - Language server pour Rust
- **task.vscode-task** - Intégration Taskfile
- **vadimcn.vscode-lldb** - Debugger LLDB pour Rust
- **anthropic.claude-dev** - Intégration Claude Code

### Utilisation du Workspace

1. Ouvrir `Lithair.code-workspace` dans VS Code
2. Les extensions seront proposées automatiquement
3. Utiliser `Ctrl+Shift+P` → "Tasks: Run Task" pour accéder aux tâches Taskfile
4. Utiliser F5 pour débugger avec breakpoints

### Configuration Rust-Analyzer

- Clippy activé avec `-D warnings`
- Format automatique à la sauvegarde
- Auto-imports activés
- Support des macros activé
- Build scripts activés

### Performance

- Exclusions configurées pour `/target`, `/.raftlog`, `/logs`
- File watcher optimisé pour les gros projets Rust
- Limitation des résultats de completion pour la performance

## Author

Yoan Roblet (Arcker)
