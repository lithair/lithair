//! Intelligent Batching System for Replication
//!
//! This module provides a smart batching system that:
//! - Batches operations for slow followers
//! - Tracks follower health status
//! - Manages desync detection and resync triggers

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use super::consensus_log::LogEntry;

/// Follower health status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowerHealth {
    /// Responding within normal latency (< 500ms)
    Healthy,
    /// Responding slowly (500ms - 5s), batching ops
    Lagging,
    /// Not responding or too far behind (> 5s or > 1000 ops)
    Desynced,
    /// Unknown (initial state or recently added)
    Unknown,
}

/// Follower state tracking
#[derive(Debug)]
pub struct FollowerState {
    /// Peer address
    pub address: String,
    /// Last known replicated index
    pub last_replicated_index: AtomicU64,
    /// Last successful response time
    pub last_response: RwLock<Instant>,
    /// Last response latency
    pub last_latency_ms: AtomicU64,
    /// Current health status
    pub health: RwLock<FollowerHealth>,
    /// Pending entries to send (batched)
    pub pending_entries: RwLock<Vec<LogEntry>>,
    /// Consecutive failures
    pub consecutive_failures: AtomicU64,
}

impl FollowerState {
    pub fn new(address: String) -> Self {
        Self {
            address,
            last_replicated_index: AtomicU64::new(0),
            last_response: RwLock::new(Instant::now()),
            last_latency_ms: AtomicU64::new(0),
            health: RwLock::new(FollowerHealth::Unknown),
            pending_entries: RwLock::new(Vec::new()),
            consecutive_failures: AtomicU64::new(0),
        }
    }

    /// Update state after successful response
    pub async fn record_success(&self, replicated_index: u64, latency_ms: u64) {
        self.last_replicated_index.store(replicated_index, Ordering::SeqCst);
        self.last_latency_ms.store(latency_ms, Ordering::SeqCst);
        self.consecutive_failures.store(0, Ordering::SeqCst);

        *self.last_response.write().await = Instant::now();

        // Update health based on latency
        let mut health = self.health.write().await;
        *health = if latency_ms < 500 {
            FollowerHealth::Healthy
        } else {
            FollowerHealth::Lagging
        };

        // Clear pending entries that were replicated
        let mut pending = self.pending_entries.write().await;
        pending.retain(|e| e.log_id.index > replicated_index);
    }

    /// Update state after failure
    pub async fn record_failure(&self) {
        let failures = self.consecutive_failures.fetch_add(1, Ordering::SeqCst) + 1;

        let mut health = self.health.write().await;
        if failures >= 3 {
            *health = FollowerHealth::Desynced;
        } else {
            *health = FollowerHealth::Lagging;
        }
    }

    /// Check if follower needs resync based on time and index gap
    pub async fn needs_resync(&self, leader_commit_index: u64) -> bool {
        let health = self.health.read().await;
        if *health == FollowerHealth::Desynced {
            return true;
        }

        let last_response = self.last_response.read().await;
        let time_since_response = last_response.elapsed();

        // Desynced if > 5s without response
        if time_since_response > Duration::from_secs(5) {
            return true;
        }

        // Desynced if > 1000 ops behind
        let last_index = self.last_replicated_index.load(Ordering::SeqCst);
        if leader_commit_index > last_index && leader_commit_index - last_index > 1000 {
            return true;
        }

        false
    }

    /// Queue entry for batched send
    pub async fn queue_entry(&self, entry: LogEntry) {
        let mut pending = self.pending_entries.write().await;
        pending.push(entry);
    }

    /// Get and clear pending entries
    pub async fn take_pending(&self) -> Vec<LogEntry> {
        let mut pending = self.pending_entries.write().await;
        std::mem::take(&mut *pending)
    }

    /// Get pending count
    pub async fn pending_count(&self) -> usize {
        self.pending_entries.read().await.len()
    }
}

/// Replication batcher configuration
#[derive(Debug, Clone)]
pub struct BatcherConfig {
    /// Maximum time to wait before sending a batch (default: 10ms)
    pub batch_interval_ms: u64,
    /// Maximum entries per batch (default: 100)
    pub max_batch_size: usize,
    /// Latency threshold for "healthy" status (default: 500ms)
    pub healthy_latency_ms: u64,
    /// Time threshold for "desynced" status (default: 5s)
    pub desync_timeout_secs: u64,
    /// Index gap threshold for "desynced" status (default: 1000)
    pub desync_index_gap: u64,
}

impl Default for BatcherConfig {
    fn default() -> Self {
        Self {
            batch_interval_ms: 10,
            max_batch_size: 100,
            healthy_latency_ms: 500,
            desync_timeout_secs: 5,
            desync_index_gap: 1000,
        }
    }
}

/// Replication batcher manages batching and follower health
pub struct ReplicationBatcher {
    /// Configuration
    config: BatcherConfig,
    /// Follower states by address
    followers: RwLock<HashMap<String, Arc<FollowerState>>>,
    /// Pending batch for immediate send (healthy followers)
    immediate_batch: RwLock<Vec<LogEntry>>,
    /// Last batch send time
    last_batch_time: RwLock<Instant>,
}

impl ReplicationBatcher {
    pub fn new(config: BatcherConfig) -> Self {
        Self {
            config,
            followers: RwLock::new(HashMap::new()),
            immediate_batch: RwLock::new(Vec::new()),
            last_batch_time: RwLock::new(Instant::now()),
        }
    }

    pub fn with_default_config() -> Self {
        Self::new(BatcherConfig::default())
    }

    /// Initialize with peer addresses
    pub async fn initialize(&self, peers: &[String]) {
        let mut followers = self.followers.write().await;
        for peer in peers {
            if !followers.contains_key(peer) {
                followers.insert(peer.clone(), Arc::new(FollowerState::new(peer.clone())));
            }
        }
    }

    /// Get follower state
    pub async fn get_follower(&self, address: &str) -> Option<Arc<FollowerState>> {
        self.followers.read().await.get(address).cloned()
    }

    /// Queue an entry for replication
    /// Returns immediately - entry will be batched and sent appropriately
    pub async fn queue_entry(&self, entry: LogEntry) {
        // Add to immediate batch
        let mut batch = self.immediate_batch.write().await;
        batch.push(entry.clone());

        // Also queue for lagging followers
        let followers = self.followers.read().await;
        for follower in followers.values() {
            let health = follower.health.read().await;
            if *health == FollowerHealth::Lagging {
                drop(health);
                follower.queue_entry(entry.clone()).await;
            }
        }
    }

    /// Check if batch is ready to send
    pub async fn should_send_batch(&self) -> bool {
        let batch = self.immediate_batch.read().await;
        if batch.is_empty() {
            return false;
        }

        // Send if batch is full
        if batch.len() >= self.config.max_batch_size {
            return true;
        }

        // Send if interval has passed
        let last_time = self.last_batch_time.read().await;
        last_time.elapsed() >= Duration::from_millis(self.config.batch_interval_ms)
    }

    /// Take the immediate batch for sending
    pub async fn take_batch(&self) -> Vec<LogEntry> {
        let mut batch = self.immediate_batch.write().await;
        *self.last_batch_time.write().await = Instant::now();
        std::mem::take(&mut *batch)
    }

    /// Record successful replication to a follower
    pub async fn record_success(&self, address: &str, replicated_index: u64, latency_ms: u64) {
        if let Some(follower) = self.get_follower(address).await {
            follower.record_success(replicated_index, latency_ms).await;
        }
    }

    /// Record failed replication attempt
    pub async fn record_failure(&self, address: &str) {
        if let Some(follower) = self.get_follower(address).await {
            follower.record_failure().await;
        }
    }

    /// Get followers that need entries sent
    /// Returns (healthy_peers, entries_for_healthy), (lagging_peers, batched_entries)
    pub async fn get_pending_replications(
        &self,
        _leader_commit_index: u64,
    ) -> (Vec<String>, Vec<(String, Vec<LogEntry>)>) {
        let followers = self.followers.read().await;
        let mut healthy_peers = Vec::new();
        let mut lagging_replications = Vec::new();

        for (address, follower) in followers.iter() {
            let health = follower.health.read().await;
            match *health {
                FollowerHealth::Healthy | FollowerHealth::Unknown => {
                    healthy_peers.push(address.clone());
                }
                FollowerHealth::Lagging => {
                    let pending = follower.take_pending().await;
                    if !pending.is_empty() {
                        lagging_replications.push((address.clone(), pending));
                    }
                }
                FollowerHealth::Desynced => {
                    // Skip - needs resync
                }
            }
        }

        (healthy_peers, lagging_replications)
    }

    /// Get followers that need full resync
    pub async fn get_desynced_followers(&self, leader_commit_index: u64) -> Vec<String> {
        let followers = self.followers.read().await;
        let mut desynced = Vec::new();

        for (address, follower) in followers.iter() {
            if follower.needs_resync(leader_commit_index).await {
                desynced.push(address.clone());
            }
        }

        desynced
    }

    /// Get health summary for status reporting
    pub async fn get_health_summary(&self) -> HashMap<String, FollowerHealth> {
        let followers = self.followers.read().await;
        let mut summary = HashMap::new();

        for (address, follower) in followers.iter() {
            let health = follower.health.read().await;
            summary.insert(address.clone(), *health);
        }

        summary
    }

    /// Get detailed stats for a follower
    pub async fn get_follower_stats(&self, address: &str) -> Option<FollowerStats> {
        let follower = self.get_follower(address).await?;

        let health = *follower.health.read().await;
        let pending_count = follower.pending_count().await;

        Some(FollowerStats {
            address: follower.address.clone(),
            health,
            last_replicated_index: follower.last_replicated_index.load(Ordering::SeqCst),
            last_latency_ms: follower.last_latency_ms.load(Ordering::SeqCst),
            pending_count,
            consecutive_failures: follower.consecutive_failures.load(Ordering::SeqCst),
        })
    }
}

/// Statistics for a follower
#[derive(Debug, Clone)]
pub struct FollowerStats {
    pub address: String,
    pub health: FollowerHealth,
    pub last_replicated_index: u64,
    pub last_latency_ms: u64,
    pub pending_count: usize,
    pub consecutive_failures: u64,
}

impl std::fmt::Display for FollowerHealth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FollowerHealth::Healthy => write!(f, "healthy"),
            FollowerHealth::Lagging => write!(f, "lagging"),
            FollowerHealth::Desynced => write!(f, "desynced"),
            FollowerHealth::Unknown => write!(f, "unknown"),
        }
    }
}

impl serde::Serialize for FollowerHealth {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::consensus_log::{CrudOperation, LogId};

    fn make_entry(index: u64) -> LogEntry {
        LogEntry {
            log_id: LogId::new(1, index),
            operation: CrudOperation::Create {
                model_path: "/test".to_string(),
                data: serde_json::json!({"id": index}),
            },
            timestamp_ms: index * 1000,
        }
    }

    #[tokio::test]
    async fn test_follower_health_transitions() {
        let follower = FollowerState::new("127.0.0.1:8081".to_string());

        // Initially unknown
        assert_eq!(*follower.health.read().await, FollowerHealth::Unknown);

        // Fast response -> healthy
        follower.record_success(1, 100).await;
        assert_eq!(*follower.health.read().await, FollowerHealth::Healthy);

        // Slow response -> lagging
        follower.record_success(2, 800).await;
        assert_eq!(*follower.health.read().await, FollowerHealth::Lagging);

        // Multiple failures -> desynced
        follower.record_failure().await;
        follower.record_failure().await;
        follower.record_failure().await;
        assert_eq!(*follower.health.read().await, FollowerHealth::Desynced);

        // Recovery
        follower.record_success(3, 50).await;
        assert_eq!(*follower.health.read().await, FollowerHealth::Healthy);
    }

    #[tokio::test]
    async fn test_batching() {
        let batcher = ReplicationBatcher::new(BatcherConfig {
            batch_interval_ms: 10,
            max_batch_size: 5,
            ..Default::default()
        });

        // Queue entries
        for i in 1..=3 {
            batcher.queue_entry(make_entry(i)).await;
        }

        // Not ready yet (not full, interval not passed)
        // Would need to wait 10ms for interval

        // Take batch
        let batch = batcher.take_batch().await;
        assert_eq!(batch.len(), 3);

        // Batch is now empty
        let batch = batcher.take_batch().await;
        assert!(batch.is_empty());
    }

    #[tokio::test]
    async fn test_lagging_follower_batching() {
        let batcher = ReplicationBatcher::with_default_config();
        batcher.initialize(&["127.0.0.1:8081".to_string()]).await;

        // Mark follower as lagging
        let follower = batcher.get_follower("127.0.0.1:8081").await.unwrap();
        follower.record_success(0, 800).await; // Slow -> lagging

        // Queue entries
        for i in 1..=5 {
            batcher.queue_entry(make_entry(i)).await;
        }

        // Check pending for lagging follower
        let pending = follower.pending_count().await;
        assert_eq!(pending, 5);

        // Take pending
        let entries = follower.take_pending().await;
        assert_eq!(entries.len(), 5);
        assert_eq!(follower.pending_count().await, 0);
    }
}
