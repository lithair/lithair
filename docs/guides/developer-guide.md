# Lithair Framework - Developer Guide

This guide is for developers who want to contribute to the Lithair framework itself.

## Setup Development Environment

### Prerequisites
- Rust 1.70+ with rustfmt and clippy
- Git
- A good understanding of Rust systems programming

### Clone and Setup
```bash
git clone <lithair-repo>
cd Lithair
cargo check  # Verify everything compiles
```

### Development Dependencies
The framework itself has minimal dependencies:
- `lithair-core`: Zero external dependencies
- `lithair-macros`: Only proc-macro2, quote, syn for macro development

## Project Structure

```
Lithair/
├── lithair-core/           # Main framework implementation
│   ├── src/
│   │   ├── lib.rs           # Public API
│   │   ├── http/            # Custom HTTP server
│   │   ├── engine/          # Event sourcing + state management
│   │   ├── serialization/   # JSON + binary serialization
│   │   └── macros/          # Helper code for macros
├── lithair-macros/        # Procedural macros
├── docs/                    # Documentation
└── .vscode/                 # Development configuration
```

## Development Workflow

### 1. Building
```bash
# Build the entire workspace
cargo build

# Build specific component
cargo build -p lithair-core
cargo build -p lithair-macros
```

### 2. Testing
```bash
# Run all tests
cargo test

# Run tests for specific component
cargo test -p lithair-core
cargo test -p lithair-macros

# Run with output
cargo test -- --nocapture
```

### 3. Linting and Formatting
```bash
# Format code
cargo fmt

# Check lints
cargo clippy

# Check without dependencies
cargo check
```

## Implementation Priorities

### Phase 1: Core Infrastructure (Current)
- [ ] HTTP server implementation (`lithair-core/src/http/`)
- [ ] Basic event sourcing (`lithair-core/src/engine/`)
- [ ] JSON serialization (`lithair-core/src/serialization/`)
- [ ] Basic macros (`lithair-macros/src/`)

### Phase 2: Framework API
- [ ] Complete `#[RaftstoneModel]` macro
- [ ] Complete `#[RaftstoneApi]` macro
- [ ] Route generation and handling
- [ ] Error handling system

### Phase 3: Advanced Features
- [ ] Binary serialization for performance
- [ ] Event log compaction
- [ ] Admin dashboard
- [ ] WebSocket support

## Implementation Guidelines

### Zero Dependencies Policy
- **Core principle**: `lithair-core` must have zero external dependencies
- **Exception**: `lithair-macros` can use standard proc-macro dependencies
- **Rationale**: Complete control over behavior, performance, and security

### Performance First
- Prefer zero-copy operations where possible
- Use `std::mem::ManuallyDrop` for performance-critical paths
- Profile regularly with `cargo bench`
- Minimize allocations in hot paths

### API Design Principles
1. **Simple by default**: Common cases should be one-liners
2. **Powerful when needed**: Advanced use cases should be possible
3. **Type-safe**: Leverage Rust's type system to prevent errors
4. **Zero-runtime-cost**: Abstractions should compile away

### Error Handling
- Use `Result<T, Error>` consistently
- Provide detailed error messages
- Include context in error chains
- Never panic in library code (except for programming errors)

## Module Implementation Guide

### HTTP Module (`lithair-core/src/http/`)

**Key files:**
- `server.rs`: TCP listener and connection handling
- `request.rs`: HTTP request parsing
- `response.rs`: HTTP response building
- `router.rs`: URL routing and method dispatch

**Implementation notes:**
- Use `std::net::TcpListener` for networking
- Implement HTTP/1.1 parser from scratch
- Support keep-alive connections
- Handle common HTTP methods (GET, POST, PUT, DELETE)

**Testing approach:**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_get_request() {
        let raw = b"GET /users HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let request = HttpRequest::parse(raw).unwrap();
        assert_eq!(request.method, HttpMethod::GET);
        assert_eq!(request.path, "/users");
    }
}
```

### Engine Module (`lithair-core/src/engine/`)

**Key files:**
- `state.rs`: StateEngine with RwLock
- `events.rs`: Event trait and EventStore
- `persistence.rs`: File-based persistence

**Implementation notes:**
- Use `parking_lot::RwLock` for better performance than std
- Implement append-only log with fsync for durability
- Support event replay for state reconstruction
- Handle concurrent reads with shared locks

### Serialization Module (`lithair-core/src/serialization/`)

**Key files:**
- `json.rs`: JSON parser and serializer
- `binary.rs`: Binary format for persistence

**Implementation notes:**
- JSON parser should handle all valid JSON
- Focus on correctness first, then performance
- Binary format should be self-describing with version info
- Support for common Rust types (String, Vec, HashMap, etc.)

### Macros Module (`lithair-macros/`)

**Implementation notes:**
- Use `syn` for parsing Rust syntax
- Use `quote` for code generation
- Generate readable code for debugging
- Include span information for good error messages

**Example macro structure:**
```rust
#[proc_macro_derive(RaftstoneModel)]
pub fn lithair_model(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    
    // Extract struct information
    let name = &input.ident;
    let fields = match &input.data {
        Data::Struct(DataStruct { fields, .. }) => fields,
        _ => panic!("RaftstoneModel only works on structs"),
    };
    
    // Generate event types and implementations
    let expanded = quote! {
        // Generated code here
    };
    
    TokenStream::from(expanded)
}
```

## Testing Strategy

### Unit Tests
- Each module should have comprehensive unit tests
- Test both success and error cases
- Use property-based testing where appropriate

### Integration Tests
- Test the full framework API with real examples
- Verify that generated code compiles and works
- Test serialization round-trips

### Performance Tests
- Benchmark critical paths (serialization, state access, HTTP parsing)
- Set performance regression CI checks
- Profile memory usage patterns

### Example Test Structure
```rust
// tests/integration_test.rs
use lithair_core::{Lithair, RaftstoneApplication};

#[derive(Default)]
struct TestApp {
    counter: u64,
}

impl RaftstoneApplication for TestApp {
    type State = Self;
    fn initial_state() -> Self::State { Self::default() }
    fn routes() -> Vec<Route<Self::State>> { vec![] }
}

#[test]
fn test_basic_framework_creation() {
    let app = TestApp::default();
    let framework = Lithair::new(app);
    // Test framework initialization
}
```

## Documentation Standards

### Code Documentation
- All public APIs must have doc comments
- Include examples in doc comments where helpful
- Document panics, errors, and safety requirements

### Architectural Documentation
- Keep `docs/ARCHITECTURE.md` updated with changes
- Document design decisions and trade-offs
- Include performance characteristics

### Example Documentation
```rust
/// Parses an HTTP request from raw bytes.
/// 
/// # Examples
/// 
/// ```
/// let request = HttpRequest::parse(b"GET / HTTP/1.1\r\n\r\n")?;
/// assert_eq!(request.method, HttpMethod::GET);
/// ```
/// 
/// # Errors
/// 
/// Returns `HttpError::InvalidRequest` if the request is malformed.
pub fn parse(raw: &[u8]) -> Result<HttpRequest, HttpError> {
    // Implementation
}
```

## Performance Guidelines

### Profiling
```bash
# Install profiling tools
cargo install cargo-flamegraph

# Profile a benchmark
cargo flamegraph --bench http_parser

# Profile tests
cargo test --release -- --test-threads=1
```

### Memory Management
- Prefer stack allocation over heap when possible
- Reuse buffers in hot loops
- Use `Cow<str>` for strings that might be borrowed
- Profile allocations with tools like `heaptrack`

### Optimization Checklist
- [ ] Zero-copy parsing where possible
- [ ] Batch operations to reduce syscalls
- [ ] Use efficient data structures (`FxHashMap` vs `HashMap`)
- [ ] Minimize string allocations
- [ ] Profile and optimize hot paths

## Git Workflow (Trunk-Based Development)

This project uses **trunk-based development**: `main` is the protected trunk, and all
changes go through short-lived feature branches merged via Pull Requests.

### Branch Naming Conventions

| Prefix | Usage | Example |
|--------|-------|---------|
| `feat/` | New features | `feat/native-tls` |
| `fix/` | Bug fixes | `fix/session-expiry` |
| `chore/` | Maintenance, deps, CI | `chore/bump-tokio` |
| `docs/` | Documentation only | `docs/trunk-based-workflow` |
| `refactor/` | Code restructuring | `refactor/extract-pem-helpers` |

### Development Flow

```bash
# 1. Start from an up-to-date main
git checkout main && git pull origin main

# 2. Create a feature branch
git checkout -b feat/my-feature

# 3. Work in small, focused commits
#    Always validate before pushing:
task ci:full                          # fmt + clippy -D warnings + tests
git add <specific-files>
git commit -m "feat: concise description"

# 4. Push and open a Pull Request
git push -u origin feat/my-feature
gh pr create --title "feat: concise title" --body "$(cat <<'EOF'
## Summary
- What this PR does and why

## Test plan
- [ ] `task ci:full` passes
- [ ] Manual verification of ...
EOF
)"

# 5. After CI passes and review is approved, merge (squash)
gh pr merge --squash --delete-branch

# 6. Back to main for the next task
git checkout main && git pull origin main
```

### Rules

| Rule | Rationale |
|------|-----------|
| Never push directly to `main` | Protected branch -- all changes via PR |
| One concern per PR | Easier review, safer rollback |
| CI must pass before merge | `task ci:full` at minimum |
| Short-lived branches | Merge within hours/days, not weeks |
| Squash merge | Keeps `main` history clean and linear |
| Delete branch after merge | No stale branches |

### Commit Message Format

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>: <concise description>

<optional body explaining the "why", not the "what">

Co-Authored-By: ...   # if pair-programming or AI-assisted
```

Common types: `feat`, `fix`, `chore`, `docs`, `refactor`, `test`, `perf`.

### Pre-Push Checklist

1. `cargo fmt -- --check` (or `task fmt`)
2. `cargo clippy --workspace --all-targets -- -D warnings` (or `task lint`)
3. `cargo test -p lithair-core -p lithair-macros` (or `task test`)
4. All three combined: **`task ci:full`**
5. Before requesting review: **`task ci:github`** (includes smoke tests)

### Code Review Checklist

- [ ] Code follows existing Rust idioms and project conventions
- [ ] All tests pass (`task ci:full`)
- [ ] New behavior has test coverage
- [ ] Documentation is updated if public API changed
- [ ] No security regressions (OWASP top 10)
- [ ] Performance implications considered for hot paths

## Release Process

### Version Numbering
- Follow semantic versioning (semver)
- Pre-1.0: Breaking changes increment minor version
- Post-1.0: Breaking changes increment major version

### Release Checklist
- [ ] All tests pass
- [ ] Documentation is updated
- [ ] Changelog is updated
- [ ] Version numbers are bumped
- [ ] Create git tag
- [ ] Publish to crates.io

This framework is designed to be a long-term, stable foundation for building high-performance distributed applications. Every design decision should consider the impact on developer experience, performance, and maintainability.