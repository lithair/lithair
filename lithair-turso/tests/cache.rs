//! RFC 304: L1 copies of Turso models declared with `#[retention]`.
use lithair_macros::DeclarativeModel;
use lithair_turso::{Database, Decision, Error, Migration, Mutation, Write};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, DeclarativeModel)]
#[storage(turso, collection = "cached")]
#[retention(memory = 3, ttl = "1s")]
struct Note {
    #[db(primary_key)]
    #[permission(read = "Read", write = "Write")]
    id: String,
    title: String,
}
fn note(id: &str, title: &str) -> Note {
    Note { id: id.into(), title: title.into() }
}
fn rw() -> Vec<String> {
    vec!["Read".into(), "Write".into()]
}
async fn database() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path().join("cache.db").to_str().unwrap()).await.unwrap();
    (dir, db)
}

#[tokio::test]
async fn copies_are_served_bounded_and_never_stale_after_local_writes() {
    let (_dir, db) = database().await;
    let store = db.store::<Note>("tenant").unwrap();
    store.create(note("a", "one"), &rw()).await.unwrap();
    assert_eq!(store.get("a", &rw()).await.unwrap().unwrap().title, "one");
    assert_eq!(store.get("a", &rw()).await.unwrap().unwrap().title, "one");
    let stats = store.cache_stats().unwrap();
    assert_eq!((stats.hits, stats.misses, stats.items), (1, 1, 1));

    // Every handle on the partition shares the copies.
    let other = db.store::<Note>("tenant").unwrap();
    other.get("a", &rw()).await.unwrap();
    assert_eq!(store.cache_stats().unwrap().hits, 2);

    // Permission hooks run on cached reads too.
    assert!(matches!(store.get("a", &[]).await, Err(Error::Forbidden)));

    // Read-your-writes: update, patch, batch and delete invalidate.
    store.update("a", note("a", "two"), &rw()).await.unwrap();
    assert_eq!(store.get("a", &rw()).await.unwrap().unwrap().title, "two");
    store.patch("a", json!({"title": "three"}), &rw()).await.unwrap();
    assert_eq!(other.get("a", &rw()).await.unwrap().unwrap().title, "three");
    store.create(note("b", "b"), &rw()).await.unwrap();
    store.get("b", &rw()).await.unwrap();
    store
        .batch(
            vec![
                Mutation::Patch { id: "a".into(), changes: json!({"title": "four"}) },
                Mutation::Delete { id: "b".into() },
            ],
            &rw(),
        )
        .await
        .unwrap();
    assert_eq!(store.get("a", &rw()).await.unwrap().unwrap().title, "four");
    assert!(store.get("b", &rw()).await.unwrap().is_none());

    // "Not found" is cached, and a create invalidates it.
    assert!(store.get("c", &rw()).await.unwrap().is_none());
    let before = store.cache_stats().unwrap().hits;
    assert!(store.get("c", &rw()).await.unwrap().is_none());
    assert_eq!(store.cache_stats().unwrap().hits, before + 1, "negative hit");
    store.create(note("c", "c"), &rw()).await.unwrap();
    assert_eq!(store.get("c", &rw()).await.unwrap().unwrap().title, "c");

    // A failed batch changes nothing and leaves no stale copy.
    let failed = store
        .batch(
            vec![
                Mutation::Patch { id: "a".into(), changes: json!({"title": "lost"}) },
                Mutation::Create(note("c", "duplicate")),
            ],
            &rw(),
        )
        .await;
    assert!(matches!(failed, Err(Error::Conflict)));
    assert_eq!(store.get("a", &rw()).await.unwrap().unwrap().title, "four");

    // get_fresh reads the database and does not count as a hit.
    let hits = store.cache_stats().unwrap().hits;
    store.get_fresh("a", &rw()).await.unwrap();
    assert_eq!(store.cache_stats().unwrap().hits, hits);
}

#[tokio::test]
async fn the_lru_bound_and_the_ttl_are_respected() {
    let (_dir, db) = database().await;
    let store = db.store::<Note>("tenant").unwrap();
    for id in ["a", "b", "c", "d"] {
        store.create(note(id, id), &rw()).await.unwrap();
        store.get(id, &rw()).await.unwrap();
    }
    let stats = store.cache_stats().unwrap();
    assert_eq!(stats.items, 3, "memory = 3");
    assert!(stats.evictions >= 1);
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let misses = store.cache_stats().unwrap().misses;
    store.get("d", &rw()).await.unwrap();
    assert_eq!(store.cache_stats().unwrap().misses, misses + 1, "expired by the ttl");
}

#[tokio::test]
async fn command_writes_and_migrations_invalidate_copies() {
    let (_dir, db) = database().await;
    let store = db.store::<Note>("tenant").unwrap();
    store.prepare().await.unwrap();
    store.create(note("a", "one"), &rw()).await.unwrap();
    store.get("a", &rw()).await.unwrap();
    let commands = db.commands("tenant", &["cached"]).unwrap();
    commands
        .execute(|_| {
            Box::pin(async {
                Ok(Decision::Commit {
                    writes: vec![Write::model(&note("a", "by command"))?],
                    reply: Value::Null,
                })
            })
        })
        .await
        .unwrap();
    assert_eq!(store.get("a", &rw()).await.unwrap().unwrap().title, "by command");

    // Upgrading the model migrates the stored documents: no copy survives.
    store.get("a", &rw()).await.unwrap();
    let v2 = db.store::<NoteV2>("tenant").unwrap();
    v2.prepare().await.unwrap();
    assert_eq!(v2.get("a", &rw()).await.unwrap().unwrap().title, "by command!");
    assert!(matches!(store.get("a", &rw()).await, Err(Error::Schema(_))));
}

fn exclaim(document: &mut Value) -> lithair_turso::Result<()> {
    document["title"] = json!(format!("{}!", document["title"].as_str().unwrap_or_default()));
    Ok(())
}
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
#[storage(turso, collection = "cached", version = 2, migrations(exclaim))]
#[retention(memory = 3, ttl = "1s")]
struct NoteV2 {
    #[db(primary_key)]
    #[permission(read = "Read", write = "Write")]
    id: String,
    title: String,
}
const _: &[Migration] = <NoteV2 as lithair_turso::SqlModel>::MIGRATIONS;
