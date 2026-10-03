//! Sessions replicated in the native consensus group (RFC 308, store A).
//!
//! Sessions live in an internal collection of the cluster's replicated,
//! checkpointed state. A login, logout or revocation is a consensus write;
//! a lookup takes the read barrier, so only the ready leader authorizes and a
//! minority never trusts a revoked session. Anywhere else the store answers
//! [`SessionStoreUnavailable`], which HTTP layers turn into 503. Expiry is
//! absolute, written by the leader at login and checked with its clock.
use super::commands::{CommandStore, Decision, Write};
use super::model::SESSIONS;
use crate::session::{Session, SessionStore, SessionStoreUnavailable};
use serde_json::Value;

/// Writes per consensus command when cleaning up expired sessions.
const CLEANUP_BATCH: usize = 100;

/// A [`SessionStore`] backed by a native cluster declaring
/// [`super::Model::sessions`]. Obtain it with
/// [`super::NativeCluster::session_store`].
pub struct NativeSessionStore {
    commands: CommandStore,
}

/// Consensus refusals (follower, no quorum, deadline) mean "ask the leader".
fn unavailable(error: anyhow::Error) -> anyhow::Error {
    anyhow::Error::new(SessionStoreUnavailable).context(error.to_string())
}

impl NativeSessionStore {
    pub(super) fn new(commands: CommandStore) -> Self {
        Self { commands }
    }

    async fn write(&self, write: Write) -> anyhow::Result<()> {
        self.commands
            .execute(move |_| Ok(Decision::Commit { writes: vec![write], reply: Value::Null }))
            .await
            .map(|_| ())
            .map_err(unavailable)
    }

    /// Ids of every stored session, live or not, with their expiry.
    async fn all(&self) -> anyhow::Result<Vec<(String, Option<Session>)>> {
        self.commands
            .read(|view| {
                Ok(view
                    .collection(SESSIONS)
                    .map(|sessions| {
                        sessions
                            .iter()
                            .map(|(id, value)| {
                                (id.clone(), serde_json::from_value::<Session>(value.clone()).ok())
                            })
                            .collect()
                    })
                    .unwrap_or_default())
            })
            .await
            .map_err(unavailable)
    }
}

#[async_trait::async_trait]
impl SessionStore for NativeSessionStore {
    async fn get(&self, id: &str) -> anyhow::Result<Option<Session>> {
        let id = id.to_owned();
        let stored = self
            .commands
            .read(move |view| Ok(view.get(SESSIONS, &id).cloned()))
            .await
            .map_err(unavailable)?;
        Ok(stored
            .and_then(|value| serde_json::from_value::<Session>(value).ok())
            .filter(|session| !session.is_expired()))
    }

    async fn set(&self, session: Session) -> anyhow::Result<()> {
        let value = serde_json::to_value(&session)?;
        self.write(Write::put(SESSIONS, session.id, value)).await
    }

    async fn delete(&self, id: &str) -> anyhow::Result<()> {
        self.write(Write::delete(SESSIONS, id)).await
    }

    /// Only the ready leader cleans up; elsewhere this is a no-op, so every
    /// node can run the periodic `SessionManager` cleanup.
    async fn cleanup_expired(&self) -> anyhow::Result<usize> {
        if !self.commands.ready().await {
            return Ok(0);
        }
        let expired: Vec<String> = self
            .all()
            .await?
            .into_iter()
            .filter(|(_, session)| session.as_ref().is_none_or(|s| s.is_expired()))
            .map(|(id, _)| id)
            .collect();
        for chunk in expired.chunks(CLEANUP_BATCH) {
            let writes = chunk.iter().map(|id| Write::delete(SESSIONS, id)).collect();
            self.commands
                .execute(move |_| Ok(Decision::Commit { writes, reply: Value::Null }))
                .await
                .map_err(unavailable)?;
        }
        Ok(expired.len())
    }

    async fn count(&self) -> anyhow::Result<usize> {
        Ok(self
            .all()
            .await?
            .into_iter()
            .filter(|(_, session)| session.as_ref().is_some_and(|s| !s.is_expired()))
            .count())
    }

    fn shared_authority(&self) -> bool {
        true
    }
}
