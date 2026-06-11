//! Regression tests for issue #122 — `#[db(fk = "table")]` was silently
//! dropped by the macro parser, so `ModelSpec::get_policy(...)` returned
//! `fk_collection: None` and the engine-level `AutoJoiner` could not expand
//! the relation. Only the alternative `#[relation(foreign_key = "table")]`
//! syntax worked.
//!
//! Root cause: `parse_db_attributes` in
//! `lithair-macros/src/declarative_simple.rs` walked the attribute tokens
//! `TokenTree` by `TokenTree`, so the pair-shaped `fk = "table"` arrived as
//! three separate trees and `extract_string_value("fk")` found no quote.
//! Same bug class as issue #75 (`#[http(validate = "...")]`, covered by
//! `validate_attribute_test.rs`) and the retention/pinned helper-attribute
//! miss (covered by `retention_macro_test.rs`). The fix converts the parser
//! to string-level comma splitting, mirroring `parse_http_attributes`.
//!
//! These tests also pin the NON-regression surface of the converted parser:
//! the single-token flags (`primary_key`, `unique`, `indexed`, `nullable`)
//! and the separately-parsed `default = X` values must keep producing the
//! same spec, because `#[lithair_model]`'s `serde(default)` generation and
//! the additive-upgrade safety mechanism depend on them
//! (see docs/operations/upgrade.md).

use lithair_core::model::ModelSpec;
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};

/// Mirrors the `BddOrder` reproducer from issue #122 (cucumber-tests
/// world.rs): an FK declared with the `#[db(fk = "...")]` syntax, plus the
/// full set of previously-working `#[db(...)]` forms on sibling fields so a
/// parser regression on ANY of them surfaces here.
#[derive(Debug, Clone, Default, Serialize, Deserialize, DeclarativeModel)]
struct FkOrder {
    #[db(primary_key, indexed)]
    id: String,

    /// The issue #122 reproducer: pair-shaped `fk = "table"`.
    #[db(fk = "some_table")]
    user_id: String,

    /// `fk` combined with a flag in the SAME attribute list — exercises the
    /// comma-split walking more than one fragment.
    #[db(indexed, fk = "bdd_products")]
    product_id: String,

    #[db(unique, nullable)]
    email: String,

    /// `default = <integer>` — additive-upgrade safety mechanism.
    #[db(default = 0)]
    quantity: u32,

    /// `default = <bool>`.
    #[db(default = false)]
    archived: bool,

    /// `default = "<fn name>"` (string form).
    #[db(default = "compute_source")]
    source: String,
}

fn probe() -> FkOrder {
    FkOrder::default()
}

/// Issue #122: the FK target declared via `#[db(fk = "...")]` must reach
/// the derive-generated `ModelSpec` policy — this is exactly what the
/// engine-level `AutoJoiner` reads to expand the relation.
#[test]
fn db_fk_pair_attribute_reaches_model_spec_policy() {
    let policy = probe().get_policy("user_id").expect("user_id must have a field policy");
    assert!(policy.fk, "user_id must be marked as a foreign key — issue #122");
    assert_eq!(
        policy.fk_collection,
        Some("some_table".into()),
        "fk_collection must carry the #[db(fk = \"some_table\")] target — issue #122"
    );
}

/// `fk` must also parse when it shares the attribute list with a flag.
#[test]
fn db_fk_combined_with_flag_in_same_attribute() {
    let policy = probe().get_policy("product_id").expect("product_id must have a field policy");
    assert!(policy.fk, "product_id must be marked as a foreign key");
    assert_eq!(policy.fk_collection, Some("bdd_products".into()));
    assert!(policy.indexed, "the `indexed` flag sharing the list with `fk` must still parse");
}

/// Fields without an `fk` annotation must NOT be reported as foreign keys.
#[test]
fn non_fk_field_has_no_fk_policy() {
    let policy = probe().get_policy("email").expect("email must have a field policy");
    assert!(!policy.fk, "email carries no fk annotation");
    assert_eq!(policy.fk_collection, None);
}

/// Non-regression: the single-token flags must keep parsing exactly as
/// before the comma-split conversion.
#[test]
fn db_flags_still_parse_after_comma_split_conversion() {
    let spec = FkOrder::get_declarative_spec();

    let id = spec.fields.get("id").expect("id field present");
    assert!(id.primary_key, "primary_key flag must parse");
    assert!(id.indexed, "indexed flag must parse alongside primary_key");

    let email = spec.fields.get("email").expect("email field present");
    assert!(email.unique, "unique flag must parse");
    assert!(email.nullable, "nullable flag must parse alongside unique");

    // get_policy must agree with the spec for the flag-driven bits.
    let email_policy = probe().get_policy("email").expect("email policy");
    assert!(email_policy.unique);
}

/// Non-regression: `default = X` (parsed by a separate string-level search
/// in `parse_db_attributes`) must keep producing the same `default_value`
/// for every supported form — `#[lithair_model]`'s `serde(default)`
/// generation and the additive-upgrade mechanism depend on it.
#[test]
fn db_default_values_still_parse_after_comma_split_conversion() {
    let spec = FkOrder::get_declarative_spec();

    let quantity = spec.fields.get("quantity").expect("quantity field present");
    assert_eq!(
        quantity.default_value.as_deref(),
        Some("0"),
        "default = 0 must produce default_value Some(\"0\")"
    );

    let archived = spec.fields.get("archived").expect("archived field present");
    assert_eq!(
        archived.default_value.as_deref(),
        Some("false"),
        "default = false must produce default_value Some(\"false\")"
    );

    let source = spec.fields.get("source").expect("source field present");
    assert_eq!(
        source.default_value.as_deref(),
        Some("compute_source"),
        "default = \"compute_source\" must produce the unquoted fn name"
    );
}

/// Non-regression: the FK syntax fixed by #122 and the `default` mechanism
/// must coexist — the spec carries `foreign_key` alongside everything else.
#[test]
fn declarative_spec_carries_foreign_key_target() {
    let spec = FkOrder::get_declarative_spec();
    let user_id = spec.fields.get("user_id").expect("user_id field present");
    assert_eq!(
        user_id.foreign_key.as_deref(),
        Some("some_table"),
        "spec.foreign_key must carry the #[db(fk = ...)] target"
    );
}
