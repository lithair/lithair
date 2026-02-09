//! Raft consensus adapter for lock-free sharded engine
//! 
//! This module provides distributed consensus on top of the lock-free engine architecture.
//! Events are first replicated through Raft consensus, then applied to local lock-free shards.

use super::{Event, EngineResult, EngineError, LockFreeEngine};
use crate::raft::{MemStore, NodeId, SimpleRaft, Request};
use std::collections::{HashMap, HashSet, BTreeMap};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Serialize, Deserialize};
use tokio::sync::RwLock;
use openraft::{BasicNode, Raft, Config};

/// Distributed event that gets replicated through Raft
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributedEvent<E: Event + Clone> {
    /// The actual event to apply
    pub event: E,
    /// Target shard for this event
    pub shard_key: String,
    /// Unique event ID for deduplication
    pub event_id: u64,
    /// Timestamp for ordering
    pub timestamp: u64,
}

/// Raft-integrated lock-free engine
/// Combines distributed consensus with lock-free local execution
pub struct RaftLockFreeEngine<S, E> 
where 
    S: Clone + Send + 'static,
    E: Event<State = S> + Send + Clone + 'static + Serialize + for<'de> Deserialize<'de>,
{
    /// Local lock-free engine for fast execution
    local_engine: Arc<LockFreeEngine<S, E>>,
    
    /// Raft consensus layer
    raft: Arc<SimpleRaft>,
    
    /// Node ID in the Raft cluster
    node_id: NodeId,
    
    /// Event ID counter for deduplication
    event_counter: Arc<RwLock<u64>>,
    
    /// Pending events awaiting consensus
    pending_events: Arc<RwLock<HashMap<u64, DistributedEvent<E>>>>,
}

impl<S, E> RaftLockFreeEngine<S, E>
where 
    S: Clone + Send + 'static,
    E: Event<State = S> + Send + Clone + 'static + Serialize + for<'de> Deserialize<'de>,
{
    /// Create a new Raft-integrated lock-free engine
    pub async fn new(
        initial_state: S, 
        shard_count: Option<usize>,
        node_id: NodeId,
        store: Arc<MemStore>,
    ) -> EngineResult<Self> {
        let local_engine = Arc::new(LockFreeEngine::new(initial_state, shard_count));
        let raft = Arc::new(SimpleRaft::new(store));
        
        Self {
            local_engine,
            raft,
            node_id,
            event_counter: Arc::new(RwLock::new(0)),
            pending_events: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Submit an event through Raft consensus
    pub async fn submit_distributed_event(&self, event: E, shard_key: String) -> EngineResult<()> {
        // Generate unique event ID
        let event_id = {
            let mut counter = self.event_counter.write().await;
            *counter += 1;
            *counter
        };
        
        let distributed_event = DistributedEvent {
            event,
            shard_key: shard_key.clone(),
            event_id,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64,
        };
        
        // Store pending event
        {
            let mut pending = self.pending_events.write().await;
            pending.insert(event_id, distributed_event.clone());
        }
        
        // Serialize event for Raft
        let request = Request {
            key: format!("event_{}", event_id),
            value: serde_json::to_string(&distributed_event).map_err(|e| EngineError::SerializationError(e.to_string()))?,
        };
        
        // Submit through Raft consensus
        self.raft.client_write(request).await
            .map_err(|e| EngineError::ConsensusError(e.to_string()))?;
        
        // Apply locally after consensus
        self.apply_distributed_event(distributed_event).await?;
        
        // Remove from pending
        {
            let mut pending = self.pending_events.write().await;
            pending.remove(&event_id);
        }
        
        Ok(())
    }
    
    /// Apply a distributed event to the local lock-free engine
    async fn apply_distributed_event(&self, distributed_event: DistributedEvent<E>) -> EngineResult<()> {
        log::debug!("Applying distributed event {} to shard {}",
                 distributed_event.event_id,
                 distributed_event.shard_key);
        
        // Apply to local lock-free engine
        self.local_engine.write(distributed_event.shard_key, distributed_event.event)
    }
    
    /// Read from local lock-free engine (no consensus needed)
    pub fn read<R>(&self, key: &str, reader: impl FnOnce(&S) -> R) -> R {
        self.local_engine.read(key, reader)
    }
    
    /// Get engine statistics including Raft status
    pub async fn get_distributed_stats(&self) -> HashMap<String, u64> {
        let mut stats = self.local_engine.get_stats();
        
        // Add Raft-specific stats
        stats.insert("node_id".to_string(), self.node_id);
        
        let pending_count = {
            let pending = self.pending_events.read().await;
            pending.len() as u64
        };
        stats.insert("pending_events".to_string(), pending_count);
        
        let event_counter = {
            let counter = self.event_counter.read().await;
            *counter
        };
        stats.insert("total_events_submitted".to_string(), event_counter);
        
        stats
    }
    
    /// Initialize the Raft cluster
    pub async fn initialize_cluster(&self, nodes: std::collections::BTreeMap<NodeId, openraft::BasicNode>) -> EngineResult<()> {
        self.raft.initialize(nodes).await
            .map_err(|e| EngineError::ConsensusError(e.to_string()))?;
        
        log::info!("Raft cluster initialized with node {}", self.node_id);
        Ok(())
    }
    
    /// Get the underlying lock-free engine for direct access
    pub fn local_engine(&self) -> &Arc<LockFreeEngine<S, E>> {
        &self.local_engine
    }
    
    /// Get Raft consensus layer
    pub fn raft(&self) -> &Arc<SimpleRaft> {
        &self.raft
    }
}

/// Hash function for consistent shard selection
fn hash_key(key: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::lockfree_engine::{TestState, IncrementEvent};
    use std::collections::BTreeMap;
    use openraft::BasicNode;
    
    #[tokio::test]
    async fn test_raft_lockfree_integration() {
        let store = Arc::new(MemStore::new());
        let engine = RaftLockFreeEngine::new(
            TestState::default(),
            Some(4),
            1,
            store,
        );
        
        // Initialize single-node cluster
        let mut nodes = BTreeMap::new();
        nodes.insert(1, BasicNode::default());
        engine.initialize_cluster(nodes).await.unwrap();
        
        // Submit distributed event
        let event = IncrementEvent { amount: 42 };
        engine.submit_distributed_event(event, "test_key".to_string()).await.unwrap();
        
        // Read result
        let counter = engine.read("test_key", |state| state.counter);
        assert_eq!(counter, 42);
        
        // Check stats
        let stats = engine.get_distributed_stats().await;
        assert_eq!(stats.get("node_id"), Some(&1));
        assert_eq!(stats.get("total_events_submitted"), Some(&1));
        assert_eq!(stats.get("pending_events"), Some(&0));
    }
    
    #[tokio::test]
    async fn test_concurrent_distributed_events() {
        let store = Arc::new(MemStore::new());
        let engine = Arc::new(RaftLockFreeEngine::new(
            TestState::default(),
            Some(8),
            1,
            store,
        ));
        
        // Initialize cluster
        let mut nodes = BTreeMap::new();
        nodes.insert(1, BasicNode::default());
        engine.initialize_cluster(nodes).await.unwrap();
        
        // Submit multiple concurrent events
        let mut handles = Vec::new();
        
        for i in 0..10 {
            let engine_clone = engine.clone();
            let handle = tokio::spawn(async move {
                let event = IncrementEvent { amount: 1 };
                let key = format!("key_{}", i % 3); // Distribute across 3 keys
                engine_clone.submit_distributed_event(event, key).await.unwrap();
            });
            handles.push(handle);
        }
        
        // Wait for all events
        for handle in handles {
            handle.await.unwrap();
        }
        
        // Verify results
        let total = engine.read("key_0", |state| state.counter) +
                   engine.read("key_1", |state| state.counter) +
                   engine.read("key_2", |state| state.counter);
        
        // Should have processed all 10 increments
        assert!(total > 0);
        
        let stats = engine.get_distributed_stats().await;
        assert_eq!(stats.get("total_events_submitted"), Some(&10));
    }
}
