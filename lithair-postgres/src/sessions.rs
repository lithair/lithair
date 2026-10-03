//! Sessions shared by every node through PostgreSQL (RFC 308, store B).
//!
//! A login, logout or revocation committed by one node is what every node
//! reads next: reads are `SERIALIZABLE`, so any node can authorize. Expiry is
//! compared with **the database clock** (`expires_at > now()`), so clock skew
//! between Lithair nodes does not matter. Sessions are never rewritten per
//! request (absolute expiry).
use crate::{Database, Error, Result};
use lithair_core::session::{Session, SessionStore};
use serde_json::Value as Json;

/// A [`SessionStore`] in the `lithair` schema of a [`Database`]. Accepted
/// next to a native cluster (`shared_authority`).
#[derive(Clone)]
pub struct PostgresSessionStore {
    database: Database,
}

impl PostgresSessionStore {
    /// Create the sessions table if needed (under an advisory lock, so nodes
    /// starting together create it once).
    pub async fn new(database: &Database) -> Result<Self> {
        database
            .transaction(|tx, _| {
                Box::pin(async move {
                    tx.execute("SELECT pg_advisory_xact_lock($1)", &[&SESSIONS_LOCK]).await?;
                    tx.batch_execute(
                        r#"CREATE TABLE IF NOT EXISTS lithair.sessions (
                               id text COLLATE "C" PRIMARY KEY,
                               data jsonb NOT NULL,
                               created_at timestamptz NOT NULL,
                               expires_at timestamptz NOT NULL,
                               last_accessed_at timestamptz NOT NULL);
                           CREATE INDEX IF NOT EXISTS sessions_expiry
                               ON lithair.sessions (expires_at);"#,
                    )
                    .await?;
                    Ok(())
                })
            })
            .await?;
        Ok(Self { database: database.clone() })
    }
}

/// `pg_advisory_xact_lock` key for creating the sessions table.
const SESSIONS_LOCK: i64 = 0x4c49_5448_5345_5353; // "LITHSESS"

fn to_anyhow(error: Error) -> anyhow::Error {
    anyhow::anyhow!("PostgreSQL session store: {error}")
}

#[async_trait::async_trait]
impl SessionStore for PostgresSessionStore {
    async fn get(&self, id: &str) -> anyhow::Result<Option<Session>> {
        self.database
            .transaction(|tx, _| {
                Box::pin(async move {
                    let row = tx
                        .query_opt(
                            "SELECT data, created_at, expires_at, last_accessed_at
                             FROM lithair.sessions WHERE id = $1 AND expires_at > now()",
                            &[&id],
                        )
                        .await?;
                    let Some(row) = row else { return Ok(None) };
                    let data = match row.get::<_, Json>(0) {
                        Json::Object(map) => map.into_iter().collect(),
                        _ => return Err(Error::Schema("session data must be an object".into())),
                    };
                    Ok(Some(Session {
                        id: id.to_owned(),
                        data,
                        created_at: row.get(1),
                        expires_at: row.get(2),
                        last_accessed_at: row.get(3),
                    }))
                })
            })
            .await
            .map_err(to_anyhow)
    }

    async fn set(&self, session: Session) -> anyhow::Result<()> {
        let data = Json::Object(session.data.clone().into_iter().collect());
        let session = &session;
        let data = &data;
        self.database
            .transaction(|tx, _| {
                Box::pin(async move {
                    tx.execute(
                        "INSERT INTO lithair.sessions
                             (id, data, created_at, expires_at, last_accessed_at)
                         VALUES ($1, $2, $3, $4, $5)
                         ON CONFLICT (id) DO UPDATE SET data = excluded.data,
                             expires_at = excluded.expires_at,
                             last_accessed_at = excluded.last_accessed_at",
                        &[
                            &session.id,
                            data,
                            &session.created_at,
                            &session.expires_at,
                            &session.last_accessed_at,
                        ],
                    )
                    .await?;
                    Ok(())
                })
            })
            .await
            .map_err(to_anyhow)
    }

    async fn delete(&self, id: &str) -> anyhow::Result<()> {
        self.database
            .transaction(|tx, _| {
                Box::pin(async move {
                    tx.execute("DELETE FROM lithair.sessions WHERE id = $1", &[&id]).await?;
                    Ok(())
                })
            })
            .await
            .map_err(to_anyhow)
    }

    async fn cleanup_expired(&self) -> anyhow::Result<usize> {
        self.database
            .transaction(|tx, _| {
                Box::pin(async move {
                    Ok(tx
                        .execute("DELETE FROM lithair.sessions WHERE expires_at <= now()", &[])
                        .await? as usize)
                })
            })
            .await
            .map_err(to_anyhow)
    }

    async fn count(&self) -> anyhow::Result<usize> {
        self.database
            .transaction(|tx, _| {
                Box::pin(async move {
                    let row = tx
                        .query_one(
                            "SELECT count(*) FROM lithair.sessions WHERE expires_at > now()",
                            &[],
                        )
                        .await?;
                    Ok(row.get::<_, i64>(0).max(0) as usize)
                })
            })
            .await
            .map_err(to_anyhow)
    }

    fn shared_authority(&self) -> bool {
        true
    }
}
