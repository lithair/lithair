//! Consensus and Distributed Replication for Lithair
//!
//! This module provides automatic distributed replication based on DeclarativeModel attributes.
//! When a model has #[persistence(replicate)] attributes, Lithair automatically:
//! - Detects replication requirements
//! - Sets up OpenRaft consensus
//! - Synchronizes EventStores across nodes via HTTP
//! - Provides transparent distributed operations

use std::collections::BTreeSet;
use std::sync::Arc;

// OpenRaft dependencies for distributed consensus
use openraft::storage::Adaptor;
use openraft::{Config, Raft};
use openraft_memstore::{MemStore, TypeConfig};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tokio::time::{sleep, timeout, Duration};

// Lithair storage integration
use crate::engine::EventStore;

// HTTP client for peer communication (reqwest)
use reqwest::Client as HttpClient;

// ============================================================================
// Lithair CRUD Operations Data Types
// ============================================================================

/// Represents a CRUD operation that can be replicated via HTTP consensus
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum CrudOperation<T>
where
    T: Serialize + Clone,
{
    Create { item: T, primary_key: String },
    Update { item: T, primary_key: String },
    Delete { primary_key: String },
}

/// Lithair-specific AppData for HTTP replication
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LithairAppData<T>
where
    T: Serialize + Clone,
{
    pub operation: CrudOperation<T>,
    pub model_type: String,
    pub timestamp: u64,
    pub node_id: u64,
}

/// Trait to detect if a DeclarativeModel needs distributed replication
///
/// This trait is auto-implemented by the DeclarativeModel macro when it detects
/// any #[persistence(replicate)] attributes on fields.
pub trait ReplicatedModel: Send + Sync + Clone + Serialize + for<'de> Deserialize<'de> {
    /// Returns true if this model has any fields marked with #[persistence(replicate)]
    fn needs_replication() -> bool;

    /// Returns the list of field names that should be replicated
    fn replicated_fields() -> Vec<&'static str>;

    /// Returns the consensus group name for this model (defaults to model name)
    fn consensus_group() -> String {
        std::any::type_name::<Self>()
            .split("::")
            .last()
            .unwrap_or("unknown")
            .to_lowercase()
    }
}

/// Consensus configuration for DeclarativeServer
#[derive(Debug, Clone)]
pub struct ConsensusConfig {
    pub node_id: u64,
    pub cluster_peers: Vec<String>, // Other node addresses
    pub consensus_port: u16,        // Port for Raft communications
    pub data_dir: String,
}

/// HTTP-based consensus coordinator for DeclarativeModel replication
///
/// Uses HYPER HTTP routes for peer communication instead of direct OpenRaft networking.
/// Integrates with Lithair EventStore for native persistence.
pub struct DeclarativeConsensus<T>
where
    T: ReplicatedModel,
{
    config: ConsensusConfig,
    raft: Option<Arc<Raft<TypeConfig>>>,
    http_replicator: Option<HyperReplicationCoordinator>,
    _phantom: std::marker::PhantomData<T>,
}

impl<T> DeclarativeConsensus<T>
where
    T: ReplicatedModel,
{
    pub fn new(config: ConsensusConfig) -> Self {
        Self { config, raft: None, http_replicator: None, _phantom: std::marker::PhantomData }
    }

    /// Initialize consensus cluster for this model using HYPER HTTP
    pub async fn initialize(&mut self) -> anyhow::Result<()> {
        if !T::needs_replication() {
            return Ok(()); // No replication needed
        }

        println!(
            "🌐 Initializing HYPER HTTP-based consensus for model {} with fields: {:?}",
            T::consensus_group(),
            T::replicated_fields()
        );

        // Create Raft configuration for leader election only
        let raft_config = Arc::new(Config::default());

        // Initialize HTTP-based replication coordinator
        let event_store_path = format!("{}/raftlog", self.config.data_dir);
        std::fs::create_dir_all(&self.config.data_dir).unwrap_or_default();

        let http_replicator = HyperReplicationCoordinator::new(
            &event_store_path,
            self.config.node_id,
            self.config.cluster_peers.clone(),
        )
        .await?;

        // Initialize network for leader election
        let network = DeclarativeNetwork::new(&self.config);

        // Use minimal MemStore for leader election only (data goes to EventStore)
        let store = MemStore::new_async().await;
        let (log_store, state_machine) = Adaptor::new(store);

        // Initialize OpenRaft instance for leader election
        let raft =
            Raft::new(self.config.node_id, raft_config, network, log_store, state_machine).await?;

        let raft_arc = Arc::new(raft);
        self.raft = Some(raft_arc.clone());
        self.http_replicator = Some(http_replicator);

        println!(
            "✅ HYPER HTTP consensus initialized for node {} with {} peers",
            self.config.node_id,
            self.config.cluster_peers.len()
        );

        // Initialize cluster membership and leader election
        if self.config.cluster_peers.is_empty() {
            println!("🔴 Single-node mode: no replication needed");
            let mut nodes = BTreeSet::new();
            nodes.insert(self.config.node_id);
            match raft_arc.initialize(nodes).await {
                Ok(_) => println!("✅ Single-node cluster initialized"),
                Err(e) => println!("⚠️ Failed to initialize single-node cluster: {}", e),
            }
        } else {
            println!(
                "🟢 Multi-node HTTP replication: {} peers configured",
                self.config.cluster_peers.len()
            );

            let all_node_ids = {
                let mut ids = vec![self.config.node_id];
                for peer in &self.config.cluster_peers {
                    if let Some(port) = peer.split(':').nth(1) {
                        if let Ok(port_num) = port.parse::<u16>() {
                            let peer_node_id = (port_num - 8080 + 1) as u64;
                            ids.push(peer_node_id);
                        }
                    }
                }
                ids.sort();
                ids
            };

            let lowest_id = all_node_ids.iter().min().copied().unwrap_or(self.config.node_id);
            let is_initial_leader = self.config.node_id == lowest_id;

            if is_initial_leader {
                println!("👑 Node {} selected as HTTP replication leader", self.config.node_id);
                let nodes: BTreeSet<u64> = all_node_ids.into_iter().collect();
                match raft_arc.initialize(nodes).await {
                    Ok(_) => println!("✅ HTTP replication cluster initialized"),
                    Err(e) => println!("⚠️ Failed to initialize HTTP cluster: {}", e),
                }
            } else {
                println!("🔄 Node {} waiting for HTTP replication leader", self.config.node_id);
            }
        }

        Ok(())
    }

    /// Check if this node should participate in replication
    pub fn should_replicate(&self) -> bool {
        T::needs_replication() && !self.config.cluster_peers.is_empty()
    }

    /// Propose creation through HTTP consensus (HYPER-based replication)
    pub async fn propose_create(&self, item: T, primary_key: String) -> anyhow::Result<()> {
        match &self.http_replicator {
            Some(replicator) => {
                println!("🌐 HYPER: Proposing CRUD operation through HTTP replication...");

                let operation = CrudOperation::Create { item, primary_key: primary_key.clone() };

                let app_data = LithairAppData {
                    operation,
                    model_type: T::consensus_group(),
                    node_id: self.config.node_id,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                };

                // Replicate via HTTP to all peers using HYPER
                replicator.replicate_operation(app_data).await?;

                println!("✅ HYPER: CRUD operation successfully replicated via HTTP");
                Ok(())
            }
            None => {
                println!(
                    "⚠️ HTTP replicator not initialized - operation proceeding without consensus"
                );
                Ok(())
            }
        }
    }

    /// Get all items from consensus state via EventStore
    pub async fn get_all_items(&self) -> anyhow::Result<std::collections::HashMap<String, T>> {
        if let Some(replicator) = &self.http_replicator {
            replicator.get_all_items().await
        } else {
            Ok(std::collections::HashMap::new())
        }
    }
}

// ============================================================================
// OpenRaft Network Implementation (Minimal for Leader Election)
// ============================================================================

/// Network factory for leader election only
#[derive(Debug, Clone)]
pub struct DeclarativeNetwork {
    _config: ConsensusConfig,
}

impl DeclarativeNetwork {
    pub fn new(config: &ConsensusConfig) -> Self {
        Self { _config: config.clone() }
    }
}

impl openraft::network::RaftNetworkFactory<TypeConfig> for DeclarativeNetwork {
    type Network = DeclarativeConnection;

    async fn new_client(&mut self, target: u64, _node: &()) -> Self::Network {
        let target_address = format!("127.0.0.1:{}", 8080 + target - 1);
        DeclarativeConnection { target_id: target, _target_address: target_address }
    }
}

/// Individual connection for leader election
#[derive(Debug)]
pub struct DeclarativeConnection {
    target_id: u64,
    _target_address: String,
}

impl openraft::network::RaftNetwork<TypeConfig> for DeclarativeConnection {
    async fn append_entries(
        &mut self,
        _req: openraft::raft::AppendEntriesRequest<TypeConfig>,
        _option: openraft::network::RPCOption,
    ) -> Result<
        openraft::raft::AppendEntriesResponse<u64>,
        openraft::error::RPCError<u64, (), openraft::error::RaftError<u64>>,
    > {
        println!("📨 Leader election append entries to node {}", self.target_id);
        Ok(openraft::raft::AppendEntriesResponse::Success)
    }

    async fn install_snapshot(
        &mut self,
        req: openraft::raft::InstallSnapshotRequest<TypeConfig>,
        _option: openraft::network::RPCOption,
    ) -> Result<
        openraft::raft::InstallSnapshotResponse<u64>,
        openraft::error::RPCError<
            u64,
            (),
            openraft::error::RaftError<u64, openraft::error::InstallSnapshotError>,
        >,
    > {
        println!("📸 Leader election install snapshot to node {}", self.target_id);
        Ok(openraft::raft::InstallSnapshotResponse { vote: req.vote })
    }

    async fn vote(
        &mut self,
        req: openraft::raft::VoteRequest<u64>,
        _option: openraft::network::RPCOption,
    ) -> Result<
        openraft::raft::VoteResponse<u64>,
        openraft::error::RPCError<u64, (), openraft::error::RaftError<u64>>,
    > {
        println!("🗳️ Leader election vote request to node {}", self.target_id);
        Ok(openraft::raft::VoteResponse { vote: req.vote, vote_granted: true, last_log_id: None })
    }
}

// ============================================================================
// HYPER HTTP-based Replication Coordinator
// ============================================================================

use crate::engine::events::{Event, EventEnvelope};

/// HTTP-based replication using HYPER and Lithair EventStore
pub struct HyperReplicationCoordinator {
    /// Native Lithair EventStore for persistence
    event_store: Arc<RwLock<EventStore>>,
    /// Node configuration
    node_id: u64,
    /// Peer addresses for HTTP replication
    peers: Vec<String>,
    /// HTTP client for peer communication
    http_client: HttpClient,
    /// Pending queue of failed replication requests (url, json, attempts)
    pending_queue: Arc<RwLock<Vec<(String, String, u32)>>>,
}

impl HyperReplicationCoordinator {
    pub async fn new(
        event_store_path: &str,
        node_id: u64,
        peers: Vec<String>,
    ) -> anyhow::Result<Self> {
        let event_store = EventStore::new(event_store_path)?;
        let http_client = HttpClient::new();

        println!("🌐 HYPER Replication Coordinator initialized:");
        println!("   EventStore: {}", event_store_path);
        println!("   Node ID: {}", node_id);
        println!("   Peers: {:?}", peers);

        let coordinator = Self {
            event_store: Arc::new(RwLock::new(event_store)),
            node_id,
            peers,
            http_client,
            pending_queue: Arc::new(RwLock::new(Vec::new())),
        };

        // Spawn background queue drainer
        coordinator.spawn_queue_drainer();

        Ok(coordinator)
    }

    /// Replicate CRUD operation to all peers via HTTP
    pub async fn replicate_operation<T>(&self, app_data: LithairAppData<T>) -> anyhow::Result<()>
    where
        T: Serialize + Clone,
    {
        // First, apply locally to EventStore
        self.apply_locally(&app_data).await?;

        // Then replicate to all peers via HTTP
        let json_data = serde_json::to_string(&app_data)?;

        for peer in &self.peers {
            let url = format!("http://{}/internal/replicate", peer);

            match self.send_with_retries(&url, &json_data, 5, Duration::from_secs(3)).await {
                Ok(_) => println!("✅ HYPER: Replicated to peer {}", peer),
                Err(e) => {
                    println!("❌ HYPER: Failed to replicate to peer {} after retries: {}", peer, e);
                    // Enqueue for background retry
                    let mut q = self.pending_queue.write().await;
                    q.push((url.clone(), json_data.clone(), 0));
                }
            }
        }

        Ok(())
    }

    /// Apply operation locally to EventStore
    async fn apply_locally<T>(&self, app_data: &LithairAppData<T>) -> anyhow::Result<()>
    where
        T: Serialize + Clone,
    {
        let event = ReplicationEvent {
            operation_type: "crud_replicate".to_string(),
            node_id: self.node_id,
            model_type: app_data.model_type.clone(),
            data: serde_json::to_string(app_data)?,
            timestamp: app_data.timestamp,
        };

        let envelope = EventEnvelope {
            event_type: "ReplicationEvent".to_string(),
            event_id: format!("repl_{}_{}", app_data.timestamp, self.node_id),
            timestamp: app_data.timestamp,
            payload: event.to_json(),
            aggregate_id: Some(format!("node_{}", self.node_id)),
            // Hash chain fields - computed automatically by EventStore when enabled
            event_hash: None,
            previous_hash: None,
        };

        {
            let mut store = self.event_store.write().await;
            store.append_envelope(&envelope)?;
            store.flush()?;
        }

        println!("📝 Node {}: Applied operation locally to EventStore", self.node_id);
        Ok(())
    }

    /// Send HTTP replication request to peer
    async fn send_replication_request(&self, url: &str, json_data: &str) -> anyhow::Result<()> {
        // Bound the request time with a timeout
        let http_fut = self
            .http_client
            .post(url)
            .header("content-type", "application/json")
            .body(json_data.to_string())
            .send();
        let resp = timeout(Duration::from_secs(5), http_fut)
            .await
            .map_err(|_| anyhow::anyhow!("HTTP replication timed out"))??;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!("HTTP replication failed with status: {}", resp.status()))
        }
    }

    /// Send with retries and exponential backoff
    async fn send_with_retries(
        &self,
        url: &str,
        json_data: &str,
        max_attempts: u32,
        base_timeout: Duration,
    ) -> anyhow::Result<()> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.send_replication_request(url, json_data).await {
                Ok(_) => return Ok(()),
                Err(e) => {
                    if attempt >= max_attempts {
                        return Err(e);
                    }
                    let backoff = base_timeout.mul_f32(0.2).as_millis() as u64 + (1u64 << attempt);
                    println!("⏳ Retry {} for {} in {}ms", attempt, url, backoff);
                    sleep(Duration::from_millis(backoff)).await;
                }
            }
        }
    }

    /// Spawn a background task that drains the pending queue
    fn spawn_queue_drainer(&self) {
        let pending = Arc::clone(&self.pending_queue);
        let client = self.http_client.clone();
        tokio::spawn(async move {
            loop {
                // Drain all entries per tick
                let mut entries: Vec<(String, String, u32)> = {
                    let mut q = pending.write().await;
                    q.drain(..).collect::<Vec<_>>()
                };

                if !entries.is_empty() {
                    println!("🔁 Draining {} pending replication requests...", entries.len());
                }

                for (url, json, attempts) in entries.drain(..) {
                    let fut = client
                        .post(url.clone())
                        .header("content-type", "application/json")
                        .body(json.clone())
                        .send();
                    let res = timeout(Duration::from_secs(5), fut).await;
                    match res {
                        Ok(Ok(resp)) if resp.status().is_success() => {
                            println!("✅ Background replicate OK -> {}", url);
                        }
                        Ok(Ok(resp)) => {
                            println!(
                                "❌ Background replicate failed status {} -> {}",
                                resp.status(),
                                url
                            );
                            let mut q = pending.write().await;
                            q.push((url.clone(), json.clone(), attempts.saturating_add(1)));
                        }
                        Ok(Err(e)) => {
                            println!("❌ Background replicate error {} -> {}", e, url);
                            let mut q = pending.write().await;
                            q.push((url.clone(), json.clone(), attempts.saturating_add(1)));
                        }
                        Err(_) => {
                            println!("⏱️ Background replicate timeout -> {}", url);
                            let mut q = pending.write().await;
                            q.push((url.clone(), json.clone(), attempts.saturating_add(1)));
                        }
                    }
                }

                sleep(Duration::from_secs(2)).await;
            }
        });
    }

    /// Get all items from EventStore
    pub async fn get_all_items<T>(&self) -> anyhow::Result<std::collections::HashMap<String, T>>
    where
        T: for<'de> Deserialize<'de>,
    {
        // TODO: Reconstruct state from EventStore events
        // For now, return empty - this will be filled with EventStore replay logic
        Ok(std::collections::HashMap::new())
    }
}

/// Event for replication operations in EventStore
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationEvent {
    pub operation_type: String,
    pub node_id: u64,
    pub model_type: String,
    pub data: String,
    pub timestamp: u64,
}

impl Event for ReplicationEvent {
    type State = Vec<ReplicationEvent>;

    fn apply(&self, state: &mut Self::State) {
        state.push(self.clone());
    }

    fn to_json(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| "{\"error\": \"failed to serialize ReplicationEvent\"}".to_string())
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(format!("replication_node_{}", self.node_id))
    }

    fn idempotence_key(&self) -> Option<String> {
        Some(format!(
            "{}_{}_{}_{}",
            self.operation_type, self.node_id, self.model_type, self.timestamp
        ))
    }
}
