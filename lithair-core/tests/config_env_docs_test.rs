//! Meta-test: the configuration docs must match the env vars the code reads.
//!
//! Same philosophy as cucumber-tests' `no_orphan_features`: the July 2026
//! audit (#189) found 34 documented variables no code read (including
//! `LT_JWT_SECRET` and `LT_ADMIN_PASSWORD`) and 39 real variables the docs
//! never mentioned. This test fails CI when either direction drifts again:
//!
//! 1. every `LT_*`/`LITHAIR_*` variable read in `lithair-core/src` must
//!    appear in docs/configuration-reference.md (whose "Env Var" column is
//!    the authoritative name list — the matrix names fields, not env vars);
//! 2. every such variable named in either doc must be read somewhere in
//!    `lithair-core/src` (or sit on the explicit allowlist below).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Full variable names the docs mention on purpose without any code reading
/// them. Keep this list justified line by line.
const DOC_ALLOWLIST: &[&str] = &[
    // Documented as *rejected* legacy aliases in configuration-reference.md.
    "LT_COLT_ENABLED",
    "LT_COLT_ORIGINS",
    // Documented as negative examples of the naming scheme that does NOT
    // exist ("LT_SERVER_PORT is silently ignored; the real name is LT_PORT").
    "LT_SERVER_PORT",
    "LT_LOGGING_LEVEL",
];

fn workspace_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

fn is_var_char(c: char) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'
}

fn tracked(name: &str) -> bool {
    name.starts_with("LT_")
        || name.starts_with("LITHAIR_")
        || name == "EXPERIMENT_DATA_BASE"
        || name == "RUST_LOG"
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read src dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Collect the variable name following each occurrence of `pattern` in
/// `source` (the name runs until the closing quote).
fn names_after(source: &str, pattern: &str, out: &mut BTreeSet<String>) {
    for (idx, _) in source.match_indices(pattern) {
        let tail = &source[idx + pattern.len()..];
        let name: String = tail.chars().take_while(|&c| is_var_char(c)).collect();
        if !name.is_empty() && tail[name.len()..].starts_with('"') {
            out.insert(name);
        }
    }
}

/// (canonical, accepted): `canonical` must be documented; `accepted` is the
/// wider set a doc mention may legitimately reference.
fn code_vars() -> (BTreeSet<String>, BTreeSet<String>) {
    let mut sources = Vec::new();
    rust_sources(&workspace_root().join("lithair-core/src"), &mut sources);

    let mut canonical = BTreeSet::new();
    let mut accepted = BTreeSet::new();
    for path in sources {
        let source = std::fs::read_to_string(&path).expect("read source file");

        let mut literals = BTreeSet::new();
        names_after(&source, "env::var(\"", &mut literals);
        for name in literals.into_iter().filter(|n| tracked(n)) {
            canonical.insert(name.clone());
            accepted.insert(name);
        }

        // `raft_env("X")` reads LT_RAFT_X with a legacy LITHAIR_RAFT_X
        // fallback. Only the LT_ name must be documented per variable; the
        // legacy prefix is documented as a blanket alias statement.
        let mut raft = BTreeSet::new();
        names_after(&source, "raft_env(\"", &mut raft);
        for suffix in raft {
            canonical.insert(format!("LT_RAFT_{suffix}"));
            accepted.insert(format!("LT_RAFT_{suffix}"));
            accepted.insert(format!("LITHAIR_RAFT_{suffix}"));
        }
    }

    // Read via tracing_subscriber's EnvFilter::try_from_default_env(), not a
    // literal env::var — see init_default_tracing in app/mod.rs.
    canonical.insert("RUST_LOG".to_string());
    accepted.insert("RUST_LOG".to_string());

    (canonical, accepted)
}

fn doc_vars(doc: &str) -> BTreeSet<String> {
    let text = std::fs::read_to_string(workspace_root().join(doc))
        .unwrap_or_else(|e| panic!("read {doc}: {e}"));

    let mut vars = BTreeSet::new();
    let mut rest = text.as_str();
    while let Some(start) = rest.find(|c: char| is_var_char(c)) {
        let run: String = rest[start..].chars().take_while(|&c| is_var_char(c)).collect();
        // Runs ending in '_' are prefix mentions like `LT_RAFT_*`, not names.
        if tracked(&run) && !run.ends_with('_') {
            vars.insert(run.clone());
        }
        rest = &rest[start + run.len()..];
    }
    vars
}

#[test]
fn config_docs_match_code() {
    const DOCS: [&str; 2] = ["docs/configuration-reference.md", "docs/configuration-matrix.md"];

    let (canonical, accepted) = code_vars();
    assert!(canonical.len() > 40, "scanner is broken: only {} vars found", canonical.len());

    let per_doc: Vec<BTreeSet<String>> = DOCS.iter().map(|d| doc_vars(d)).collect();

    // The reference doc's "Env Var" column is the authoritative name list;
    // the matrix names config fields and only mentions env names in notes.
    let undocumented: Vec<_> = canonical.difference(&per_doc[0]).collect();
    assert!(
        undocumented.is_empty(),
        "env vars read by lithair-core but missing from {}: {undocumented:?}\n\
         Document them there, or delete the dead read.",
        DOCS[0]
    );

    let all_documented: BTreeSet<_> = per_doc.into_iter().flatten().collect();
    let fantasy: Vec<_> = all_documented
        .iter()
        .filter(|v| !accepted.contains(*v) && !DOC_ALLOWLIST.contains(&v.as_str()))
        .collect();
    assert!(
        fantasy.is_empty(),
        "env vars documented but read by no code (fantasy vars): {fantasy:?}\n\
         Remove them from the docs, or add the missing env::var read — \
         intentional negative examples belong in DOC_ALLOWLIST with a comment."
    );
}
