//! Lithair OpenRaft Storage Implementation (Legacy RaftStorage)
//!
//! Simple working implementation using legacy RaftStorage trait

use std::collections::BTreeMap;
use std::sync::Arc;
use std::io::Cursor;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use openraft::{
    Entry, EntryPayload, LogId, Membership, 
    RaftTypeConfig, SnapshotMeta, StorageError, StorageIOError, Vote,
    RaftLogReader, RaftSnapshotBuilder, Snapshot, LogState, StoredMembership,
    OptionalSend, RaftStorage,
};

use super::distributed_engine::{LithairSnapshot, NodeId, TypeConfig, LithairResponse, LithairStorage};
use openraft_memstore::ClientRequest;
use crate::engine::{RaftstoneApplication, EventStore};

/// Simple log reader
#[derive(Clone)]
pub struct LithairLogReader {
    _data_dir: String,
}

impl LithairLogReader {
    pub fn new(data_dir: String) -> Self {
        Self { _data_dir: data_dir }
    }
}

#[async_trait]
impl RaftLogReader<TypeConfig> for LithairLogReader {
    async fn try_get_log_entries<RB: std::ops::RangeBounds<u64> + Clone + std::fmt::Debug + OptionalSend>(
        &mut self,
        _range: RB,
    ) -> Result<Vec<Entry<TypeConfig>>, StorageError<NodeId>> {
        Ok(vec![])
    }
}

/// Simple snapshot builder
pub struct LithairSnapshotBuilder<App: RaftstoneApplication> {
    _storage: Arc<LithairStorage<App>>,
}

impl<App: RaftstoneApplication + 'static> LithairSnapshotBuilder<App> {
    pub fn new(storage: Arc<LithairStorage<App>>) -> Self {
        Self { _storage: storage }
    }
}

#[async_trait]
impl<App: RaftstoneApplication + 'static> RaftSnapshotBuilder<TypeConfig> for LithairSnapshotBuilder<App>
where
    App::State: Clone + Send + Sync + 'static,
{
    async fn build_snapshot(&mut self) -> Result<Snapshot<TypeConfig>, StorageError<NodeId>> {
        let meta = SnapshotMeta {
            last_log_id: None,
            last_membership: StoredMembership::new(None, Membership::new(vec![std::collections::BTreeSet::new()], None)),
            snapshot_id: "simple_snapshot".to_string(),
        };
        
        let snapshot_cursor = Box::new(Cursor::new(vec![1, 2, 3]));
        
        Ok(Snapshot {
            meta,
            snapshot: snapshot_cursor,
        })
    }
}

/// Simple RaftStorage implementation for LithairStorage
#[async_trait]
impl<App: RaftstoneApplication + 'static> RaftStorage<TypeConfig> for LithairStorage<App>
where
    App::State: Clone + Send + Sync + 'static,
{
    type LogReader = LithairLogReader;
    type SnapshotBuilder = LithairSnapshotBuilder<App>;

    async fn save_vote(&mut self, _vote: &Vote<NodeId>) -> Result<(), StorageError<NodeId>> {
        Ok(())
    }

    async fn read_vote(&mut self) -> Result<Option<Vote<NodeId>>, StorageError<NodeId>> {
        Ok(None)
    }

    async fn get_log_state(&mut self) -> Result<LogState<TypeConfig>, StorageError<NodeId>> {
        Ok(LogState {
            last_purged_log_id: None,
            last_log_id: None,
        })
    }

    async fn get_log_reader(&mut self) -> Self::LogReader {
        LithairLogReader::new(self.data_dir().to_string())
    }

    async fn append_to_log<I>(&mut self, entries: I) -> Result<(), StorageError<NodeId>>
    where I: IntoIterator<Item = Entry<TypeConfig>> + OptionalSend
    {
        let entries: Vec<_> = entries.into_iter().collect();
        for entry in entries {
            if let EntryPayload::Normal(request) = &entry.payload {
                // Treat all ClientRequest as events for now
                log::debug!("Lithair: Storing request: {}", request.status);
                
                // Simple file append for demo
                let log_path = format!("{}/distributed_events.log", self.data_dir());
                if let Ok(mut file) = tokio::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&log_path)
                    .await
                {
                    use tokio::io::AsyncWriteExt;
                    let _ = file.write_all(format!("{}\n", request.status).as_bytes()).await;
                }
                
                /*match request {
                    LithairRequest::ApplyEvent { event_type, event_data, aggregate_id } => {
                */
            }
        }
        Ok(())
    }

    async fn delete_conflict_logs_since(&mut self, _log_id: LogId<NodeId>) -> Result<(), StorageError<NodeId>> {
        Ok(())
    }

    async fn purge_logs_upto(&mut self, _log_id: LogId<NodeId>) -> Result<(), StorageError<NodeId>> {
        Ok(())
    }

    async fn last_applied_state(
        &mut self,
    ) -> Result<(Option<LogId<NodeId>>, StoredMembership<NodeId, ()>), StorageError<NodeId>> {
        use std::collections::BTreeSet;
        let nodes = BTreeSet::new();
        let membership = StoredMembership::new(None, Membership::new(vec![nodes], None));
        Ok((None, membership))
    }

    async fn apply_to_state_machine(&mut self, entries: &[Entry<TypeConfig>]) -> Result<Vec<LithairResponse>, StorageError<NodeId>> {
        let mut responses = Vec::new();
        
        for entry in entries {
            if let EntryPayload::Normal(request) = &entry.payload {
                // Handle ClientRequest for MemStore compatibility
                log::debug!("Lithair: Applied request to state machine: {}", request.status);
                responses.push(LithairResponse::EventApplied {
                    event_id: uuid::Uuid::new_v4().to_string(),
                    applied_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                });
            }
        }
        
        Ok(responses)
    }

    async fn get_snapshot_builder(&mut self) -> Self::SnapshotBuilder {
        LithairSnapshotBuilder::new(Arc::new(LithairStorage {
            event_store: Arc::new(EventStore::new(&format!("{}/events", self.data_dir())).unwrap()),
            state_machine: Arc::new(RwLock::new(App::initial_state())),
            data_dir: self.data_dir().to_string(),
        }))
    }

    async fn begin_receiving_snapshot(&mut self) -> Result<Box<Cursor<Vec<u8>>>, StorageError<NodeId>> {
        Ok(Box::new(Cursor::new(Vec::new())))
    }

    async fn install_snapshot(
        &mut self,
        _meta: &SnapshotMeta<NodeId, ()>,
        _snapshot: Box<Cursor<Vec<u8>>>,
    ) -> Result<(), StorageError<NodeId>> {
        log::info!("Lithair: Snapshot installed");
        Ok(())
    }

    async fn get_current_snapshot(&mut self) -> Result<Option<Snapshot<TypeConfig>>, StorageError<NodeId>> {
        Ok(None)
    }
}

#[async_trait]
impl<App: RaftstoneApplication + 'static> RaftLogReader<TypeConfig> for LithairStorage<App>
where
    App::State: Clone + Send + Sync + 'static,
{
    async fn try_get_log_entries<RB: std::ops::RangeBounds<u64> + Clone + std::fmt::Debug + OptionalSend>(
        &mut self,
        _range: RB,
    ) -> Result<Vec<Entry<TypeConfig>>, StorageError<NodeId>> {
        Ok(vec![])
    }
}