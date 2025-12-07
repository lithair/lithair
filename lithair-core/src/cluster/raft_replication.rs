use std::sync::Arc;
use std::collections::BTreeMap;
use anyhow::Result;
use serde::{Serialize, Deserialize};
use openraft::{Config, Raft, RaftMetrics, StorageError};
use openraft::storage::{RaftLogStorage, RaftStateMachine};
use openraft::raft::AppendEntriesRequest;
use openraft::BasicNode;
use openraft_memstore::{ClientRequest, ClientResponse, MemStore, MemStoreStateMachine};
use tokio::sync::RwLock;

use crate::http::HttpExposable;
use crate::consensus::ReplicatedModel;

pub type LithairRaft = Raft<LithairTypeConfig>;
pub type LithairStore = Arc<MemStore<LithairTypeConfig>>;

/// Configuration for Lithair Raft implementation
#[derive(Clone)]
pub struct LithairTypeConfig {}

impl openraft::RaftTypeConfig for LithairTypeConfig {
    type D = LithairRequest;
    type R = LithairResponse;
    type NodeId = u64;
    type Node = BasicNode;
    type Entry = openraft::Entry<LithairTypeConfig>;
    type SnapshotData = String;
    type AsyncRuntime = openraft::TokioRuntime;
}

/// Lithair Request Types - Generic CRUD operations for any model
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum LithairRequest {
    /// Create a new item in a model (generic)
    Create {
        model_path: String,  // e.g., "/api/products"
        data: serde_json::Value,
    },
    /// Update an existing item (generic)
    Update {
        model_path: String,
        id: String,
        data: serde_json::Value,
    },
    /// Delete an item (generic)
    Delete {
        model_path: String,
        id: String,
    },
    /// Batch operation (multiple operations in a single consensus round)
    BatchOperation {
        operations: Vec<LithairRequest>,
    },
    // Legacy variants for backwards compatibility
    CreateProduct { data: serde_json::Value },
    UpdateProduct { id: String, data: serde_json::Value },
    DeleteProduct { id: String },
}

/// Lithair Response Types
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum LithairResponse {
    Success { data: Option<serde_json::Value> },
    Error { message: String },
    NotLeader { leader_id: Option<u64> },
}

/// Data Replication Manager for distributing data across nodes
pub struct DataReplicationManager<T> 
where
    T: ReplicatedModel + HttpExposable + Clone + Send + Sync + 'static,
{
    raft: Arc<LithairRaft>,
    store: LithairStore,
    node_id: u64,
    peers: Vec<String>,
    data_cache: Arc<RwLock<BTreeMap<String, T>>>,
}

impl<T> DataReplicationManager<T> 
where
    T: ReplicatedModel + HttpExposable + Clone + Send + Sync + 'static,
{
    /// Create new replication manager
    pub async fn new(node_id: u64, peers: Vec<String>) -> Result<Self> {
        // Create Raft configuration
        let config = Config {
            heartbeat_interval: 150,
            election_timeout_min: 300,
            election_timeout_max: 600,
            cluster_name: "lithair-cluster".to_string(),
            ..Default::default()
        };

        // Create storage
        let store = Arc::new(MemStore::new());
        
        // Create node map
        let mut node_map = BTreeMap::new();
        node_map.insert(node_id, BasicNode::new("127.0.0.1:0".to_string()));
        
        // Add peers to node map
        for (idx, peer) in peers.iter().enumerate() {
            let peer_id = (idx as u64) + 2; // Start peer IDs from 2
            if peer_id != node_id {
                node_map.insert(peer_id, BasicNode::new(peer.clone()));
            }
        }

        // Initialize cluster if this is node 1 (leader)
        if node_id == 1 && !peers.is_empty() {
            let mut node_ids = vec![node_id];
            node_ids.extend(node_map.keys().cloned());
            store.initialize(node_ids).await?;
        }

        // Create Raft instance
        let raft = Raft::new(node_id, config.clone(), Arc::new(network::LithairNetwork::new()), store.clone());

        let data_cache = Arc::new(RwLock::new(BTreeMap::new()));

        Ok(Self {
            raft: Arc::new(raft),
            store,
            node_id,
            peers,
            data_cache,
        })
    }

    /// Submit a write request to the Raft cluster
    pub async fn submit_write(&self, request: LithairRequest) -> Result<LithairResponse> {
        let client_req = ClientRequest::new(request);
        
        match self.raft.client_write(client_req).await {
            Ok(response) => {
                match response.data {
                    ClientResponse::Write(data) => Ok(LithairResponse::Success { data: Some(data) }),
                    ClientResponse::Read(data) => Ok(LithairResponse::Success { data: Some(data) }),
                }
            }
            Err(e) => {
                if let Some(forward_to_leader) = e.forward_to_leader() {
                    Ok(LithairResponse::NotLeader { leader_id: Some(forward_to_leader) })
                } else {
                    Ok(LithairResponse::Error { message: e.to_string() })
                }
            }
        }
    }

    /// Check if this node is the leader
    pub async fn is_leader(&self) -> bool {
        let metrics = self.raft.metrics().borrow().clone();
        metrics.current_leader == Some(self.node_id)
    }

    /// Get current leader ID
    pub async fn get_leader_id(&self) -> Option<u64> {
        let metrics = self.raft.metrics().borrow().clone();
        metrics.current_leader
    }

    /// Get all data from cache (for reads)
    pub async fn get_all_data(&self) -> Vec<T> {
        let cache = self.data_cache.read().await;
        cache.values().cloned().collect()
    }

    /// Get specific data by ID
    pub async fn get_data_by_id(&self, id: &str) -> Option<T> {
        let cache = self.data_cache.read().await;
        cache.get(id).cloned()
    }

    /// Update local cache when receiving replicated data
    pub async fn update_local_cache(&self, id: String, data: T) {
        let mut cache = self.data_cache.write().await;
        cache.insert(id, data);
    }

    /// Remove from local cache
    pub async fn remove_from_cache(&self, id: &str) {
        let mut cache = self.data_cache.write().await;
        cache.remove(id);
    }

    /// Get Raft metrics for status reporting
    pub async fn get_metrics(&self) -> RaftMetrics<u64, BasicNode> {
        self.raft.metrics().borrow().clone()
    }
}

/// Network implementation for Raft communication
pub mod network {
    use super::*;
    use openraft::network::RaftNetwork;
    use openraft::raft::{AppendEntriesRequest, AppendEntriesResponse, VoteRequest, VoteResponse};
    use openraft::network::RPCOption;
    use reqwest::Client;
    use std::collections::BTreeSet;

    pub struct LithairNetwork {
        client: Client,
    }

    impl LithairNetwork {
        pub fn new() -> Self {
            Self {
                client: Client::new(),
            }
        }
    }

    #[async_trait::async_trait]
    impl RaftNetwork<LithairTypeConfig> for LithairNetwork {
        async fn append_entries(
            &mut self,
            target: u64,
            req: AppendEntriesRequest<LithairTypeConfig>,
            _option: RPCOption,
        ) -> Result<AppendEntriesResponse<u64>, openraft::network::RPCError<u64, BasicNode, openraft::error::ReplicationError<u64>>> {
            // Convert target node ID to endpoint URL
            let target_port = 8080 + target - 1;
            let url = format!("http://127.0.0.1:{}/raft/append_entries", target_port);
            
            match self.client.post(&url).json(&req).send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        match response.json().await {
                            Ok(resp) => Ok(resp),
                            Err(e) => Err(openraft::network::RPCError::Network(e.into())),
                        }
                    } else {
                        Err(openraft::network::RPCError::Network(anyhow::anyhow!("HTTP error: {}", response.status()).into()))
                    }
                }
                Err(e) => Err(openraft::network::RPCError::Network(e.into())),
            }
        }

        async fn vote(
            &mut self,
            target: u64,
            req: VoteRequest<u64>,
            _option: RPCOption,
        ) -> Result<VoteResponse<u64>, openraft::network::RPCError<u64, BasicNode, openraft::error::VoteError<u64>>> {
            // Convert target node ID to endpoint URL
            let target_port = 8080 + target - 1;
            let url = format!("http://127.0.0.1:{}/raft/vote", target_port);
            
            match self.client.post(&url).json(&req).send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        match response.json().await {
                            Ok(resp) => Ok(resp),
                            Err(e) => Err(openraft::network::RPCError::Network(e.into())),
                        }
                    } else {
                        Err(openraft::network::RPCError::Network(anyhow::anyhow!("HTTP error: {}", response.status()).into()))
                    }
                }
                Err(e) => Err(openraft::network::RPCError::Network(e.into())),
            }
        }
    }
}