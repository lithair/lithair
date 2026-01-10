# Lithair Rust Guidelines (Clippy + Idiomatic Best Practices)

This document defines the Rust quality standards for Lithair. All contributors (humans and agents) must follow these rules. The goal is to keep code safe, idiomatic, and performant, with zero warnings on `cargo check` and clean, actionable `cargo clippy` output.

## Policy
- Always build with stable Rust and keep code compatible with our `rust-toolchain.toml`.
- Run `cargo check` frequently and keep it warning-free.
- Run `cargo clippy` regularly. Fix actionable lints; justify or suppress only when necessary.
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
- Run `cargo check -q` and `cargo clippy -q` locally before committing.
- Prefer unit tests close to the code under `#[cfg(test)]`.
- Keep examples compiling (`cargo run --example ...`) when applicable.

## When to allow Clippy
- If a lint conflicts with a critical performance path and the idiomatic change regresses throughput, explain and add a narrow `#[allow(...)]` with justification.
- Do not add crate-wide `allow` unless absolutely necessary.

## Commit Checklist
- cargo fmt --all
- cargo check -q
- cargo clippy -q
- Tests updated/added if applicable

By following these guidelines, we keep Lithair robust, maintainable, and performant.
