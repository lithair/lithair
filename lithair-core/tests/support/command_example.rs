//! Trusted test application. No HTTP authentication is provided by this fixture.
use lithair_core::cluster::native::commands::{CommandStore, Decision, Write};
use serde_json::{json, Value};
pub const CONTRACT: &str = "example-tasks-v1";
pub const COLLECTIONS: &[&str] = &["tasks", "operations", "events", "outbox", "receipts", "policy"];

pub async fn submit(
    store: &CommandStore,
    tenant: &str,
    principal: &str,
    target: &str,
    key: &str,
    expected: u64,
) -> anyhow::Result<Value> {
    let (tenant, principal, target, key) =
        (tenant.to_owned(), principal.to_owned(), target.to_owned(), key.to_owned());
    let operation = uuid::Uuid::new_v4().to_string();
    store.execute(move |view| {
        let policy = json!([tenant, principal]).to_string();
        anyhow::ensure!(view.get("policy", &policy) != Some(&json!(false)), "forbidden");
        let scoped_key = json!([tenant, principal, "change-task", key]).to_string();
        let fingerprint = json!([target, expected]);
        if let Some(receipt) = view.get("receipts", &scoped_key) {
            anyhow::ensure!(receipt["fingerprint"] == fingerprint, "idempotency conflict");
            return Ok(Decision::Read(receipt["operation"].clone()));
        }
        let task_key = json!([tenant, target]).to_string();
        let revision = view.get("tasks", &task_key).and_then(|t| t["revision"].as_u64()).unwrap_or(0);
        anyhow::ensure!(revision == expected, "revision conflict");
        let result = json!({"id":operation,"tenant":tenant,"principal":principal,"target":target,"revision":revision+1});
        Ok(Decision::Commit {
            writes: vec![
                Write::put("tasks", task_key, json!({"tenant":tenant,"id":target,"revision":revision+1})),
                Write::put("operations", &operation, result.clone()),
                Write::put("events", &operation, json!({"tenant":tenant,"operation":operation,"type":"TaskChanged"})),
                Write::put("outbox", &operation, json!({"tenant":tenant,"event":operation})),
                Write::put("receipts", scoped_key, json!({"fingerprint":fingerprint,"operation":result})),
            ], reply: result,
        })
    }).await
}
pub async fn permission(
    store: &CommandStore,
    tenant: &str,
    principal: &str,
    allowed: bool,
) -> anyhow::Result<Value> {
    let key = json!([tenant, principal]).to_string();
    store
        .execute(move |_| {
            Ok(Decision::Commit {
                writes: vec![Write::put("policy", key, json!(allowed))],
                reply: Value::Null,
            })
        })
        .await
}
pub async fn counts(store: &CommandStore) -> anyhow::Result<Value> {
    store
        .read(|view| {
            let mut result = serde_json::Map::new();
            for collection in COLLECTIONS.iter().filter(|c| **c != "policy") {
                result.insert(
                    collection.to_string(),
                    json!(view.collection(collection).unwrap().len()),
                );
            }
            Ok(Value::Object(result))
        })
        .await
}
