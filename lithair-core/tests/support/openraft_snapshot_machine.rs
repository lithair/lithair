//! Test-only application state. Production backends must supply their own
//! deterministic apply and validate a complete native/SQL/session checkpoint.
use crate::durable_log::DurableLog;
use openraft::{
    Entry, ErrorSubject, ErrorVerb, LogId, LogState, RaftLogReader, RaftSnapshotBuilder,
    RaftStorage, Snapshot, SnapshotMeta, StorageError, StoredMembership, Vote,
};
use openraft_memstore::{ClientResponse, MemStore, MemStoreStateMachine, TypeConfig};
use std::{
    io::Cursor,
    ops::RangeBounds,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

#[derive(Clone)]
pub(crate) struct SnapshotMachine {
    pub durable: DurableLog<TypeConfig>,
    pub memory: Arc<MemStore>,
    pub installs: Arc<AtomicU64>,
}
fn invalid(message: &str) -> StorageError<u64> {
    StorageError::from_io_error(
        ErrorSubject::StateMachine,
        ErrorVerb::Read,
        std::io::Error::new(std::io::ErrorKind::InvalidData, message),
    )
}
fn validate(meta: &SnapshotMeta<u64, ()>, data: &[u8]) -> std::io::Result<()> {
    let state: MemStoreStateMachine = serde_json::from_slice(data)?;
    if state.last_applied_log != meta.last_log_id || state.last_membership != meta.last_membership {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "snapshot metadata disagrees with application state",
        ));
    }
    Ok(())
}
impl SnapshotMachine {
    pub async fn restore(durable: DurableLog<TypeConfig>) -> Result<Self, StorageError<u64>> {
        let mut machine = Self {
            durable,
            memory: MemStore::new_async().await,
            installs: Arc::new(AtomicU64::new(0)),
        };
        if let Some(snapshot) = machine.durable.snapshot().await? {
            validate(&snapshot.meta, &snapshot.data).map_err(|e| invalid(&e.to_string()))?;
            machine
                .memory
                .install_snapshot(&snapshot.meta, Box::new(Cursor::new(snapshot.data.to_vec())))
                .await?;
        }
        Ok(machine)
    }
}
impl RaftSnapshotBuilder<TypeConfig> for SnapshotMachine {
    async fn build_snapshot(&mut self) -> Result<Snapshot<TypeConfig>, StorageError<u64>> {
        let state = self.memory.get_state_machine().await;
        let data = serde_json::to_vec(&state).map_err(|e| invalid(&e.to_string()))?;
        let meta = SnapshotMeta {
            last_log_id: state.last_applied_log,
            last_membership: state.last_membership,
            snapshot_id: uuid::Uuid::new_v4().to_string(),
        };
        self.durable.save_snapshot(meta.clone(), data.clone()).await?;
        Ok(Snapshot { meta, snapshot: Box::new(Cursor::new(data)) })
    }
}
impl RaftLogReader<TypeConfig> for SnapshotMachine {
    async fn try_get_log_entries<R: RangeBounds<u64> + Clone + std::fmt::Debug + Send>(
        &mut self,
        range: R,
    ) -> Result<Vec<Entry<TypeConfig>>, StorageError<u64>> {
        self.durable.try_get_log_entries(range).await
    }
}
impl RaftStorage<TypeConfig> for SnapshotMachine {
    type LogReader = DurableLog<TypeConfig>;
    type SnapshotBuilder = Self;
    async fn get_log_reader(&mut self) -> Self::LogReader {
        self.durable.clone()
    }
    async fn get_log_state(&mut self) -> Result<LogState<TypeConfig>, StorageError<u64>> {
        self.durable.get_log_state().await
    }
    async fn save_vote(&mut self, vote: &Vote<u64>) -> Result<(), StorageError<u64>> {
        self.durable.save_vote(vote).await
    }
    async fn read_vote(&mut self) -> Result<Option<Vote<u64>>, StorageError<u64>> {
        self.durable.read_vote().await
    }
    async fn save_committed(&mut self, id: Option<LogId<u64>>) -> Result<(), StorageError<u64>> {
        self.durable.save_committed(id).await
    }
    async fn read_committed(&mut self) -> Result<Option<LogId<u64>>, StorageError<u64>> {
        self.durable.read_committed().await
    }
    async fn append_to_log<I: IntoIterator<Item = Entry<TypeConfig>> + Send>(
        &mut self,
        entries: I,
    ) -> Result<(), StorageError<u64>> {
        self.durable.append_to_log(entries).await
    }
    async fn delete_conflict_logs_since(
        &mut self,
        id: LogId<u64>,
    ) -> Result<(), StorageError<u64>> {
        self.durable.delete_conflict_logs_since(id).await
    }
    async fn purge_logs_upto(&mut self, id: LogId<u64>) -> Result<(), StorageError<u64>> {
        self.durable.wait_for_snapshot(id).await?;
        self.durable.purge_logs_upto(id).await?;
        self.durable.compact().await
    }
    async fn last_applied_state(
        &mut self,
    ) -> Result<(Option<LogId<u64>>, StoredMembership<u64, ()>), StorageError<u64>> {
        self.memory.last_applied_state().await
    }
    async fn apply_to_state_machine(
        &mut self,
        entries: &[Entry<TypeConfig>],
    ) -> Result<Vec<ClientResponse>, StorageError<u64>> {
        self.memory.apply_to_state_machine(entries).await
    }
    async fn get_snapshot_builder(&mut self) -> Self::SnapshotBuilder {
        self.clone()
    }
    async fn begin_receiving_snapshot(
        &mut self,
    ) -> Result<Box<Cursor<Vec<u8>>>, StorageError<u64>> {
        Ok(Box::new(Cursor::new(Vec::new())))
    }
    async fn install_snapshot(
        &mut self,
        meta: &SnapshotMeta<u64, ()>,
        snapshot: Box<Cursor<Vec<u8>>>,
    ) -> Result<(), StorageError<u64>> {
        validate(meta, snapshot.get_ref()).map_err(|e| invalid(&e.to_string()))?;
        self.durable.save_snapshot(meta.clone(), snapshot.get_ref().clone()).await?;
        self.memory.install_snapshot(meta, snapshot).await?;
        self.installs.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    async fn get_current_snapshot(
        &mut self,
    ) -> Result<Option<Snapshot<TypeConfig>>, StorageError<u64>> {
        Ok(self
            .durable
            .snapshot()
            .await?
            .map(|s| Snapshot { meta: s.meta, snapshot: Box::new(Cursor::new(s.data.to_vec())) }))
    }
}
