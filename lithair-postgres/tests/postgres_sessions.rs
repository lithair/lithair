//! RFC 308 store B: sessions shared through PostgreSQL. Two `Database`
//! instances are two nodes.
#[path = "support/env.rs"]
mod support;
use chrono::{Duration, Utc};
use lithair_core::session::{Session, SessionStore};
use lithair_postgres::PostgresSessionStore;
use support::*;

fn session(id: &str, expires_in: Duration) -> Session {
    let mut session = Session::new(id.into(), Utc::now() + expires_in);
    session.set("role", "Admin").unwrap();
    session.set("user_id", "alice").unwrap();
    session
}
fn id() -> String {
    format!("s-{}", uuid::Uuid::new_v4())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sessions_are_shared_and_revoked_across_nodes() {
    let Some(env) = env() else { return };
    let (a, b) = (connect(&env).await, connect(&env).await);
    // Two nodes creating the table at once is fine.
    let (sa, sb) = tokio::join!(PostgresSessionStore::new(&a), PostgresSessionStore::new(&b));
    let (sa, sb) = (sa.unwrap(), sb.unwrap());
    assert!(sa.shared_authority());

    let token = id();
    sa.set(session(&token, Duration::hours(1))).await.unwrap();
    let seen = sb.get(&token).await.unwrap().expect("node B sees node A's login");
    assert_eq!(seen.get::<String>("role").as_deref(), Some("Admin"));
    assert_eq!(seen.get::<String>("user_id").as_deref(), Some("alice"));
    assert!((seen.expires_at - (Utc::now() + Duration::hours(1))).num_seconds().abs() < 5);

    // Logout on B: A refuses the token immediately.
    sb.delete(&token).await.unwrap();
    assert!(sa.get(&token).await.unwrap().is_none());

    // A role change (rewrite) on A is what B reads next.
    let token = id();
    sa.set(session(&token, Duration::hours(1))).await.unwrap();
    let mut demoted = session(&token, Duration::hours(1));
    demoted.set("role", "Reader").unwrap();
    sa.set(demoted).await.unwrap();
    let seen = sb.get(&token).await.unwrap().unwrap();
    assert_eq!(seen.get::<String>("role").as_deref(), Some("Reader"));
}

#[tokio::test]
async fn expiry_is_judged_by_the_database_clock_and_cleaned_up() {
    let Some(env) = env() else { return };
    let db = connect(&env).await;
    let store = PostgresSessionStore::new(&db).await.unwrap();
    let (expired, live) = (id(), id());
    store.set(session(&expired, Duration::seconds(-1))).await.unwrap();
    store.set(session(&live, Duration::hours(1))).await.unwrap();
    assert!(store.get(&expired).await.unwrap().is_none(), "past expires_at");
    assert!(store.get(&live).await.unwrap().is_some());

    // The comparison uses the server clock: a session expiring one minute in
    // the database's future is live even though `now()` is evaluated there.
    let (client, connection) = tokio_postgres_connect(&env.url).await;
    let task = tokio::spawn(connection);
    let skewed = id();
    store.set(session(&skewed, Duration::hours(1))).await.unwrap();
    client
        .execute(
            "UPDATE lithair.sessions SET expires_at = now() + interval '1 minute' WHERE id = $1",
            &[&skewed],
        )
        .await
        .unwrap();
    assert!(store.get(&skewed).await.unwrap().is_some());
    client
        .execute(
            "UPDATE lithair.sessions SET expires_at = now() - interval '1 second' WHERE id = $1",
            &[&skewed],
        )
        .await
        .unwrap();
    assert!(store.get(&skewed).await.unwrap().is_none());
    drop(client);
    let _ = task.await;

    // Cleanup removes expired rows only; count ignores them.
    assert!(store.cleanup_expired().await.unwrap() >= 2);
    assert!(store.get(&live).await.unwrap().is_some());
    assert!(store.count().await.unwrap() >= 1);
}
