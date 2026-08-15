//! Locks the release profile to `panic = "unwind"` (the default).
//!
//! The server's panic-isolation guarantees — tokio task boundaries and the
//! mutation-hook `catch_unwind` (#213) — only hold when panics unwind.
//! `panic = "abort"` in the release profile silently voids them in
//! production while every (dev-profile) test keeps passing: the exact
//! green-in-CI / broken-in-release gap this meta-test exists to close
//! (found by the PR #213 verification review).
//!
//! Same spirit as the config-docs parity test (#191): assert the invariant
//! at its source instead of hoping a behavior test happens to run under the
//! right profile.

use std::path::Path;

/// Extract the body of one `[section]` from a Cargo.toml string: the lines
/// after its header up to the next `[` header.
fn section<'a>(toml: &'a str, header: &str) -> Option<&'a str> {
    let start = toml.find(header)? + header.len();
    let rest = &toml[start..];
    let end = rest.find("\n[").unwrap_or(rest.len());
    Some(&rest[..end])
}

#[test]
fn release_profile_keeps_unwinding_for_panic_isolation() {
    let workspace_manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("Cargo.toml");
    let toml = std::fs::read_to_string(&workspace_manifest)
        .unwrap_or_else(|e| panic!("read {}: {e}", workspace_manifest.display()));

    // `[profile.ci]` inherits `release`, so checking release covers both
    // unless ci overrides — check every profile section to be explicit.
    for profile in ["[profile.release]", "[profile.ci]"] {
        let Some(body) = section(&toml, profile) else { continue };
        let sets_abort = body
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with('#'))
            .any(|l| l.starts_with("panic") && l.contains("abort"));
        assert!(
            !sets_abort,
            "{profile} sets panic = \"abort\": this voids the server's panic \
             isolation (tokio task boundaries, mutation-hook catch_unwind from \
             #213) in release builds only, while the dev-profile test suite \
             stays green. Remove it; if a benchmark needs abort, put a `panic` \
             key in [profile.bench] instead."
        );
    }
}
