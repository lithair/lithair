//! Trusted sample application for SQL application commands, shared by the
//! lithair-turso tests and the Gherkin runner. No HTTP identity is involved.
use super::DeclarativeModel;
use lithair_turso::{Commands, Database, Decision, Error, SqlModel, Write};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
pub struct Item {
    pub id: String,
    #[http(validate = "non_empty")]
    pub title: String,
    pub revision: u64,
}
impl SqlModel for Item {
    const COLLECTION: &'static str = "items";
}
/// The same partition under a schema it was never prepared with.
#[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
pub struct ItemV2 {
    pub id: String,
    pub title: String,
    pub revision: u64,
}
impl SqlModel for ItemV2 {
    const COLLECTION: &'static str = "items";
    const VERSION: u32 = 2;
    const SCHEMA: &'static str = "id:string,title:string,revision:u64";
    const MIGRATIONS: &'static [lithair_turso::Migration] = &[|_| Ok(())];
}

pub const COLLECTIONS: &[&str] = &["items", "operations", "events", "outbox", "receipts", "policy"];
pub const BUSINESS: &[&str] = &["items", "operations", "events", "outbox", "receipts"];

pub async fn open(path: &std::path::Path, tenant: &str) -> (Database, Commands) {
    let db = Database::open(path.to_str().unwrap()).await.unwrap();
    db.store::<Item>(tenant).unwrap().prepare().await.unwrap();
    let commands = db.commands(tenant, COLLECTIONS).unwrap();
    (db, commands)
}

/// Policy first, then the principal-scoped receipt, then the revision check.
pub async fn submit(
    commands: &Commands,
    principal: &str,
    target: &str,
    key: &str,
    expected: u64,
    title: &str,
) -> lithair_turso::Result<Value> {
    let (principal, target, key, title) =
        (principal.to_owned(), target.to_owned(), key.to_owned(), title.to_owned());
    commands
        .execute(move |view| {
            Box::pin(async move {
                let policy = view.get("policy", &principal).await?;
                anyhow::ensure!(policy.is_none_or(|p| p["allowed"] != false), "forbidden");
                let scoped = json!([principal, "change-item", key]).to_string();
                let fingerprint = json!([target, expected, title]);
                if let Some(receipt) = view.get("receipts", &scoped).await? {
                    anyhow::ensure!(receipt["fingerprint"] == fingerprint, "idempotency conflict");
                    return Ok(Decision::Read(receipt["operation"].clone()));
                }
                let revision = view.model::<Item>(&target).await?.map_or(0, |i| i.revision);
                anyhow::ensure!(revision == expected, "revision conflict");
                let operation = json!({"id": scoped, "target": target, "revision": revision + 1});
                let item = Item { id: target.clone(), title, revision: revision + 1 };
                Ok(Decision::Commit {
                    writes: vec![
                        Write::model(&item)?,
                        Write::put("operations", &scoped, operation.clone()),
                        Write::put(
                            "events",
                            &scoped,
                            json!({"type": "ItemChanged", "target": target}),
                        ),
                        Write::put("outbox", &scoped, json!({"event": scoped})),
                        Write::put(
                            "receipts",
                            &scoped,
                            json!({"fingerprint": fingerprint, "operation": operation}),
                        ),
                    ],
                    reply: operation,
                })
            })
        })
        .await
}

pub async fn permission(commands: &Commands, principal: &str, allowed: bool) {
    let principal = principal.to_owned();
    commands
        .execute(move |_| {
            Box::pin(async move {
                Ok(Decision::Commit {
                    writes: vec![Write::put("policy", &principal, json!({"allowed": allowed}))],
                    reply: Value::Null,
                })
            })
        })
        .await
        .unwrap();
}

pub async fn counts(commands: &Commands) -> Value {
    commands
        .execute(|view| {
            Box::pin(async move {
                let mut counts = serde_json::Map::new();
                for collection in BUSINESS {
                    let (mut total, mut after) = (0, None::<String>);
                    loop {
                        let page = view.list(collection, after.as_deref(), 100).await?;
                        match page.last() {
                            Some((key, _)) => after = Some(key.clone()),
                            None => break,
                        }
                        total += page.len();
                    }
                    counts.insert(collection.to_string(), json!(total));
                }
                Ok(Decision::Read(Value::Object(counts)))
            })
        })
        .await
        .unwrap()
}
pub fn uniform(n: usize) -> Value {
    json!({"items": n, "operations": n, "events": n, "outbox": n, "receipts": n})
}

/// Atomic commits, receipt replay beyond 300 later commands, scoped keys and restart.
pub async fn durable_receipts() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("commands.db");
    let (db, commands) = open(&path, "tenant-a").await;
    let first = submit(&commands, "alice", "item", "first", 0, "one").await.unwrap();
    assert_eq!(first["revision"], 1);
    assert_eq!(counts(&commands).await, uniform(1));
    // The typed record is readable through the ordinary model store.
    let store = db.store::<Item>("tenant-a").unwrap();
    assert_eq!(store.get("item", &[]).await.unwrap().unwrap().revision, 1);

    // Receipts live in a declared collection: no cache window evicts them.
    for n in 0..300 {
        submit(&commands, "alice", &format!("item-{n}"), &format!("key-{n}"), 0, "t")
            .await
            .unwrap();
    }
    assert_eq!(submit(&commands, "alice", "item", "first", 0, "one").await.unwrap(), first);
    let conflict = submit(&commands, "alice", "other", "first", 0, "one").await;
    assert!(matches!(conflict, Err(Error::Command(e)) if e.to_string() == "idempotency conflict"));
    // Another principal using the same key gets its own receipt.
    let bob = submit(&commands, "bob", "bob-item", "first", 0, "two").await.unwrap();
    assert_ne!(bob["id"], first["id"]);
    assert_eq!(counts(&commands).await, uniform(302));

    drop((store, commands, db));
    let (_db, commands) = open(&path, "tenant-a").await;
    assert_eq!(submit(&commands, "alice", "item", "first", 0, "one").await.unwrap(), first);
    assert_eq!(counts(&commands).await, uniform(302));
}
