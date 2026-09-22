//! Immutable enrollment and fail-closed, single-use initial bootstrap.
use super::*;
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NodeIdentity {
    pub cluster_id: String,
    pub node_id: u64,
    pub bootstrap_node: u64,
    pub voters: BTreeMap<u64, [u8; 32]>,
}
impl NodeIdentity {
    /// The common initial plan excludes per-node IDs and dial addresses.
    pub(crate) fn plan_digest(&self) -> io::Result<[u8; 32]> {
        use sha2::{Digest, Sha256};
        self.validate()?;
        let mut hash = Sha256::new();
        hash.update(b"lithair-initial-group-v1\0");
        hash.update((self.cluster_id.len() as u64).to_le_bytes());
        hash.update(self.cluster_id.as_bytes());
        hash.update(self.bootstrap_node.to_le_bytes());
        for (id, fingerprint) in &self.voters {
            hash.update(id.to_le_bytes());
            hash.update(fingerprint);
        }
        Ok(hash.finalize().into())
    }

    pub(crate) fn validate(&self) -> io::Result<()> {
        if self.cluster_id.is_empty() || self.cluster_id.len() > 128 {
            return Err(invalid("cluster ID must contain 1 to 128 bytes"));
        }
        if self.voters.len() != 3
            || !self.voters.contains_key(&self.node_id)
            || !self.voters.contains_key(&self.bootstrap_node)
            || self.voters.values().collect::<BTreeSet<_>>().len() != 3
        {
            return Err(invalid(
                "three distinct enrolled voters must include the local and bootstrap nodes",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BoundNode {
    pub identity: NodeIdentity,
    bootstrap_claimed: bool,
}
impl BoundNode {
    pub(super) fn new(identity: NodeIdentity) -> Self {
        Self { identity, bootstrap_claimed: false }
    }
}

// Deliberately not Clone or serializable: only a completed durable claim creates
// a permit. Dropping it cannot reset the on-disk claim, including after restart.
pub(crate) struct BootstrapPermit {
    node_id: u64,
    voters: BTreeSet<u64>,
}
impl BootstrapPermit {
    pub(crate) async fn initialize<C>(self, raft: &openraft::Raft<C>) -> anyhow::Result<()>
    where
        C: RaftTypeConfig<NodeId = u64>,
        C::Node: Default,
    {
        anyhow::ensure!(raft.metrics().borrow().id == self.node_id, "bootstrap node mismatch");
        let members = self
            .voters
            .into_iter()
            .map(|id| (id, C::Node::default()))
            .collect::<BTreeMap<_, _>>();
        raft.initialize(members).await?;
        Ok(())
    }
}

impl<C: RaftTypeConfig<NodeId = u64>> DurableLog<C> {
    /// Offline validation holds the normal exclusive lock but never repairs or
    /// cleans the store. It is not evidence of quorum or application readiness.
    pub(crate) async fn inspect_for_node(
        directory: impl AsRef<Path>,
        identity: NodeIdentity,
    ) -> anyhow::Result<serde_json::Value> {
        let directory = directory.as_ref().to_owned();
        tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let inner = Inner::<C>::load(directory, false, Some(identity), None, false)?;
            let node = inner.end.node.as_ref().ok_or_else(|| invalid("unbound store"))?;
            Ok(serde_json::json!({
                "manifest_version":inner.end.version,
                "credentials":inner.end.credential_policy()?,
                "store_id":uuid::Uuid::from_bytes(inner.end.identity).to_string(),
                "bootstrap_claimed":node.bootstrap_claimed,
                "pristine":inner.state.vote.is_none() && inner.state.logs.is_empty()
                    && inner.state.committed.is_none() && inner.state.purged.is_none() && inner.snapshot.is_none(),
                "retained_entries":inner.state.logs.len(),
                "commit_index":inner.state.committed.as_ref().map(|id|id.index),
                "purged_index":inner.state.purged.as_ref().map(|id|id.index),
                "snapshot_index":inner.snapshot.as_ref().and_then(|s|s.meta.last_log_id.as_ref()).map(|id|id.index),
                "durable_journal_bytes":inner.end.offset,
                "ignored_tail_bytes":inner.journal.metadata()?.len()-inner.end.offset,
                "scope":"offline_consensus_integrity"
            }))
        }).await?
    }

    /// Explicit provisioning only. An existing unbound store is never adopted.
    pub(crate) async fn create_for_node(
        directory: impl AsRef<Path>,
        identity: NodeIdentity,
    ) -> Result<Self, StorageError<u64>> {
        Self::load(directory, true, Some(identity)).await
    }
    pub(crate) async fn open_for_node(
        directory: impl AsRef<Path>,
        identity: NodeIdentity,
    ) -> Result<Self, StorageError<u64>> {
        Self::load(directory, false, Some(identity)).await
    }
    pub(crate) async fn node_identity(&self) -> Result<NodeIdentity, StorageError<u64>> {
        self.access(ErrorVerb::Read, |inner| {
            inner
                .end
                .node
                .as_ref()
                .map(|n| n.identity.clone())
                .ok_or_else(|| invalid("unbound OpenRaft store"))
        })
        .await
    }

    /// Claim must precede OpenRaft initialization. Failure after admission is
    /// indeterminate; never reset or automatically retry an interrupted claim.
    pub(crate) async fn claim_bootstrap(&self) -> Result<BootstrapPermit, StorageError<u64>> {
        self.access(ErrorVerb::Write, |inner| {
            let node = inner.end.node.as_ref().ok_or_else(|| invalid("unbound OpenRaft store"))?;
            if node.bootstrap_claimed
                || node.identity.node_id != node.identity.bootstrap_node
                || inner.end.credential_policy()?.generation != 0
            {
                return Err(invalid("bootstrap already claimed or not the designated node"));
            }
            if inner.state.vote.is_some()
                || !inner.state.logs.is_empty()
                || inner.state.committed.is_some()
                || inner.state.purged.is_some()
                || inner.snapshot.is_some()
            {
                return Err(invalid("bootstrap requires a pristine consensus store"));
            }
            let permit = BootstrapPermit {
                node_id: node.identity.node_id,
                voters: node.identity.voters.keys().copied().collect(),
            };
            let mut claimed = node.clone();
            claimed.bootstrap_claimed = true;
            let end = DurableEnd { node: Some(claimed), ..inner.end.clone() };
            inner.failed = true;
            inner.activate(&end)?;
            inner.end = end;
            inner.checkpoint(Stage::Complete)?;
            inner.failed = false;
            Ok(permit)
        })
        .await
    }

    /// Operator action, never called by recovery or peer availability changes.
    /// The caller must pair this store with the supplied Raft instance.
    pub(crate) async fn bootstrap(&self, raft: &openraft::Raft<C>) -> anyhow::Result<()>
    where
        C::Node: Default,
    {
        anyhow::ensure!(
            self.node_identity().await?.node_id == raft.metrics().borrow().id,
            "bootstrap node mismatch"
        );
        self.claim_bootstrap().await?.initialize(raft).await
    }
}
