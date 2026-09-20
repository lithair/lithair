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
            if node.bootstrap_claimed || node.identity.node_id != node.identity.bootstrap_node {
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
