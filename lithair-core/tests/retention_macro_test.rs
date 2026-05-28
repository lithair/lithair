//! Regression tests for the macro-generated `RetentionAware` impl.
//!
//! The `DeclarativeModel` derive macro generates an `impl RetentionAware
//! for T` block driven by `#[retention(memory = N)]` (struct-level) and
//! `#[pinned]` (field-level) annotations. If the macro stops emitting
//! these — or emits them with wrong values — every downstream behavior
//! (eviction, warm map, listing) silently degrades to "everything in
//! memory". This test exercises the generated impl directly so a
//! regression on the macro side surfaces fast.

use lithair_core::lifecycle::{LifecycleAware, RetentionAware};
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, DeclarativeModel)]
#[retention(memory = 100)]
struct Email {
    #[db(primary_key)]
    #[pinned]
    id: String,

    #[pinned]
    from: String,

    #[pinned]
    subject: String,

    // Not pinned — evicted with the rest of the item.
    body: String,
}

/// No annotations → no retention, no pinned fields. Backward-compat check.
#[derive(Debug, Clone, Default, Serialize, Deserialize, DeclarativeModel)]
struct Bare {
    #[db(primary_key)]
    id: String,
    value: String,
}

#[test]
fn retention_config_memory_count_from_annotation() {
    let cfg = Email::retention_config();
    assert_eq!(
        cfg.memory_count,
        Some(100),
        "memory_count must reflect #[retention(memory = 100)]"
    );
}

#[test]
fn pinned_fields_listed_from_annotations() {
    let pinned: Vec<&'static str> = Email::pinned_fields();
    // Order is not guaranteed by the macro (depends on HashMap iteration);
    // assert set membership instead.
    assert_eq!(pinned.len(), 3, "expected 3 pinned fields (id, from, subject)");
    assert!(pinned.contains(&"id"));
    assert!(pinned.contains(&"from"));
    assert!(pinned.contains(&"subject"));
    assert!(!pinned.contains(&"body"), "body must NOT be pinned");
}

#[test]
fn has_retention_limit_when_memory_count_set() {
    assert!(Email::has_retention_limit());
}

#[test]
fn bare_model_has_no_retention() {
    let cfg = Bare::retention_config();
    assert_eq!(cfg.memory_count, None, "unannotated model must have None memory_count");
    assert!(!Bare::has_retention_limit());
    assert!(Bare::pinned_fields().is_empty(), "unannotated model must have no pinned fields");
}

#[test]
fn lifecycle_aware_reports_pinned_via_field_policy() {
    let e = Email::default();
    // is_field_pinned() is the user-facing helper on LifecycleAware that
    // reads the generated FieldPolicy.pinned bit.
    assert!(e.is_field_pinned("from"), "from is annotated #[pinned]");
    assert!(e.is_field_pinned("subject"));
    assert!(!e.is_field_pinned("body"), "body is NOT annotated #[pinned]");
    assert!(!e.is_field_pinned("nonexistent_field"));
}
