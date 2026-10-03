//! RFC 304 on PostgreSQL: L1 copies kept coherent across nodes by pg_notify.
//! Two `Database` instances are two nodes. Copies live for an hour, so every
//! refresh below comes from an invalidation, never from expiry.
#[path = "support/env.rs"]
mod support;
use lithair_macros::DeclarativeModel;
use lithair_postgres::{Database, Decision, Error, Mutation, PostgresConfig, Store, Write};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use support::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, DeclarativeModel)]
#[storage(postgres, collection = "pg_cached")]
#[retention(memory = 100, ttl = "1h")]
struct Note {
    #[db(primary_key)]
    id: String,
    title: String,
}
fn note(id: &str, title: &str) -> Note {
    Note { id: id.into(), title: title.into() }
}

/// Poll node `store` until it shows `title` for `id`: the staleness bound.
async fn converges(store: &Store<Note>, id: &str, title: Option<&str>) -> Duration {
    let start = Instant::now();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let seen = store.get(id, &[]).await.unwrap().map(|n| n.title);
            if seen.as_deref() == title {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("{id} never showed {title:?}"));
    start.elapsed()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn writes_on_one_node_evict_copies_on_the_others() {
    let Some(env) = env() else { return };
    let ns = format!("t-{}", uuid::Uuid::new_v4());
    let (a, b) = (connect(&env).await, connect(&env).await);
    let (sa, sb) = (a.store::<Note>(&ns).unwrap(), b.store::<Note>(&ns).unwrap());
    sa.create(note("x", "one"), &[]).await.unwrap();
    assert_eq!(sb.get("x", &[]).await.unwrap().unwrap().title, "one");
    assert_eq!(sb.get("x", &[]).await.unwrap().unwrap().title, "one");
    assert_eq!(sb.cache_stats().unwrap().hits, 1, "node B serves its copy");

    // Update, patch, batch, delete and create on A all reach B's copies.
    sa.update("x", note("x", "two"), &[]).await.unwrap();
    let bound = converges(&sb, "x", Some("two")).await;
    assert!(bound < Duration::from_secs(1), "stale for {bound:?}");
    sa.patch("x", json!({"title": "three"}), &[]).await.unwrap();
    converges(&sb, "x", Some("three")).await;
    sa.batch(vec![Mutation::Delete { id: "x".into() }], &[]).await.unwrap();
    converges(&sb, "x", None).await;
    // B now caches "not found"; a create on A evicts it.
    assert!(sb.get("x", &[]).await.unwrap().is_none());
    sa.create(note("x", "four"), &[]).await.unwrap();
    converges(&sb, "x", Some("four")).await;

    // Commands publish invalidations too.
    let commands = a.commands(&ns, &["pg_cached"]).unwrap();
    commands
        .execute(|_| {
            Box::pin(async {
                Ok(Decision::Commit {
                    writes: vec![Write::model(&note("x", "by command"))?],
                    reply: Value::Null,
                })
            })
        })
        .await
        .unwrap();
    converges(&sb, "x", Some("by command")).await;

    // A rolled-back batch publishes nothing: B keeps serving its valid copy.
    sb.get("x", &[]).await.unwrap();
    let hits = sb.cache_stats().unwrap().hits;
    let failed = sa
        .batch(
            vec![
                Mutation::Patch { id: "x".into(), changes: json!({"title": "lost"}) },
                Mutation::Create(note("x", "duplicate")),
            ],
            &[],
        )
        .await;
    assert!(matches!(failed, Err(Error::Conflict)));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(sb.get("x", &[]).await.unwrap().unwrap().title, "by command");
    assert_eq!(sb.cache_stats().unwrap().hits, hits + 1, "B's copy was not evicted");
}

#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
#[storage(postgres, collection = "pg_cached_evolving")]
#[retention(memory = 100, ttl = "1h")]
struct V1 {
    #[db(primary_key)]
    id: String,
    title: String,
}
fn shout(document: &mut Value) -> lithair_postgres::Result<()> {
    document["title"] = json!(document["title"].as_str().unwrap_or_default().to_uppercase());
    Ok(())
}
#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
#[storage(postgres, collection = "pg_cached_evolving", version = 2, migrations(shout))]
#[retention(memory = 100, ttl = "1h")]
struct V2 {
    #[db(primary_key)]
    id: String,
    title: String,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_migration_on_one_node_flushes_the_partition_everywhere() {
    let Some(env) = env() else { return };
    let ns = format!("t-{}", uuid::Uuid::new_v4());
    let (a, b) = (connect(&env).await, connect(&env).await);
    let old = b.store::<V1>(&ns).unwrap();
    old.create(V1 { id: "x".into(), title: "quiet".into() }, &[]).await.unwrap();
    old.get("x", &[]).await.unwrap();
    old.get("x", &[]).await.unwrap();
    assert_eq!(old.cache_stats().unwrap().hits, 1);
    a.store::<V2>(&ns).unwrap().prepare().await.unwrap();
    // B's copy is gone: its old handle now reaches the database and is refused.
    tokio::time::timeout(Duration::from_secs(5), async {
        while old.get("x", &[]).await.is_ok() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the old copy is flushed");
    assert!(matches!(old.get("x", &[]).await, Err(Error::Schema(_))));
}

#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
#[storage(postgres, collection = "pg_no_ttl")]
#[retention(memory = 10)]
struct NoTtl {
    #[db(primary_key)]
    id: String,
}

#[tokio::test]
async fn ttl_only_is_an_explicit_choice_that_requires_a_ttl() {
    let Some(env) = env() else { return };
    let db = Database::connect(
        PostgresConfig::new(&env.url).with_ca_file(&env.ca).with_ttl_only_cache(),
    )
    .await
    .unwrap();
    let ns = format!("t-{}", uuid::Uuid::new_v4());
    assert!(matches!(db.store::<NoTtl>(&ns), Err(Error::Config(_))));
    let store = db.store::<Note>(&ns).unwrap();
    store.create(note("x", "one"), &[]).await.unwrap();
    store.get("x", &[]).await.unwrap();
    store.get("x", &[]).await.unwrap();
    assert_eq!(store.cache_stats().unwrap().hits, 1, "cached without a listener");
    // Its own writes still invalidate locally.
    store.update("x", note("x", "two"), &[]).await.unwrap();
    assert_eq!(store.get("x", &[]).await.unwrap().unwrap().title, "two");
}
