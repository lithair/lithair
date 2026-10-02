//! lithair-postgres against a real PostgreSQL with TLS (scripts/postgres-tests.sh).
//! Every test uses its own namespace; "two processes" are two separate pools.
use lithair_core::app::{LithairServer, Method};
use lithair_macros::DeclarativeModel;
use lithair_postgres::{
    Database, Decision, Equal, Error, Migration, Mutation, Page, PostgresConfig, SqlModel, Write,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

#[path = "support/env.rs"]
mod support;
use support::*;

fn namespace() -> String {
    format!("t-{}", uuid::Uuid::new_v4())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, DeclarativeModel)]
#[storage(postgres, durable, collection = "notes", filters("category"))]
struct Note {
    #[db(primary_key)]
    #[permission(read = "Read", write = "Write")]
    id: String,
    #[http(validate = "non_empty")]
    title: String,
    category: String,
    #[serde(default)]
    left: u64,
    #[serde(default)]
    right: u64,
}
fn note(id: &str, category: &str) -> Note {
    Note { id: id.into(), title: "hello".into(), category: category.into(), left: 0, right: 0 }
}
fn rw() -> Vec<String> {
    vec!["Read".into(), "Write".into()]
}

#[tokio::test]
async fn crud_filters_pagination_permissions_and_rollback() {
    let Some(env) = env() else { return };
    let db = connect(&env).await;
    let store = db.store::<Note>(&namespace()).unwrap();
    for n in 0..7 {
        let category = if n % 2 == 0 { "work" } else { "home" };
        store.create(note(&format!("n{n}"), category), &rw()).await.unwrap();
    }
    assert!(matches!(store.create(note("n0", "work"), &rw()).await, Err(Error::Conflict)));
    assert!(matches!(store.create(note("n9", "work"), &[]).await, Err(Error::Forbidden)));
    assert!(matches!(store.get("n0", &[]).await, Err(Error::Forbidden)));
    let mut invalid = note("bad", "work");
    invalid.title.clear();
    assert!(matches!(store.create(invalid, &rw()).await, Err(Error::Validation(_))));

    let page = store.list(Page { limit: 3, offset: 0 }, None, &rw()).await.unwrap();
    assert_eq!(page.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(), ["n0", "n1", "n2"]);
    let work = Equal { field: "category", value: "work" };
    assert_eq!(store.list(Page::default(), Some(work), &rw()).await.unwrap().len(), 4);
    let unknown = Equal { field: "title", value: "hello" };
    assert!(store.list(Page::default(), Some(unknown), &rw()).await.is_err());

    store.patch("n1", json!({"title": "patched"}), &rw()).await.unwrap();
    assert!(store.patch("n1", json!({"nope": 1}), &rw()).await.is_err());
    let mut updated = note("n2", "work");
    updated.title = "updated".into();
    store.update("n2", updated, &rw()).await.unwrap();
    assert!(store.update("n2", note("other", "work"), &rw()).await.is_err());
    assert_eq!(store.get("n1", &rw()).await.unwrap().unwrap().title, "patched");

    // A failing mutation rolls back the earlier ones of its batch.
    let batch = store
        .batch(
            vec![Mutation::Delete { id: "n3".into() }, Mutation::Create(note("n0", "work"))],
            &rw(),
        )
        .await;
    assert!(matches!(batch, Err(Error::Conflict)));
    assert!(store.get("n3", &rw()).await.unwrap().is_some());
    store.delete("n3", &rw()).await.unwrap();
    assert!(matches!(store.delete("n3", &rw()).await, Err(Error::NotFound)));
    assert!(store.get("n3", &rw()).await.unwrap().is_none());

    // Namespaces are partitions.
    let other = db.store::<Note>(&namespace()).unwrap();
    assert!(other.get("n0", &rw()).await.unwrap().is_none());
}

#[tokio::test]
async fn tls_is_mandatory_and_verified() {
    let Some(env) = env() else { return };
    // An unrelated CA is refused, whatever sslmode the URL asks for.
    let foreign = PostgresConfig::new(&env.url).with_ca_file(&env.other_ca);
    assert!(Database::connect(foreign).await.is_err());
    let disable = format!("{}?sslmode=disable", env.url);
    assert!(Database::connect(PostgresConfig::new(&disable).with_ca_file(&env.other_ca))
        .await
        .is_err());
    // The test-only plain mode is limited to loopback hosts.
    let remote = env.url.replace("@localhost", "@db.example.com");
    let plain = PostgresConfig::new(remote).allow_insecure_loopback_for_tests();
    assert!(matches!(Database::connect(plain).await, Err(Error::Config(_))));
    assert!(
        Database::connect(PostgresConfig::new(&env.url).allow_insecure_loopback_for_tests())
            .await
            .is_ok()
    );
    // The URL (with its password) never appears in diagnostics.
    let config = PostgresConfig::new(&env.url).with_ca_file(&env.ca);
    assert!(!format!("{config:?}").contains("lithair:"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_processes_write_one_document_without_lost_updates() {
    let Some(env) = env() else { return };
    let ns = namespace();
    let (a, b) = (connect(&env).await, connect(&env).await);
    let (sa, sb) = (a.store::<Note>(&ns).unwrap(), b.store::<Note>(&ns).unwrap());
    sa.create(note("shared", "work"), &rw()).await.unwrap();
    // Each process patches its own field 10 times, concurrently: every patch
    // reads and rewrites the whole document, so without SERIALIZABLE (and its
    // retries) one process would overwrite the other's field.
    let bump = |store: lithair_postgres::Store<Note>, field: &'static str| async move {
        for value in 1..=10u64 {
            store.patch("shared", json!({ field: value }), &rw()).await.unwrap();
        }
    };
    tokio::join!(bump(sa.clone(), "left"), bump(sb.clone(), "right"));
    let shared = sa.get("shared", &rw()).await.unwrap().unwrap();
    assert_eq!((shared.left, shared.right), (10, 10));

    // Concurrent creates of one id from both processes: exactly one wins.
    let creates = (0..8).map(|n| {
        let store = if n % 2 == 0 { sa.clone() } else { sb.clone() };
        async move { store.create(note("race", "work"), &rw()).await }
    });
    let results = futures_join_all(creates).await;
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(results.iter().filter(|r| r.is_err()).all(|r| matches!(r, Err(Error::Conflict))));
}

async fn futures_join_all<F>(futures: impl Iterator<Item = F>) -> Vec<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    let handles: Vec<_> = futures.map(tokio::spawn).collect();
    let mut results = Vec::new();
    for handle in handles {
        results.push(handle.await.unwrap());
    }
    results
}

#[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
#[storage(postgres, collection = "evolving")]
struct V1 {
    #[db(primary_key)]
    id: String,
    title: String,
}
fn exclaim(document: &mut Value) -> lithair_postgres::Result<()> {
    document["title"] = json!(format!("{}!", document["title"].as_str().unwrap_or_default()));
    Ok(())
}
#[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
#[storage(postgres, collection = "evolving", version = 2, migrations(exclaim))]
struct V2 {
    #[db(primary_key)]
    id: String,
    title: String,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_nodes_migrate_a_partition_once() {
    let Some(env) = env() else { return };
    let ns = namespace();
    let a = connect(&env).await;
    let v1 = a.store::<V1>(&ns).unwrap();
    for id in ["one", "two"] {
        v1.create(V1 { id: id.into(), title: "kept".into() }, &[]).await.unwrap();
    }
    // Four "nodes" start the upgraded model together.
    let mut nodes = Vec::new();
    for _ in 0..4 {
        let db = connect(&env).await;
        let ns = ns.clone();
        nodes.push(tokio::spawn(async move { db.store::<V2>(&ns).unwrap().prepare().await }));
    }
    for node in nodes {
        node.await.unwrap().unwrap();
    }
    let v2 = a.store::<V2>(&ns).unwrap();
    assert_eq!(v2.get("one", &[]).await.unwrap().unwrap().title, "kept!");
    // Old handles and downgrades are refused, data untouched.
    assert!(matches!(v1.get("one", &[]).await, Err(Error::Schema(_))));
    assert!(a.store::<V1>(&ns).unwrap().prepare().await.is_err());
    assert_eq!(v2.get("two", &[]).await.unwrap().unwrap().title, "kept!");
    let _: &[Migration] = <V2 as SqlModel>::MIGRATIONS;
}

const COLLECTIONS: &[&str] = &["notes", "receipts", "outbox", "policy"];

/// Policy first, then the principal-scoped receipt, then the revision check.
async fn submit(
    commands: &lithair_postgres::Commands,
    principal: &str,
    key: &str,
    expected: u64,
) -> lithair_postgres::Result<Value> {
    let (principal, key) = (principal.to_owned(), key.to_owned());
    commands
        .execute(move |view| {
            let (principal, key) = (principal.clone(), key.clone());
            Box::pin(async move {
                let policy = view.get("policy", &principal).await?;
                anyhow::ensure!(policy.is_none_or(|p| p["allowed"] != false), "forbidden");
                let scoped = json!([principal, key]).to_string();
                if let Some(receipt) = view.get("receipts", &scoped).await? {
                    anyhow::ensure!(receipt["expected"] == expected, "idempotency conflict");
                    return Ok(Decision::Read(receipt["reply"].clone()));
                }
                let current = view.model::<Note>("item").await?;
                let revision = current.as_ref().map_or(0, |n| n.left);
                anyhow::ensure!(revision == expected, "revision conflict");
                let reply = json!({"key": scoped, "revision": revision + 1});
                let mut item = current.unwrap_or_else(|| note("item", "work"));
                item.left = revision + 1;
                Ok(Decision::Commit {
                    writes: vec![
                        Write::model(&item)?,
                        Write::put("outbox", &scoped, json!({"revision": revision + 1})),
                        Write::put(
                            "receipts",
                            &scoped,
                            json!({"expected": expected, "reply": reply}),
                        ),
                    ],
                    reply,
                })
            })
        })
        .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn commands_are_atomic_and_serialized_across_processes() {
    let Some(env) = env() else { return };
    let ns = namespace();
    let (a, b) = (connect(&env).await, connect(&env).await);
    a.store::<Note>(&ns).unwrap().prepare().await.unwrap();
    let (ca, cb) = (a.commands(&ns, COLLECTIONS).unwrap(), b.commands(&ns, COLLECTIONS).unwrap());
    let first = submit(&ca, "alice", "k1", 0).await.unwrap();
    assert_eq!(submit(&cb, "alice", "k1", 0).await.unwrap(), first, "receipt replay");
    assert!(matches!(submit(&cb, "alice", "k1", 5).await, Err(Error::Command(_))));

    // Eight different keys race on revision 1 from two processes: one wins.
    let racing = (0..8).map(|n| {
        let commands = if n % 2 == 0 { ca.clone() } else { cb.clone() };
        async move { submit(&commands, "alice", &format!("race-{n}"), 1).await }
    });
    let results = futures_join_all(racing).await;
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1, "{results:?}");
    for result in results.iter().filter(|r| r.is_err()) {
        assert!(matches!(result, Err(Error::Command(e)) if e.to_string() == "revision conflict"));
    }

    // Revocation is checked before the receipt.
    let revoke = |allowed: bool| {
        ca.execute(move |_| {
            Box::pin(async move {
                Ok(Decision::Commit {
                    writes: vec![Write::put("policy", "alice", json!({"allowed": allowed}))],
                    reply: Value::Null,
                })
            })
        })
    };
    revoke(false).await.unwrap();
    assert!(
        matches!(submit(&ca, "alice", "k1", 0).await, Err(Error::Command(e)) if e.to_string() == "forbidden")
    );
    revoke(true).await.unwrap();

    // A rejected or invalid decision writes nothing; raw puts into a typed
    // partition are refused.
    let rejected = ca
        .execute(|_| {
            Box::pin(async move {
                Ok(Decision::Commit {
                    writes: vec![
                        Write::put("outbox", "partial", json!({})),
                        Write::put("notes", "item", json!({"raw": true})),
                    ],
                    reply: Value::Null,
                })
            })
        })
        .await;
    assert!(rejected.is_err());
    let partial = ca
        .execute(|view| {
            Box::pin(async move { Ok(Decision::Read(json!(view.get("outbox", "partial").await?))) })
        })
        .await
        .unwrap();
    assert_eq!(partial, Value::Null);
    assert!(ca.execute(|_| panic!("decision panic")).await.is_err());
}

#[tokio::test]
async fn a_newer_lithair_format_is_refused() {
    let Some(env) = env() else { return };
    let config = || PostgresConfig::new(&env.isolated).with_ca_file(&env.ca);
    Database::connect(config()).await.unwrap();
    let (client, connection) = tokio_postgres_connect(&env.isolated).await;
    let task = tokio::spawn(connection);
    client.execute("UPDATE lithair.format SET version = 99", &[]).await.unwrap();
    assert!(matches!(Database::connect(config()).await, Err(Error::Schema(_))));
    client.execute("UPDATE lithair.format SET version = 1", &[]).await.unwrap();
    drop(client);
    let _ = task.await;
}

#[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
#[storage(postgres, collection = "archives")]
struct Archive {
    #[db(primary_key)]
    id: String,
    title: String,
}

#[tokio::test]
async fn declared_models_serve_generated_routes_from_the_installed_database() {
    let Some(env) = env() else { return };
    connect(&env).await.install().unwrap();
    let dir = tempfile_dir();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = LithairServer::new()
        .with_admin_panel(false)
        .with_model::<Archive>(&format!("{dir}/archives"), "/api/archives")
        .build()
        .unwrap();
    let (stop, done) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(server.serve_with_listener(listener, async {
        let _ = done.await;
    }));
    let client = reqwest::Client::new();
    let id = uuid::Uuid::new_v4().to_string();
    let created = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(resp) = client
                .post(format!("{url}/api/archives"))
                .json(&json!({"id": id, "title": "kept"}))
                .send()
                .await
            {
                break resp;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(created.status(), 201);
    let got: Value = client
        .get(format!("{url}/api/archives/{id}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(got["title"], "kept");
    let deleted = client.request(Method::DELETE, format!("{url}/api/archives/{id}")).send().await;
    assert_eq!(deleted.unwrap().status(), 204);
    let _ = stop.send(());
    task.await.unwrap().unwrap();
}
fn tempfile_dir() -> String {
    let dir = std::env::temp_dir().join(format!("lithair-pg-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.to_string_lossy().into_owned()
}
