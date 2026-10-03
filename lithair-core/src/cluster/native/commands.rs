//! Application-owned atomic commands on the native consensus runtime.
//!
//! This is a trusted Rust API, with no generated HTTP routes or authentication.
//! Authenticate callers before admission and check current authorization in the
//! decision callback **before** looking up a receipt. Keep business receipts,
//! events and outbox records in declared collections; they are never implicitly
//! evicted. Callbacks must be deterministic and must not perform external effects.
//! See `docs/features/state-engine/application-commands.md` for limits/recovery.
use super::{state, Change, Command, Config, NativeCluster, Reply, State, DEADLINE};
use anyhow::{ensure, Context};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use tokio::net::TcpListener;

/// Canonical data changes; the entire batch is committed at one ordered revision.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Write {
    Put { collection: String, key: String, value: Value },
    Delete { collection: String, key: String },
}
impl Write {
    pub fn put(collection: impl Into<String>, key: impl Into<String>, value: Value) -> Self {
        Self::Put { collection: collection.into(), key: key.into(), value }
    }
    pub fn delete(collection: impl Into<String>, key: impl Into<String>) -> Self {
        Self::Delete { collection: collection.into(), key: key.into() }
    }
    pub(super) fn collection(&self) -> &str {
        match self {
            Self::Put { collection, .. } | Self::Delete { collection, .. } => collection,
        }
    }
    fn key(&self) -> &str {
        match self {
            Self::Put { key, .. } | Self::Delete { key, .. } => key,
        }
    }
}

/// A borrowed, consistent view for trusted application code. Tenant filtering
/// and policy evaluation belong to the application, including on read/replay.
pub struct View<'a> {
    state: &'a State,
}
impl View<'_> {
    /// Group revision, not an aggregate's business version.
    pub fn revision(&self) -> u64 {
        self.state.revision
    }
    pub fn get(&self, collection: &str, key: &str) -> Option<&Value> {
        self.state.models.get(collection)?.get(key)
    }
    pub fn collection(&self, collection: &str) -> Option<&BTreeMap<String, Value>> {
        self.state.models.get(collection)
    }
}

/// `Read` returns a decision without writing (for example, an authorized receipt
/// replay). `Commit` atomically writes all records, then returns the reply.
pub enum Decision {
    Read(Value),
    Commit { writes: Vec<Write>, reply: Value },
}

/// An opt-in three-voter application store, separate from generated model CRUD.
/// Provision with the existing operator tools; opening never bootstraps. All
/// nodes must agree on the explicit application version and collection names.
#[derive(Clone)]
pub struct CommandStore {
    runtime: NativeCluster,
    namespace: String,
}
impl CommandStore {
    pub async fn open(
        config: impl AsRef<Path>,
        application: &str,
        collections: &[&str],
    ) -> anyhow::Result<Self> {
        Self::open_inner(config.as_ref(), application, collections, None).await
    }
    pub async fn open_with_listener(
        config: impl AsRef<Path>,
        application: &str,
        collections: &[&str],
        listener: TcpListener,
    ) -> anyhow::Result<Self> {
        Self::open_inner(config.as_ref(), application, collections, Some(listener)).await
    }
    async fn open_inner(
        config: &Path,
        application: &str,
        collections: &[&str],
        listener: Option<TcpListener>,
    ) -> anyhow::Result<Self> {
        ensure!(
            !application.is_empty() && application.len() <= 128,
            "an explicit application contract version is required"
        );
        let ids: BTreeSet<String> = collections.iter().map(|id| (*id).to_owned()).collect();
        ensure!(
            !ids.is_empty() && ids.len() == collections.len(),
            "declare distinct nonempty collections"
        );
        ensure!(
            ids.iter().all(|id| !id.is_empty()
                && id.len() <= 128
                && id.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))),
            "invalid collection name"
        );
        let namespace = ids.first().context("missing collection")?.clone();
        // Distinct from the declarative model contract, even for identical names.
        let contract: [u8; 32] = Sha256::digest(serde_json::to_vec(
            &json!({"application_commands":1,"application":application,"collections":ids}),
        )?)
        .into();
        let path = config.to_owned();
        let prepared =
            tokio::task::spawn_blocking(move || crate::cluster::operator::prepare::<Config>(&path))
                .await??;
        let listener = match listener {
            Some(listener) => {
                ensure!(
                    listener.local_addr()?.port() == prepared.transport.local_address().port(),
                    "peer listener port differs from configured dial port"
                );
                listener
            }
            None => TcpListener::bind(prepared.transport.local_address()).await?,
        };
        let empty = State::collections(contract, ids);
        let runtime =
            NativeCluster::start_runtime(prepared, listener, contract, BTreeMap::new(), empty)
                .await?;
        Ok(Self { runtime, namespace })
    }
    /// Commands over an internal collection of an existing runtime (the
    /// replicated sessions of a model cluster).
    pub(super) fn over(runtime: NativeCluster, namespace: &str) -> Self {
        Self { runtime, namespace: namespace.to_owned() }
    }
    pub async fn bootstrap(&self) -> anyhow::Result<()> {
        self.runtime.bootstrap().await
    }
    pub async fn shutdown(&self) -> anyhow::Result<()> {
        self.runtime.shutdown().await
    }
    pub async fn ready(&self) -> bool {
        self.runtime.ready().await
    }
    /// Local diagnostics only; use `read` for application queries.
    pub async fn inspect(&self) -> Value {
        self.runtime.inspect().await
    }

    /// Read after a fresh quorum barrier. The callback must enforce read policy.
    pub async fn read<T>(
        &self,
        read: impl FnOnce(View<'_>) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        tokio::time::timeout(DEADLINE, async {
            self.runtime.barrier().await?;
            let state = self.runtime.inner.machine.state.read().await;
            read(View { state: &state })
        })
        .await
        .context("application read deadline")?
    }

    /// Serialize admission, establish a quorum barrier, run application policy
    /// and decision, then commit one canonical batch with a revision condition.
    /// The callback is never cached or bypassed by a technical request receipt.
    /// It must not block, access the network, or publish external effects.
    ///
    /// Once admitted, work may continue after caller cancellation. A timeout,
    /// leadership loss or storage error can mean an unknown commit outcome: retry
    /// the SAME business key and payload, with authorization checked again by
    /// the callback. There is no implicit business deduplication or receipt TTL.
    pub async fn execute<F>(&self, decide: F) -> anyhow::Result<Value>
    where
        F: FnOnce(View<'_>) -> anyhow::Result<Decision> + Send + 'static,
    {
        let slot = self
            .runtime
            .inner
            .slots
            .clone()
            .try_acquire_owned()
            .context("application admission full")?;
        let store = self.clone();
        let task = tokio::spawn(async move {
            let _slot = slot;
            tokio::time::timeout(DEADLINE * 2, store.commit(decide))
                .await
                .context("application command deadline")?
        });
        tokio::time::timeout(DEADLINE, task)
            .await
            .context("application outcome unknown: deadline")?
            .context("application decision task failed")?
    }
    async fn commit<F>(&self, decide: F) -> anyhow::Result<Value>
    where
        F: FnOnce(View<'_>) -> anyhow::Result<Decision>,
    {
        let _admission = self.runtime.inner.admission.lock().await;
        self.runtime.barrier().await?;
        let state = self.runtime.inner.machine.state.read().await;
        let (writes, reply) = match decide(View { state: &state })? {
            Decision::Read(reply) => return Ok(reply),
            Decision::Commit { writes, reply } => (writes, reply),
        };
        validate_writes(&state.models, &writes)?;
        let change = Change::Batch(writes);
        let fingerprint = Sha256::digest(serde_json::to_vec(&(&change, &reply))?).into();
        let command = Command {
            version: 1,
            contract: state.contract,
            model: self.namespace.clone(),
            request_id: uuid::Uuid::new_v4().to_string(),
            fingerprint,
            revision: state.revision,
            change,
            reply: Reply { status: 200, body: Some(json!({"application_reply":reply})) },
        };
        ensure!(
            serde_json::to_vec(&command)?.len() <= 256 * 1024,
            "application command exceeds 256 KiB"
        );
        let mut candidate = state.clone();
        candidate.execute(&command)?;
        ensure!(
            serde_json::to_vec(&candidate)?.len() <= state::MAX_CHECKPOINT - 64 * 1024,
            "application checkpoint capacity exceeded"
        );
        drop(state);
        let result = self.runtime.inner.raft.client_write(command).await?.data;
        ensure!(
            result.status == 200,
            "application revision changed before commit; decide again with the same business key"
        );
        result
            .body
            .and_then(|body| body.get("application_reply").cloned())
            .context("missing committed application reply")
    }
}

pub(super) fn validate_writes(
    collections: &BTreeMap<String, BTreeMap<String, Value>>,
    writes: &[Write],
) -> std::io::Result<()> {
    let mut seen = BTreeSet::new();
    if writes.is_empty()
        || writes.iter().any(|w| {
            !collections.contains_key(w.collection())
                || w.key().is_empty()
                || w.key().len() > 1024
                || !seen.insert((w.collection(), w.key()))
        })
    {
        return Err(std::io::Error::other(
            "invalid atomic batch: empty, unknown collection, invalid key or duplicate target",
        ));
    }
    Ok(())
}
