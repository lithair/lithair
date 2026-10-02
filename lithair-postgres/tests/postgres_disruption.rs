//! Fault injection: killed connections and a server restart. A separate test
//! binary, so no other test runs while connections and the server go away.
#[path = "support/env.rs"]
mod support;
use lithair_postgres::{Commands, Database, Decision, PostgresConfig, Write};
use serde_json::{json, Value};
use std::time::Duration;
use support::*;

async fn read(commands: &Commands) -> lithair_postgres::Result<Value> {
    commands
        .execute(|view| {
            Box::pin(async move { Ok(Decision::Read(json!(view.get("kv", "kept").await?))) })
        })
        .await
}

#[tokio::test]
async fn pools_recover_from_killed_connections_and_a_server_restart() {
    let Some(env) = env() else { return };
    // The isolated database: only connections to it are killed.
    let db = Database::connect(PostgresConfig::new(&env.isolated).with_ca_file(&env.ca))
        .await
        .unwrap();
    let commands = db.commands(&format!("t-{}", uuid::Uuid::new_v4()), &["kv"]).unwrap();
    commands
        .execute(|_| {
            Box::pin(async {
                Ok(Decision::Commit {
                    writes: vec![Write::put("kv", "kept", json!({"value": 1}))],
                    reply: Value::Null,
                })
            })
        })
        .await
        .unwrap();

    // Every pooled connection dies, as in a database failover.
    let (admin, connection) = tokio_postgres_connect(&env.isolated).await;
    let task = tokio::spawn(connection);
    let killed: i64 = admin
        .query_one(
            "SELECT count(pg_terminate_backend(pid)) FROM pg_stat_activity
             WHERE datname = current_database() AND pid <> pg_backend_pid()",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert!(killed > 0, "the pool held at least one connection");
    drop(admin);
    let _ = task.await;
    // The pool replaces dead connections; the committed record is intact.
    assert_eq!(read(&commands).await.unwrap(), json!({"value": 1}));

    let Some(restart) = &env.restart else { return };
    let status = std::process::Command::new("sh").arg("-c").arg(restart).status().unwrap();
    assert!(status.success());
    let recovered = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if let Ok(value) = read(&commands).await {
                break value;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .expect("the pool reconnects after a server restart");
    assert_eq!(recovered, json!({"value": 1}));
}
