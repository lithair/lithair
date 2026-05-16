//! Regression tests for issue #75 — `#[http(validate = "...")]` rules
//! were silently dropped by the macro parser, so `HttpExposable::validate()`
//! was a no-op on POST/PUT/PATCH paths. Empty `name` therefore persisted
//! with `HTTP 201` instead of the documented `HTTP 400 'name' must not be
//! empty` (see `examples/03-rest-api/src/main.rs` line 49-52).
//!
//! These tests exercise the generated `HttpExposable::validate()` impl
//! directly — the same function the POST handler calls at
//! `lithair-core/src/http/declarative.rs:925`. If `validate()` correctly
//! rejects empty input here, the POST endpoint will return 400 in the
//! handler. The full HTTP round-trip is already covered by the existing
//! cucumber suite; this test isolates the validation-rule wiring so a
//! regression on the macro parser surfaces fast.

use lithair_core::http::HttpExposable;
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};

/// Mirrors the model in `examples/03-rest-api/src/main.rs` (the example whose
/// doc-comment explicitly advertises the rejected-empty-title behavior).
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
struct Todo {
    #[http(expose)]
    #[lifecycle(immutable)]
    #[db(unique)]
    id: String,

    #[http(expose, validate = "non_empty", validate = "max_length(200)")]
    #[lifecycle(audited)]
    title: String,

    #[http(expose)]
    done: bool,
}

/// Also mirrors the reproducer from issue #75 verbatim, with multiple
/// fields all carrying `validate = "non_empty"` so we catch any pattern
/// regression that only affects the first-seen rule on a struct.
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
struct Account {
    #[http(expose)]
    #[lifecycle(immutable)]
    #[db(unique)]
    id: String,

    #[http(expose, validate = "non_empty", validate = "max_length(200)")]
    #[lifecycle(audited)]
    name: String,

    #[http(expose, validate = "non_empty")]
    #[db(unique)]
    #[lifecycle(audited)]
    email: String,

    #[http(expose, validate = "non_empty")]
    provider: String,

    #[http(expose)]
    enabled: bool,
}

#[test]
fn empty_title_rejected_per_03_rest_api_example() {
    // This is the exact behavior the `examples/03-rest-api` doc-comment
    // promises (line 48-52). Pre-fix this assertion failed because the
    // macro parser dropped the `non_empty` rule and `validate()` returned
    // `Ok(())` for an empty title.
    let todo = Todo { id: "1".into(), title: String::new(), done: false };
    let err = todo.validate().expect_err(
        "Todo with empty title must be rejected per #[http(validate = \"non_empty\")] — issue #75",
    );
    assert!(
        err.contains("'title'") && err.contains("must not be empty"),
        "expected canonical 'title' empty-validation message, got: {err}"
    );
}

#[test]
fn non_empty_title_passes_validation() {
    let todo = Todo { id: "1".into(), title: "Learn Lithair".into(), done: false };
    todo.validate().expect("valid Todo must pass validation");
}

#[test]
fn title_over_max_length_rejected() {
    // Second rule on the same field — guards against a parser regression
    // that only loads the first rule encountered.
    let todo = Todo { id: "1".into(), title: "x".repeat(201), done: false };
    let err = todo
        .validate()
        .expect_err("Todo with title > 200 chars must be rejected per max_length(200)");
    assert!(
        err.contains("'title'") && err.contains("at most 200"),
        "expected canonical max_length message, got: {err}"
    );
}

#[test]
fn account_empty_name_rejected_issue_75_reproducer() {
    // Reproducer from issue #75: empty `name` on POST must reject.
    let account = Account {
        id: "acc-bad".into(),
        name: String::new(),
        email: "x@example.com".into(),
        provider: "gmail".into(),
        enabled: true,
    };
    let err = account
        .validate()
        .expect_err("empty `name` must reject per issue #75 reproducer");
    assert!(
        err.contains("'name'") && err.contains("must not be empty"),
        "expected canonical 'name' empty-validation message, got: {err}"
    );
}

#[test]
fn account_empty_email_rejected() {
    // Validates that #[http(validate = "non_empty")] still applies even
    // when other attributes (#[db(unique)], #[lifecycle(audited)]) sit
    // between/around it on the same field.
    let account = Account {
        id: "acc-bad".into(),
        name: "Alice".into(),
        email: String::new(),
        provider: "gmail".into(),
        enabled: true,
    };
    let err = account.validate().expect_err("empty `email` must reject");
    assert!(
        err.contains("'email'") && err.contains("must not be empty"),
        "expected canonical 'email' empty-validation message, got: {err}"
    );
}

#[test]
fn account_all_required_fields_present_passes() {
    let account = Account {
        id: "acc-1".into(),
        name: "Alice".into(),
        email: "alice@example.com".into(),
        provider: "gmail".into(),
        enabled: true,
    };
    account.validate().expect("valid Account must pass validation");
}

/// Belt-and-suspenders coverage of the spec metadata: the parsed
/// `validation` list on each field must contain the literal rule
/// strings, since downstream tooling (OpenAPI generation, schema
/// introspection) reads them from `get_declarative_spec()`.
#[test]
fn declarative_spec_carries_validation_rules() {
    let spec = Account::get_declarative_spec();
    let name = spec.fields.get("name").expect("name field present");
    assert!(
        name.validation.iter().any(|r| r == "non_empty"),
        "name.validation must include `non_empty`; got {:?}",
        name.validation
    );
    assert!(
        name.validation.iter().any(|r| r == "max_length(200)"),
        "name.validation must include `max_length(200)`; got {:?}",
        name.validation
    );

    let email = spec.fields.get("email").expect("email field present");
    assert!(
        email.validation.iter().any(|r| r == "non_empty"),
        "email.validation must include `non_empty`; got {:?}",
        email.validation
    );
}
