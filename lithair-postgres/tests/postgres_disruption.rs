//! Fault injection: killed connections, killed cache listeners and a server
//! restart. A separate test binary, so no other test runs while connections
//! and the server go away.
#[path = "support/env.rs"]
mod support;
use lithair_macros::DeclarativeModel;
use lithair_postgres::{Commands, Database, Decision, PostgresConfig, Store, Write};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use support::*;

/// These tests restart the server and kill connections: one at a time.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
    let _serial = SERIAL.lock().await;
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, DeclarativeModel)]
#[storage(postgres, collection = "pg_cached_disruption")]
#[retention(memory = 100, ttl = "1h")]
struct Note {
    #[db(primary_key)]
    id: String,
    title: String,
}
fn note(id: &str, title: &str) -> Note {
    Note { id: id.into(), title: title.into() }
}
async fn converges(store: &Store<Note>, id: &str, title: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while store.get(id, &[]).await.unwrap().map(|n| n.title).as_deref() != Some(title) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("{id} never showed {title}"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_lost_listener_disables_copies_until_it_listens_again() {
    let Some(env) = env() else { return };
    let _serial = SERIAL.lock().await;
    let ns = format!("t-{}", uuid::Uuid::new_v4());
    let (a, b) = (connect(&env).await, connect(&env).await);
    let (sa, sb) = (a.store::<Note>(&ns).unwrap(), b.store::<Note>(&ns).unwrap());
    sa.create(note("x", "one"), &[]).await.unwrap();
    sb.get("x", &[]).await.unwrap();

    // Kill every cache listener, as a network cut or a failover would.
    let (admin, connection) = tokio_postgres_connect(&env.url).await;
    let task = tokio::spawn(connection);
    let killed: i64 = admin
        .query_one(
            "SELECT count(pg_terminate_backend(pid)) FROM pg_stat_activity
             WHERE application_name = $1",
            &[&lithair_postgres::LISTENER_NAME],
        )
        .await
        .unwrap()
        .get(0);
    assert!(killed >= 2, "one listener per node");
    // Once B notices, it serves the database: a write missed while
    // disconnected is visible anyway.
    tokio::time::sleep(Duration::from_millis(50)).await;
    sa.update("x", note("x", "while deaf"), &[]).await.unwrap();
    assert_eq!(sb.get("x", &[]).await.unwrap().unwrap().title, "while deaf");

    // The listener comes back and copies are served again.
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let before = sb.cache_stats().unwrap().hits;
            sb.get("x", &[]).await.unwrap();
            if sb.cache_stats().unwrap().hits > before {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the listener reconnects");
    let listeners: i64 = admin
        .query_one(
            "SELECT count(*) FROM pg_stat_activity WHERE application_name = $1",
            &[&lithair_postgres::LISTENER_NAME],
        )
        .await
        .unwrap()
        .get(0);
    assert!(listeners >= 1);
    sa.update("x", note("x", "heard"), &[]).await.unwrap();
    converges(&sb, "x", "heard").await;
    drop(admin);
    let _ = task.await;
}
