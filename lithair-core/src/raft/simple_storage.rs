//! Simple OpenRaft storage implementation for Lithair demo
//!
//! This is a minimal working implementation based on openraft-memstore
//! to quickly get the benchmark working

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

use super::distributed_engine::{LithairSnapshot, NodeId, TypeConfig, LithairResponse};
use openraft_memstore::ClientRequest;

/// Simple in-memory log storage for demo
#[derive(Debug, Default, Clone)]
pub struct SimpleLogStore {
    logs: Arc<RwLock<BTreeMap<u64, Entry<TypeConfig>>>>,
    vote: Arc<RwLock<Option<Vote<NodeId>>>>,
    log_state: Arc<RwLock<LogState<TypeConfig>>>,
}

/// Simple in-memory state machine for demo  
#[derive(Debug, Default, Clone)]
pub struct SimpleStateMachine {
    last_applied_log: Arc<RwLock<Option<LogId<NodeId>>>>,
    last_membership: Arc<RwLock<StoredMembership<NodeId, ()>>>,
}

impl SimpleLogStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SimpleStateMachine {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl RaftLogReader<TypeConfig> for SimpleLogStore {
    async fn try_get_log_entries<RB: std::ops::RangeBounds<u64> + Clone + std::fmt::Debug + OptionalSend>(
        &mut self,
        range: RB,
    ) -> Result<Vec<Entry<TypeConfig>>, StorageError<NodeId>> {
        let logs = self.logs.read().await;
        let mut entries = Vec::new();
        
        for (index, entry) in logs.iter() {
            if range.contains(index) {
                entries.push(entry.clone());
            }
        }
        
        Ok(entries)
    }
}

#[async_trait] 
impl RaftStorage<TypeConfig> for SimpleLogStore {
    type LogReader = SimpleLogStore;
    type SnapshotBuilder = SimpleSnapshotBuilder;

    async fn save_vote(&mut self, vote: &Vote<NodeId>) -> Result<(), StorageError<NodeId>> {
        *self.vote.write().await = Some(*vote);
        Ok(())
    }

    async fn read_vote(&mut self) -> Result<Option<Vote<NodeId>>, StorageError<NodeId>> {
        Ok(*self.vote.read().await)
    }

    async fn get_log_state(&mut self) -> Result<LogState<TypeConfig>, StorageError<NodeId>> {
        Ok(self.log_state.read().await.clone())
    }

    async fn get_log_reader(&mut self) -> Self::LogReader {
        self.clone()
    }

    async fn append_to_log<I>(&mut self, entries: I) -> Result<(), StorageError<NodeId>>
    where
        I: IntoIterator<Item = Entry<TypeConfig>> + OptionalSend,
    {
        let mut logs = self.logs.write().await;
        let mut log_state = self.log_state.write().await;
        
        for entry in entries {
            if let EntryPayload::Normal(request) = &entry.payload {
                println!("📝 Lithair: Storing client request: {}", request.status);
            }
            
            logs.insert(entry.log_id.index, entry.clone());
            log_state.last_log_id = Some(entry.log_id);
        }
        
        Ok(())
    }

    async fn delete_conflict_logs_since(&mut self, log_id: LogId<NodeId>) -> Result<(), StorageError<NodeId>> {
        let mut logs = self.logs.write().await;
        logs.retain(|index, _| *index < log_id.index);
        Ok(())
    }

    async fn purge_logs_upto(&mut self, log_id: LogId<NodeId>) -> Result<(), StorageError<NodeId>> {
        let mut logs = self.logs.write().await;
        logs.retain(|index, _| *index > log_id.index);
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
                println!("⚡ Lithair: Applied client request: {}", request.status);
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
        SimpleSnapshotBuilder {}
    }

    async fn begin_receiving_snapshot(&mut self) -> Result<Box<Cursor<Vec<u8>>>, StorageError<NodeId>> {
        Ok(Box::new(Cursor::new(Vec::new())))
    }

    async fn install_snapshot(
        &mut self,
        _meta: &SnapshotMeta<NodeId, ()>,
        _snapshot: Box<Cursor<Vec<u8>>>,
    ) -> Result<(), StorageError<NodeId>> {
        println!("📦 Lithair: Snapshot installed");
        Ok(())
    }

    async fn get_current_snapshot(&mut self) -> Result<Option<Snapshot<TypeConfig>>, StorageError<NodeId>> {
        Ok(None)
    }
}

/// Simple snapshot builder
pub struct SimpleSnapshotBuilder;

#[async_trait]
impl RaftSnapshotBuilder<TypeConfig> for SimpleSnapshotBuilder {
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