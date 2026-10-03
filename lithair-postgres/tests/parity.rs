//! RFC 296 phase 3: Turso and PostgreSQL honor one document-store contract.
//! The same scenario runs through `lithair_core::app::DocumentStore` (the trait
//! behind the shared HTTP handler) on both backends; every outcome is recorded
//! and the two transcripts must be identical.
#[path = "support/env.rs"]
mod support;
use lithair_core::app::DocumentStore;
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};
use serde_json::json;
use support::*;

#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
struct Note {
    #[db(primary_key)]
    #[permission(read = "Read", write = "Write")]
    id: String,
    #[http(validate = "non_empty")]
    title: String,
    category: String,
}
impl lithair_turso::SqlModel for Note {
    const COLLECTION: &'static str = "parity_notes";
    const PERMISSIONS: &'static [&'static str] = &["Read", "Write"];
    const FILTER_FIELDS: &'static [&'static str] = &["category"];
}
impl lithair_postgres::SqlModel for Note {
    const COLLECTION: &'static str = "parity_notes";
    const PERMISSIONS: &'static [&'static str] = &["Read", "Write"];
    const FILTER_FIELDS: &'static [&'static str] = &["category"];
}
/// The same model with L1 copies (RFC 304): caching must not change outcomes.
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
struct CachedNote {
    #[db(primary_key)]
    #[permission(read = "Read", write = "Write")]
    id: String,
    #[http(validate = "non_empty")]
    title: String,
    category: String,
}
const POLICY: Option<lithair_core::app::CachePolicy> = Some(lithair_core::app::CachePolicy {
    max_items: Some(4),
    max_bytes: None,
    ttl: Some(std::time::Duration::from_secs(3600)),
});
impl lithair_turso::SqlModel for CachedNote {
    const COLLECTION: &'static str = "parity_cached";
    const PERMISSIONS: &'static [&'static str] = &["Read", "Write"];
    const FILTER_FIELDS: &'static [&'static str] = &["category"];
    const CACHE: Option<lithair_core::app::CachePolicy> = POLICY;
}
impl lithair_postgres::SqlModel for CachedNote {
    const COLLECTION: &'static str = "parity_cached";
    const PERMISSIONS: &'static [&'static str] = &["Read", "Write"];
    const FILTER_FIELDS: &'static [&'static str] = &["category"];
    const CACHE: Option<lithair_core::app::CachePolicy> = POLICY;
}
/// Notes as seen through either model.
trait Shape: lithair_core::http::HttpExposable + std::fmt::Debug {
    fn new(id: &str, title: String, category: &str) -> Self;
    fn id(&self) -> &str;
    fn clear_title(&mut self);
    fn show(&self) -> String;
}
impl Shape for Note {
    fn new(id: &str, title: String, category: &str) -> Self {
        Note { id: id.into(), title, category: category.into() }
    }
    fn id(&self) -> &str {
        &self.id
    }
    fn clear_title(&mut self) {
        self.title.clear();
    }
    fn show(&self) -> String {
        format!("{}/{}/{}", self.id, self.title, self.category)
    }
}
impl Shape for CachedNote {
    fn new(id: &str, title: String, category: &str) -> Self {
        CachedNote { id: id.into(), title, category: category.into() }
    }
    fn id(&self) -> &str {
        &self.id
    }
    fn clear_title(&mut self) {
        self.title.clear();
    }
    fn show(&self) -> String {
        format!("{}/{}/{}", self.id, self.title, self.category)
    }
}

/// One line per operation: what it returned, in backend-neutral terms.
async fn transcript<M: Shape, S: DocumentStore<Model = M>>(store: S) -> Vec<String> {
    let rw = vec!["Read".to_string(), "Write".to_string()];
    let read = vec!["Read".to_string()];
    let mut log = Vec::new();
    let mut outcome = |label: &str, result: Result<String, S::Error>| {
        log.push(match result {
            Ok(value) => format!("{label}: ok {value}"),
            Err(error) => format!("{label}: {:?}", S::error_kind(&error)),
        });
    };
    for n in 0..7 {
        let category = if n % 3 == 0 { "work" } else { "home" };
        outcome(
            "create",
            store
                .create(M::new(&format!("n{n}"), format!("title n{n}"), category), &rw)
                .await
                .map(|_| String::new()),
        );
    }
    outcome(
        "duplicate",
        store
            .create(M::new("n0", "title n0".into(), "work"), &rw)
            .await
            .map(|_| String::new()),
    );
    outcome(
        "no write permission",
        store
            .create(M::new("x", "title x".into(), "work"), &read)
            .await
            .map(|_| String::new()),
    );
    let mut empty = M::new("e", "title e".into(), "work");
    empty.clear_title();
    outcome("invalid model", store.create(empty, &rw).await.map(|_| String::new()));
    let ids = |page: &lithair_core::app::ListedPage<M>| {
        let ids: Vec<_> = page.data.iter().map(|n| n.id().to_owned()).collect();
        format!("{ids:?} next={:?}", page.next_offset)
    };
    for (limit, offset) in [(3, 0), (3, 3), (3, 6), (7, 0)] {
        let page = store.list_page(limit, offset, None, &rw).await;
        outcome(&format!("page {limit}+{offset}"), page.map(|p| ids(&p)));
    }
    let work = store.list_page(10, 0, Some(("category", "work")), &rw).await;
    outcome("filter work", work.map(|p| ids(&p)));
    let unlisted = store.list_page(10, 0, Some(("title", "x")), &rw).await;
    outcome("filter undeclared", unlisted.map(|p| ids(&p)));
    // Without read permission the candidate page is filtered, not shortened.
    let hidden = store.list_page(3, 0, None, &[]).await;
    outcome("page unreadable", hidden.map(|p| ids(&p)));
    outcome("get", store.get("n1", &rw).await.map(|n| format!("{:?}", n.map(|n| n.show()))));
    outcome(
        "get missing",
        store.get("zz", &rw).await.map(|n| format!("{:?}", n.map(|n| n.show()))),
    );
    outcome(
        "get forbidden",
        store.get("n1", &[]).await.map(|n| format!("{:?}", n.map(|n| n.show()))),
    );
    outcome(
        "patch",
        store.patch("n1", json!({"title": "patched"}), &rw).await.map(|_| String::new()),
    );
    outcome(
        "patch unknown field",
        store.patch("n1", json!({"nope": 1}), &rw).await.map(|_| String::new()),
    );
    outcome(
        "patch missing",
        store.patch("zz", json!({"title": "x"}), &rw).await.map(|_| String::new()),
    );
    outcome(
        "after patch",
        store.get("n1", &rw).await.map(|n| format!("{:?}", n.map(|n| n.show()))),
    );
    outcome(
        "update",
        store
            .update("n2", M::new("n2", "title n2".into(), "work"), &rw)
            .await
            .map(|_| String::new()),
    );
    outcome(
        "update changes id",
        store
            .update("n2", M::new("n9", "title n9".into(), "work"), &rw)
            .await
            .map(|_| String::new()),
    );
    outcome("delete", store.delete("n3", &rw).await.map(|_| String::new()));
    outcome("delete again", store.delete("n3", &rw).await.map(|_| String::new()));
    outcome("delete forbidden", store.delete("n4", &read).await.map(|_| String::new()));
    let all = store.list_page(10, 0, None, &rw).await;
    outcome("final", all.map(|p| ids(&p)));
    log
}

#[tokio::test]
async fn turso_and_postgres_honor_one_document_store_contract() {
    let Some(env) = env() else { return };
    let directory = tempfile::tempdir().unwrap();
    let turso = lithair_turso::Database::open(directory.path().join("parity.db").to_str().unwrap())
        .await
        .unwrap()
        .store::<Note>("parity")
        .unwrap();
    let postgres = connect(&env)
        .await
        .store::<Note>(&format!("parity-{}", uuid::Uuid::new_v4()))
        .unwrap();
    let cached_turso =
        lithair_turso::Database::open(directory.path().join("cached.db").to_str().unwrap())
            .await
            .unwrap()
            .store::<CachedNote>("parity")
            .unwrap();
    let cached_postgres = connect(&env)
        .await
        .store::<CachedNote>(&format!("parity-{}", uuid::Uuid::new_v4()))
        .unwrap();
    let (turso, postgres) = (transcript(turso).await, transcript(postgres).await);
    // RFC 304: L1 copies change no outcome, on either backend.
    let cached_turso_log = transcript(cached_turso.clone()).await;
    let cached_postgres_log = transcript(cached_postgres.clone()).await;
    assert_eq!(cached_turso_log, turso, "Turso with cache");
    assert_eq!(cached_postgres_log, postgres, "PostgreSQL with cache");
    assert!(
        cached_turso.cache_stats().unwrap().hits > 0
            && cached_postgres.cache_stats().unwrap().hits > 0
    );
    for (t, p) in turso.iter().zip(&postgres) {
        assert_eq!(t, p, "Turso and PostgreSQL disagree");
    }
    assert_eq!(turso.len(), postgres.len());
    // And the shared contract is the documented one, not a shared bug.
    let expected = [
        "duplicate: Conflict",
        "no write permission: Forbidden",
        "invalid model: InvalidInput",
        r#"page 3+0: ok ["n0", "n1", "n2"] next=Some(3)"#,
        r#"page 3+6: ok ["n6"] next=None"#,
        r#"page 7+0: ok ["n0", "n1", "n2", "n3", "n4", "n5", "n6"] next=None"#,
        r#"filter work: ok ["n0", "n3", "n6"] next=None"#,
        "filter undeclared: InvalidInput",
        "page unreadable: ok [] next=Some(3)",
        "get missing: ok None",
        "get forbidden: Forbidden",
        "patch unknown field: InvalidInput",
        "patch missing: NotFound",
        "update changes id: InvalidInput",
        "delete again: NotFound",
        "delete forbidden: Forbidden",
        r#"final: ok ["n0", "n1", "n2", "n4", "n5", "n6"] next=None"#,
    ];
    for line in expected {
        assert!(turso.iter().any(|l| l == line), "missing `{line}` in {turso:#?}");
    }
    assert!(turso
        .iter()
        .any(|l| l.starts_with("after patch: ok Some(") && l.contains("patched")));
}
