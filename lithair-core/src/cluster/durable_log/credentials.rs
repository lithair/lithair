//! Operator-managed leaf certificate authorization, independent of genesis.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PendingCertificate {
    pub node_id: u64,
    pub certificate_sha256: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CredentialPolicy {
    pub generation: u64,
    pub current: BTreeMap<u64, [u8; 32]>,
    pub pending: Option<PendingCertificate>,
}
impl CredentialPolicy {
    pub(crate) fn initial(identity: &NodeIdentity) -> Self {
        Self { generation: 0, current: identity.voters.clone(), pending: None }
    }
    pub(crate) fn accepts(&self, node_id: u64, pin: [u8; 32]) -> bool {
        self.current.get(&node_id) == Some(&pin)
            || self
                .pending
                .as_ref()
                .is_some_and(|p| p.node_id == node_id && p.certificate_sha256 == pin)
    }
    pub(crate) fn validate(&self, identity: &NodeIdentity) -> io::Result<()> {
        identity.validate()?;
        let unique: std::collections::BTreeSet<_> = self.current.values().collect();
        if !self.current.keys().eq(identity.voters.keys())
            || unique.len() != 3
            || (self.generation % 2 == 1) != self.pending.is_some()
            || (self.generation == 0 && *self != Self::initial(identity))
        {
            return Err(invalid("invalid peer credential policy"));
        }
        if let Some(pending) = &self.pending {
            if !self.current.contains_key(&pending.node_id)
                || unique.contains(&pending.certificate_sha256)
            {
                return Err(invalid(
                    "pending certificate must be new and belong to an enrolled peer",
                ));
            }
        }
        Ok(())
    }
    fn follows(&self, previous: &Self, identity: &NodeIdentity) -> io::Result<()> {
        previous.validate(identity)?;
        self.validate(identity)?;
        if previous.generation.checked_add(1) != Some(self.generation) {
            return Err(invalid("credential transition must advance exactly one generation"));
        }
        let mut expected = previous.current.clone();
        if let Some(pending) = &previous.pending {
            expected.insert(pending.node_id, pending.certificate_sha256);
        }
        if self.current != expected {
            return Err(invalid(
                "credential transition must stage overlap before retiring the old pin",
            ));
        }
        Ok(())
    }
}

impl DurableEnd {
    pub(super) fn credential_policy(&self) -> io::Result<CredentialPolicy> {
        let node = self.node.as_ref().ok_or_else(|| invalid("unbound store"))?;
        let policy = self
            .credentials
            .clone()
            .unwrap_or_else(|| CredentialPolicy::initial(&node.identity));
        policy.validate(&node.identity)?;
        Ok(policy)
    }
}
impl<C: RaftTypeConfig> Inner<C> {
    fn transition_credentials(
        &mut self,
        expected_generation: u64,
        next: CredentialPolicy,
    ) -> io::Result<()> {
        let identity = &self.end.node.as_ref().ok_or_else(|| invalid("unbound store"))?.identity;
        let current = self.end.credential_policy()?;
        if current.generation != expected_generation {
            return Err(invalid("stale credential generation; inspect the store before retrying"));
        }
        next.follows(&current, identity)?;
        let end = DurableEnd { version: 4, credentials: Some(next), ..self.end.clone() };
        // The caller owns the offline store lock. Preserve every consensus byte,
        // snapshot reference and bootstrap claim, including unacknowledged tails.
        self.failed = true;
        self.activate(&end)?;
        self.checkpoint(Stage::Complete)?;
        self.end = end;
        self.failed = false;
        Ok(())
    }
}
impl<C: RaftTypeConfig<NodeId = u64>> DurableLog<C> {
    pub(crate) async fn open_with_credentials(
        directory: impl AsRef<Path>,
        identity: NodeIdentity,
        policy: CredentialPolicy,
    ) -> Result<Self, StorageError<u64>> {
        Self::load_policy(directory, false, Some(identity), Some(policy)).await
    }
    pub(crate) async fn credential_policy(&self) -> Result<CredentialPolicy, StorageError<u64>> {
        self.access(ErrorVerb::Read, |inner| inner.end.credential_policy()).await
    }
    /// Offline compare-and-swap. Never available through a running store handle.
    /// After an I/O failure or cancellation, inspect the durable generation.
    pub(crate) async fn transition_credentials(
        directory: impl AsRef<Path>,
        identity: NodeIdentity,
        expected_generation: u64,
        next: CredentialPolicy,
    ) -> anyhow::Result<()> {
        let directory = directory.as_ref().to_owned();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let mut inner = Inner::<C>::load(directory, false, Some(identity), None, false)?;
            inner.transition_credentials(expected_generation, next)?;
            Ok(())
        })
        .await?
    }
}

#[cfg(test)]
#[path = "../../../tests/support/openraft_credential_faults.rs"]
mod tests;
