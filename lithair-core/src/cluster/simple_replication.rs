use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};

use crate::consensus::ReplicatedModel;
use crate::http::HttpExposable;

/// Simple but efficient data replication system
/// Replicates data changes from leader to followers via HTTP
pub struct SimpleDataReplicator<T>
where
    T: ReplicatedModel + HttpExposable + Clone + Send + Sync + 'static,
{
    node_id: u64,
    is_leader: bool,
    peers: Vec<String>,
    client: Client,
    data_cache: Arc<RwLock<HashMap<String, T>>>,
    processed_bulk_batches: Arc<RwLock<HashSet<String>>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
    struct TestModel {
        id: String,
        name: String,
    }

    impl HttpExposable for TestModel {
        fn http_base_path() -> &'static str {
            "test_models"
        }
        fn primary_key_field() -> &'static str {
            "id"
        }
        fn get_primary_key(&self) -> String {
            self.id.clone()
        }
        fn validate(&self) -> Result<(), String> {
            Ok(())
        }
    }

    impl ReplicatedModel for TestModel {
        fn needs_replication() -> bool {
            true
        }
        fn replicated_fields() -> Vec<&'static str> {
            vec!["id", "name"]
        }
    }

    impl crate::lifecycle::LifecycleAware for TestModel {
        fn lifecycle_policy_for_field(
            &self,
            _field_name: &str,
        ) -> Option<crate::lifecycle::FieldPolicy> {
            None
        }
        fn all_field_names(&self) -> Vec<&'static str> {
            vec!["id", "name"]
        }
        fn model_name(&self) -> &'static str {
            "TestModel"
        }
    }

    #[tokio::test]
    async fn test_bulk_dedupe_marks_and_checks() {
        let replicator = SimpleDataReplicator::<TestModel>::new(1, false, vec![]);
        assert!(!replicator.has_processed_bulk("batch-1").await);
        replicator.mark_bulk_processed("batch-1".to_string()).await;
        assert!(replicator.has_processed_bulk("batch-1").await);
        assert!(!replicator.has_processed_bulk("batch-2").await);
    }
}

/// Replication message for sending data between nodes
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ReplicationMessage<T> {
    pub operation: String, // "create", "update", "delete"
    pub data: Option<T>,
    pub id: Option<String>,
    pub leader_node_id: u64,
    pub timestamp: u64,
}

/// Bulk replication message for sending multiple items at once
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ReplicationBulkMessage<T> {
    pub operation: String, // "create_bulk"
    pub items: Vec<T>,
    pub leader_node_id: u64,
    pub timestamp: u64,
    pub batch_id: String,
}

impl<T> SimpleDataReplicator<T>
where
    T: ReplicatedModel
        + HttpExposable
        + Clone
        + Send
        + Sync
        + 'static
        + for<'de> Deserialize<'de>
        + Serialize,
{
    #[inline]
    fn is_verbose() -> bool {
        std::env::var("RS_VERBOSE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    }
    /// Check if this node is the leader
    pub fn is_leader(&self) -> bool {
        self.is_leader
    }
    /// Create new simple replicator
    pub fn new(node_id: u64, is_leader: bool, peers: Vec<String>) -> Self {
        Self {
            node_id,
            is_leader,
            peers,
            client: Client::new(),
            data_cache: Arc::new(RwLock::new(HashMap::new())),
            processed_bulk_batches: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Add data to cache and replicate to followers if leader
    pub async fn replicate_create(&self, data: T) -> Result<()> {
        let id = self.extract_id(&data);

        // Add to local cache
        {
            let mut cache = self.data_cache.write().await;
            cache.insert(id.clone(), data.clone());
        }

        // If we're the leader, replicate to followers
        if self.is_leader {
            let message = ReplicationMessage {
                operation: "create".to_string(),
                data: Some(data),
                id: Some(id),
                leader_node_id: self.node_id,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
            };

            self.send_to_followers(message).await?;
        }

        Ok(())
    }

    /// Compute the file path to store processed bulk batch ids
    fn batches_file_path(&self) -> PathBuf {
        // Follows DeclarativeCluster data layout and honors EXPERIMENT_DATA_BASE
        let base_dir = std::env::var("EXPERIMENT_DATA_BASE").unwrap_or_else(|_| "data".to_string());
        let dir = format!("{}/pure_node_{}/raft", base_dir, self.node_id);
        PathBuf::from(dir).join("processed_batches.json")
    }

    /// Load processed bulk batch ids from disk (best-effort)
    pub async fn load_processed_batches_from_disk(&self) -> Result<()> {
        let path = self.batches_file_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await.ok();
        }
        match fs::read(&path).await {
            Ok(bytes) => {
                if let Ok(list) = serde_json::from_slice::<Vec<String>>(&bytes) {
                    let mut set = self.processed_bulk_batches.write().await;
                    set.clear();
                    for id in list {
                        set.insert(id);
                    }
                    if Self::is_verbose() {
                        log::debug!("Loaded {} processed bulk batch_ids", set.len());
                    }
                }
            }
            Err(_e) => {
                // File may not exist on first run; ignore
            }
        }
        Ok(())
    }

    /// Persist processed bulk batch ids to disk (best-effort)
    pub async fn persist_processed_batches_to_disk(&self) -> Result<()> {
        let path = self.batches_file_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await.ok();
        }
        let set = self.processed_bulk_batches.read().await;
        let list: Vec<&String> = set.iter().collect();
        let json = serde_json::to_vec(&list)?;
        fs::write(&path, json).await?;
        Ok(())
    }

    /// Returns true if the given bulk batch_id has already been processed
    pub async fn has_processed_bulk(&self, batch_id: &str) -> bool {
        let set = self.processed_bulk_batches.read().await;
        set.contains(batch_id)
    }

    /// Marks the given bulk batch_id as processed
    pub async fn mark_bulk_processed(&self, batch_id: String) {
        {
            let mut set = self.processed_bulk_batches.write().await;
            set.insert(batch_id);
        }
        // Persist asynchronously; ignore errors (logged) to keep hot path fast
        if let Err(e) = self.persist_processed_batches_to_disk().await {
            log::warn!("Failed to persist processed batch ids: {}", e);
        }
    }

    /// Send bulk replication message to all followers
    async fn send_bulk_to_followers(&self, message: ReplicationBulkMessage<T>) -> Result<()> {
        let max_retries = 5u32;
        for peer in &self.peers {
            let url = format!("http://{}/internal/replicate_bulk", peer);

            let mut attempt = 0u32;
            loop {
                if Self::is_verbose() {
                    log::debug!(
                        "Replicating BULK ({} items) to {} (attempt {} of {})",
                        message.items.len(),
                        peer,
                        attempt + 1,
                        max_retries
                    );
                }
                let result = self
                    .client
                    .post(&url)
                    .json(&message)
                    .timeout(Duration::from_secs(15))
                    .send()
                    .await;

                match result {
                    Ok(response) => {
                        if response.status().is_success() {
                            if Self::is_verbose() {
                                log::debug!("BULK replicate to {} successful", peer);
                            }
                            break;
                        } else {
                            log::error!(
                                "BULK replicate to {} failed with status {}",
                                peer,
                                response.status()
                            );
                        }
                    }
                    Err(e) => {
                        log::warn!(
                            "Network error replicating BULK to {}: {} (attempt {} of {})",
                            peer,
                            e,
                            attempt + 1,
                            max_retries
                        );
                    }
                }

                attempt += 1;
                if attempt >= max_retries {
                    log::error!(
                        "Giving up BULK replicate to {} after {} attempts",
                        peer,
                        max_retries
                    );
                    break;
                }
                let backoff = Duration::from_millis(200 * (1u64 << attempt.min(5)));
                sleep(backoff).await;
            }
        }
        Ok(())
    }

    /// Bulk create replication: add a batch to cache and replicate once if leader
    pub async fn replicate_bulk_create(&self, items: Vec<T>) -> Result<()> {
        // Update local cache
        {
            let mut cache = self.data_cache.write().await;
            for item in items.iter() {
                let id = self.extract_id(item);
                cache.insert(id, item.clone());
            }
        }

        if self.is_leader {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let batch_id = format!("bulk-{}-{}-{}", self.node_id, now_ms, items.len());
            let message = ReplicationBulkMessage {
                operation: "create_bulk".to_string(),
                items,
                leader_node_id: self.node_id,
                timestamp: now_ms,
                batch_id,
            };
            self.send_bulk_to_followers(message).await?;
        }

        Ok(())
    }

    /// Update data in cache and replicate to followers if leader
    pub async fn replicate_update(&self, id: String, data: T) -> Result<()> {
        // Update local cache
        {
            let mut cache = self.data_cache.write().await;
            cache.insert(id.clone(), data.clone());
        }

        // If we're the leader, replicate to followers
        if self.is_leader {
            let message = ReplicationMessage {
                operation: "update".to_string(),
                data: Some(data),
                id: Some(id),
                leader_node_id: self.node_id,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
            };

            self.send_to_followers(message).await?;
        }

        Ok(())
    }

    /// Delete data from cache and replicate to followers if leader
    pub async fn replicate_delete(&self, id: String) -> Result<()> {
        // Remove from local cache
        {
            let mut cache = self.data_cache.write().await;
            cache.remove(&id);
        }

        // If we're the leader, replicate to followers
        if self.is_leader {
            let message = ReplicationMessage {
                operation: "delete".to_string(),
                data: None,
                id: Some(id),
                leader_node_id: self.node_id,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
            };

            self.send_to_followers(message).await?;
        }

        Ok(())
    }

    /// Get all data from cache
    pub async fn get_all_data(&self) -> Vec<T> {
        let cache = self.data_cache.read().await;
        cache.values().cloned().collect()
    }

    /// Get specific data by ID
    pub async fn get_data_by_id(&self, id: &str) -> Option<T> {
        let cache = self.data_cache.read().await;
        cache.get(id).cloned()
    }

    /// Handle incoming replication message (for followers)
    pub async fn handle_replication_message(&self, message: ReplicationMessage<T>) -> Result<()> {
        if self.is_leader {
            // Leaders ignore replication messages
            return Ok(());
        }

        if Self::is_verbose() {
            log::debug!(
                "Received replication: {} - {} from leader {}",
                message.operation,
                message.id.as_deref().unwrap_or("unknown"),
                message.leader_node_id
            );
        }

        let mut cache = self.data_cache.write().await;

        match message.operation.as_str() {
            "create" | "update" => {
                if let (Some(data), Some(id)) = (message.data, message.id) {
                    cache.insert(id, data);
                }
            }
            "delete" => {
                if let Some(id) = message.id {
                    cache.remove(&id);
                }
            }
            _ => {
                if Self::is_verbose() {
                    log::warn!("Unknown replication operation: {}", message.operation);
                }
            }
        }

        Ok(())
    }

    /// Send replication message to all followers
    async fn send_to_followers(&self, message: ReplicationMessage<T>) -> Result<()> {
        // Simple retry: up to 5 attempts with exponential backoff
        let max_retries = 5u32;
        for peer in &self.peers {
            let url = format!("http://{}/internal/replicate", peer);

            let mut attempt = 0u32;
            loop {
                let result = self
                    .client
                    .post(&url)
                    .json(&message)
                    .timeout(Duration::from_secs(8))
                    .send()
                    .await;

                match result {
                    Ok(response) => {
                        if response.status().is_success() {
                            if Self::is_verbose() {
                                log::debug!("Replicate to {} successful", peer);
                            }
                            break;
                        } else {
                            log::error!(
                                "Replicate to {} failed with status {}",
                                peer,
                                response.status()
                            );
                        }
                    }
                    Err(e) => {
                        log::warn!(
                            "Network error replicating to {}: {} (attempt {} of {})",
                            peer,
                            e,
                            attempt + 1,
                            max_retries
                        );
                    }
                }

                attempt += 1;
                if attempt >= max_retries {
                    log::error!("Giving up replicate to {} after {} attempts", peer, max_retries);
                    break;
                }
                let backoff = Duration::from_millis(200 * (1u64 << attempt.min(5)));
                sleep(backoff).await;
            }
        }
        Ok(())
    }

    /// Extract ID from data using JSON serialization (works for any model with id field)
    fn extract_id(&self, data: &T) -> String {
        // Serialize to JSON and extract the id field
        if let Ok(json_value) = serde_json::to_value(data) {
            if let Some(id_value) = json_value.get("id") {
                if let Some(id_str) = id_value.as_str() {
                    return id_str.to_string();
                }
            }
        }
        // Fallback to a generated ID if extraction fails
        format!(
            "unknown_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        )
    }

    /// Sync data from leader (for followers to call periodically)
    pub async fn sync_from_leader(&self, leader_port: u16) -> Result<()> {
        if self.is_leader {
            return Ok(());
        }

        let url = format!("http://127.0.0.1:{}/api/{}", leader_port, T::http_base_path());

        match self.client.get(&url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    match response.json::<Vec<T>>().await {
                        Ok(leader_data) => {
                            if Self::is_verbose() {
                                log::debug!("Syncing {} items from leader", leader_data.len());
                            }

                            // Replace entire cache with leader data
                            let mut cache = self.data_cache.write().await;
                            cache.clear();

                            for item in leader_data {
                                let id = self.extract_id(&item);
                                cache.insert(id, item);
                            }

                            if Self::is_verbose() {
                                log::debug!("Sync completed - {} items in cache", cache.len());
                            }
                        }
                        Err(e) => {
                            log::warn!("Failed to parse sync data: {}", e);
                        }
                    }
                }
            }
            Err(e) => {
                log::warn!("Failed to sync from leader: {}", e);
            }
        }

        Ok(())
    }

    /// Start background sync task for followers
    pub async fn start_background_sync(&self, leader_port: u16) -> Result<()> {
        if self.is_leader {
            return Ok(());
        }

        let sync_interval = Duration::from_secs(5); // Sync every 5 seconds

        loop {
            self.sync_from_leader(leader_port).await.ok();
            sleep(sync_interval).await;
        }
    }
}
