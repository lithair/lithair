use super::{Database, Equal, Error, HttpExposable, Mutation, Page, SqlModel, Store};
use lithair_macros::DeclarativeModel;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
struct Note {
    #[http(expose)]
    #[permission(read = "Read", write = "Write")]
    id: String,
    #[http(expose, validate = "non_empty")]
    title: String,
    #[http(expose)]
    category: String,
}
impl SqlModel for Note {
    const COLLECTION: &'static str = "notes_v1";
    const FILTER_FIELDS: &'static [&'static str] = &["category"];
}
fn note(id: &str) -> Note {
    Note { id: id.into(), title: "hello".into(), category: "work".into() }
}
fn permissions() -> Vec<String> {
    vec!["Read".into(), "Write".into()]
}
async fn setup() -> (tempfile::TempDir, Database, Store<Note>) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db = Database::open(tmp.path().join("store.db").to_str().expect("path"))
        .await
        .expect("open");
    let store = db.store("tenant-a").expect("store");
    (tmp, db, store)
}

#[tokio::test]
async fn opens_a_persistent_database() {
    let (tmp, db, store) = setup().await;
    store.create(note("1"), &permissions()).await.expect("create");
    let mut updated = note("1");
    updated.title = "updated".into();
    store.update("1", updated, &permissions()).await.expect("update");
    drop(store);
    drop(db);
    let db = Database::open(tmp.path().join("store.db").to_str().expect("path"))
        .await
        .expect("reopen");
    let store = db.store::<Note>("tenant-a").expect("store");
    assert_eq!(
        store.get("1", &permissions()).await.expect("get").expect("note").title,
        "updated"
    );
    store.delete("1", &permissions()).await.expect("delete");
    drop(store);
    drop(db);
    let db = Database::open(tmp.path().join("store.db").to_str().expect("path"))
        .await
        .expect("reopen");
    assert!(db
        .store::<Note>("tenant-a")
        .expect("store")
        .get("1", &permissions())
        .await
        .expect("get")
        .is_none());
}

#[tokio::test]
async fn duplicate_mid_batch_rolls_back_and_connection_remains_usable() {
    let (_tmp, _db, store) = setup().await;
    store.create(note("existing"), &permissions()).await.expect("seed");
    let result = store
        .batch(
            vec![
                Mutation::Delete { id: "existing".into() },
                Mutation::Create(note("new")),
                Mutation::Create(note("new")),
            ],
            &permissions(),
        )
        .await;
    assert!(matches!(result, Err(Error::Conflict)));
    assert!(store.get("new", &permissions()).await.expect("get").is_none());
    assert!(store.get("existing", &permissions()).await.expect("get").is_some());
    store
        .create(note("after"), &permissions())
        .await
        .expect("subsequent transaction");
}

#[tokio::test]
async fn validation_permissions_and_primary_keys_are_enforced() {
    let (_tmp, _db, store) = setup().await;
    let mut invalid = note("bad");
    invalid.title.clear();
    assert!(matches!(store.create(invalid, &permissions()).await, Err(Error::Validation(_))));
    assert!(matches!(store.create(note("1"), &[]).await, Err(Error::Forbidden)));
    store.create(note("1"), &permissions()).await.expect("create");
    assert!(matches!(store.get("1", &[]).await, Err(Error::Forbidden)));
    assert!(store.list(Page::default(), None, &[]).await.expect("filtered").is_empty());
    assert!(matches!(store.update("1", note("1"), &[]).await, Err(Error::Forbidden)));
    assert!(matches!(store.delete("1", &[]).await, Err(Error::Forbidden)));
    assert!(matches!(
        store.update("1", note("2"), &permissions()).await,
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        store.update("missing", note("missing"), &permissions()).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(store.delete("missing", &permissions()).await, Err(Error::NotFound)));
    assert!(store.get("1", &permissions()).await.expect("get").is_some());
}

#[tokio::test]
async fn sql_filter_pagination_and_bound_values() {
    let (_tmp, db, store) = setup().await;
    for id in ["c", "a", "b"] {
        store.create(note(id), &permissions()).await.expect("create");
    }
    let mut special = note("quoted'key");
    special.category = "x' OR 1=1 --".into();
    store.create(special, &permissions()).await.expect("create");
    let page = store
        .list(
            Page { limit: 1, offset: 1 },
            Some(Equal { field: "category", value: "work" }),
            &permissions(),
        )
        .await
        .expect("page");
    assert_eq!(page[0].id, "b");
    let page = store
        .list(
            Page::default(),
            Some(Equal { field: "category", value: "x' OR 1=1 --" }),
            &permissions(),
        )
        .await
        .expect("bound filter");
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].id, "quoted'key");
    assert!(matches!(
        store.list(Page { limit: 101, offset: 0 }, None, &permissions()).await,
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        store
            .list(Page::default(), Some(Equal { field: "id) OR 1=1", value: "x" }), &permissions())
            .await,
        Err(Error::InvalidInput(_))
    ));
    let other = db.store::<Note>("tenant-b' OR 1=1 --").expect("namespace");
    other.create(note("a"), &permissions()).await.expect("isolated same key");
    assert_eq!(other.list(Page::default(), None, &permissions()).await.expect("list").len(), 1);
}

#[derive(Clone, Serialize, Deserialize)]
struct Owned {
    id: String,
    owner: String,
}

#[tokio::test]
async fn pagination_lookahead_does_not_decode_the_next_document() {
    let (_tmp, db, store) = setup().await;
    store.create(note("a"), &permissions()).await.expect("first document");
    db.store::<Note>("tenant-b")
        .unwrap()
        .create(note("b"), &permissions())
        .await
        .unwrap();
    let isolated = store
        .list_page(Page { limit: 1, offset: 0 }, None, &permissions())
        .await
        .unwrap();
    assert_eq!(isolated.data.len(), 1);
    assert_eq!(isolated.next_offset, None, "another namespace must not create continuation");
    // A malformed subsequent model must not break the current page. It becomes
    // an explicit serialization error only when that page is actually requested.
    db.connection
        .lock()
        .await
        .execute(
            "INSERT INTO lithair_documents_v1 (namespace, model, id, body) VALUES (?1, ?2, ?3, ?4)",
            turso::params![
                "tenant-a",
                Note::COLLECTION,
                "b",
                r#"{"id":"b","title":42,"category":"work"}"#
            ],
        )
        .await
        .unwrap();
    let page = store
        .list_page(Page { limit: 1, offset: 0 }, None, &permissions())
        .await
        .unwrap();
    assert_eq!(page.data.len(), 1);
    assert_eq!(page.data[0].id, "a");
    assert_eq!(page.next_offset, Some(1));
    assert!(matches!(
        store.list_page(Page { limit: 1, offset: 1 }, None, &permissions()).await,
        Err(Error::Serialization(_))
    ));
    store
        .create(note("c"), &permissions())
        .await
        .expect("connection remains usable");
}
impl HttpExposable for Owned {
    fn http_base_path() -> &'static str {
        "owned"
    }
    fn primary_key_field() -> &'static str {
        "id"
    }
    fn get_primary_key(&self) -> String {
        self.id.clone()
    }
    fn validate(&self) -> std::result::Result<(), String> {
        Ok(())
    }
    fn can_read(&self, permissions: &[String]) -> bool {
        permissions.contains(&self.owner)
    }
    fn can_write(&self, permissions: &[String]) -> bool {
        permissions.contains(&self.owner)
    }
}
impl SqlModel for Owned {
    const COLLECTION: &'static str = "owned_v1";
}

#[tokio::test]
async fn update_cannot_take_ownership_and_models_are_isolated() {
    let (_tmp, db, notes) = setup().await;
    notes.create(note("same"), &permissions()).await.expect("note");
    let store = db.store::<Owned>("tenant-a").expect("store");
    store
        .create(Owned { id: "same".into(), owner: "alice".into() }, &["alice".into()])
        .await
        .expect("create");
    assert!(matches!(
        store
            .update("same", Owned { id: "same".into(), owner: "bob".into() }, &["bob".into()])
            .await,
        Err(Error::Forbidden)
    ));
    assert!(matches!(
        store
            .update("same", Owned { id: "same".into(), owner: "bob".into() }, &["alice".into()])
            .await,
        Err(Error::Forbidden)
    ));
    assert!(store.get("same", &["alice".into()]).await.expect("read").is_some());
    assert_eq!(
        notes.get("same", &permissions()).await.expect("get").expect("note").title,
        "hello"
    );
}

#[tokio::test]
async fn concurrent_duplicate_creates_have_one_winner() {
    let (_tmp, _db, store) = setup().await;
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..16 {
        let store = store.clone();
        tasks.spawn(async move { store.create(note("same"), &permissions()).await });
    }
    let mut successes = 0;
    while let Some(result) = tasks.join_next().await {
        match result.expect("task") {
            Ok(()) => successes += 1,
            Err(Error::Conflict) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }
    assert_eq!(successes, 1);
    assert_eq!(store.list(Page::default(), None, &permissions()).await.expect("list").len(), 1);
}

#[tokio::test]
async fn dropping_an_admitted_write_does_not_leave_a_partial_transaction() {
    use std::{future::Future, task::Poll};
    let (_tmp, db, store) = setup().await;
    let lock = db.connection.lock().await;
    let perms = permissions();
    let mut operation = Box::pin(
        store.batch(vec![Mutation::Create(note("one")), Mutation::Create(note("two"))], &perms),
    );
    std::future::poll_fn(|cx| {
        assert!(operation.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(operation);
    drop(lock);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if store.get("one", &perms).await.expect("read").is_some() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("admitted write completes");
    assert!(store.get("two", &perms).await.expect("read").is_some());
    store.create(note("after"), &perms).await.expect("connection usable");
}

#[tokio::test]
async fn authorization_failure_mid_batch_rolls_back_prior_mutations() {
    let (_tmp, db, _notes) = setup().await;
    let store = db.store::<Owned>("tenant-a").expect("store");
    store
        .create(Owned { id: "protected".into(), owner: "alice".into() }, &["alice".into()])
        .await
        .expect("seed");
    let result = store
        .batch(
            vec![
                Mutation::Create(Owned { id: "new".into(), owner: "bob".into() }),
                Mutation::Delete { id: "protected".into() },
            ],
            &["bob".into()],
        )
        .await;
    assert!(matches!(result, Err(Error::Forbidden)));
    assert!(store.get("new", &["bob".into()]).await.expect("get").is_none());
    assert!(store.get("protected", &["alice".into()]).await.expect("get").is_some());
}

#[tokio::test]
async fn oversized_documents_and_batches_fail_without_writes() {
    let (_tmp, db, store) = setup().await;
    let mut oversized = note("big");
    oversized.title = "x".repeat(super::MAX_DOCUMENT_BYTES + 1);
    assert!(matches!(
        store.create(oversized, &permissions()).await,
        Err(Error::InvalidInput(_))
    ));
    let batch = (0..=super::MAX_BATCH_SIZE)
        .map(|i| Mutation::Create(note(&i.to_string())))
        .collect();
    assert!(matches!(store.batch(batch, &permissions()).await, Err(Error::InvalidInput(_))));
    assert!(store
        .list(Page::default(), None, &permissions())
        .await
        .expect("empty")
        .is_empty());
    assert!(matches!(db.store::<Note>(""), Err(Error::InvalidInput(_))));
    assert!(matches!(
        store.list(Page { limit: 0, offset: 0 }, None, &permissions()).await,
        Err(Error::InvalidInput(_))
    ));
}

#[tokio::test]
async fn concurrent_patches_merge_without_losing_independent_fields() {
    let (_tmp, _db, store) = setup().await;
    store.create(note("one"), &permissions()).await.expect("create");
    let perms = permissions();
    let (title, category) = tokio::join!(
        store.patch("one", serde_json::json!({"title": "changed"}), &perms),
        store.patch("one", serde_json::json!({"category": "other"}), &perms),
    );
    title.expect("title");
    category.expect("category");
    let updated = store.get("one", &permissions()).await.expect("get").expect("document");
    assert_eq!(updated.title, "changed");
    assert_eq!(updated.category, "other");
}
