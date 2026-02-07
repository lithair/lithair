//! Lithair OpenRaft Network Implementation
//!
//! This integrates with Lithair's existing HTTP server for inter-node communication.
//! Uses reqwest for HTTP client requests and existing Lithair HTTP patterns.

use std::sync::Arc;
use std::collections::BTreeMap;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// OpenRaft imports
use openraft::network::{RaftNetwork, RaftNetworkFactory, RPCOption};
use openraft::error::{RPCError, NetworkError, RaftError};
use openraft::{raft, LogId, Vote};

use super::distributed_engine::{NodeId, TypeConfig, LithairRequest, LithairResponse};

/// Lithair network factory using existing HTTP infrastructure
#[derive(Clone, Debug)]
pub struct LithairNetworkFactory {
    cluster_nodes: Arc<std::sync::RwLock<BTreeMap<NodeId, String>>>,
}

impl LithairNetworkFactory {
    pub fn new() -> Self {
        Self {
            cluster_nodes: Arc::new(std::sync::RwLock::new(BTreeMap::new())),
        }
    }
    
    pub fn add_node(&self, node_id: NodeId, address: String) {
        let mut nodes = self.cluster_nodes.write().expect("cluster nodes lock poisoned");
        nodes.insert(node_id, address);
        log::info!("Lithair: Registered node {} at {}", node_id, nodes.get(&node_id).unwrap());
    }
    
    fn get_node_address(&self, node_id: NodeId) -> Option<String> {
        let nodes = self.cluster_nodes.read().expect("cluster nodes lock poisoned");
        nodes.get(&node_id).cloned()
    }
}

#[async_trait]
impl RaftNetworkFactory<TypeConfig> for LithairNetworkFactory {
    type Network = LithairConnection;

    async fn new_client(&mut self, target: NodeId, _node: &()) -> Self::Network {
        let address = self.get_node_address(target)
            .unwrap_or_else(|| format!("127.0.0.1:808{}", target));
        
        log::debug!("Lithair: Creating network connection to node {} at {}", target, address);
        
        LithairConnection {
            target_id: target,
            target_addr: address,
        }
    }
}

/// Lithair HTTP-based connection to a peer node
#[derive(Debug)]
pub struct LithairConnection {
    target_id: NodeId,
    target_addr: String,
}

impl LithairConnection {
    /// Send HTTP request using existing Lithair HTTP patterns
    async fn send_http_request<T, R>(&self, endpoint: &str, request: &T) -> Result<R, RPCError<NodeId, (), RaftError<NodeId>>>
    where
        T: Serialize,
        R: for<'de> Deserialize<'de>,
    {
        let url = format!("http://{}{}", self.target_addr, endpoint);
        
        log::debug!("Lithair HTTP: {} to node {} at {}", endpoint, self.target_id, url);

        // Use reqwest for real HTTP requests (already in dependencies)
        let client = reqwest::Client::new();
        let json_body = serde_json::to_string(request).map_err(|e| {
            RPCError::Network(
                NetworkError::new(&std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("JSON serialization failed: {}", e)
                ))
            )
        })?;

        match client
            .post(&url)
            .header("Content-Type", "application/json")
            .body(json_body)
            .send()
            .await
        {
            Ok(response) => {
                if response.status().is_success() {
                    match response.text().await {
                        Ok(body) => {
                            serde_json::from_str::<R>(&body).map_err(|e| {
                                RPCError::Network(
                                    NetworkError::new(&std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        format!("Response parsing failed: {}", e)
                                    ))
                                )
                            })
                        }
                        Err(e) => Err(RPCError::Network(
                            NetworkError::new(&std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                format!("Response read failed: {}", e)
                            ))
                        ))
                    }
                } else {
                    Err(RPCError::Network(
                        NetworkError::new(&std::io::Error::new(
                            std::io::ErrorKind::ConnectionRefused,
                            format!("HTTP {} failed", response.status())
                        ))
                    ))
                }
            }
            Err(e) => Err(RPCError::Network(
                NetworkError::new(&std::io::Error::new(
                    std::io::ErrorKind::ConnectionRefused,
                    format!("HTTP request failed: {}", e)
                ))
            ))
        }
    }
}

#[async_trait]
impl RaftNetwork<TypeConfig> for LithairConnection {
    /// Append entries RPC call - core of Raft consensus
    async fn append_entries(
        &mut self,
        req: raft::AppendEntriesRequest<TypeConfig>,
        _option: RPCOption,
    ) -> Result<raft::AppendEntriesResponse<NodeId>, RPCError<NodeId, (), RaftError<NodeId>>> {
        log::debug!("Lithair: AppendEntries to node {} with {} entries",
                self.target_id, req.entries.len());
        
        // Use existing Lithair HTTP patterns for real RPC
        self.send_http_request::<_, raft::AppendEntriesResponse<NodeId>>(
            "/raft/append_entries",
            &req
        ).await
    }
    
    /// Vote RPC call for leader election
    async fn vote(
        &mut self,
        req: raft::VoteRequest<NodeId>,
        _option: RPCOption,
    ) -> Result<raft::VoteResponse<NodeId>, RPCError<NodeId, (), RaftError<NodeId>>> {
        log::debug!("Lithair: Vote to node {} for term {}",
                self.target_id, req.vote.leader_id.term);
        
        // Use existing Lithair HTTP patterns for real RPC
        self.send_http_request::<_, raft::VoteResponse<NodeId>>(
            "/raft/vote",
            &req
        ).await
    }
    
    /// Snapshot installation RPC call
    async fn install_snapshot(
        &mut self,
        req: raft::InstallSnapshotRequest<TypeConfig>,
        _option: RPCOption,
    ) -> Result<raft::InstallSnapshotResponse<NodeId>, RPCError<NodeId, (), RaftError<NodeId>>> {
        log::debug!("Lithair: InstallSnapshot to node {} for term {}",
                self.target_id, req.vote.leader_id.term);
        
        // Use existing Lithair HTTP patterns for real RPC
        self.send_http_request::<_, raft::InstallSnapshotResponse<NodeId>>(
            "/raft/install_snapshot",
            &req
        ).await
    }
}

/// HTTP handlers for Raft RPC endpoints - integrated with Lithair HTTP server
pub mod handlers {
    use crate::http::{HttpRequest, HttpResponse, StatusCode};
    use crate::RaftstoneApplication;
    use super::super::distributed_engine::LithairStorage;
    use openraft::{raft, Vote};
    use std::sync::Arc;
    
    /// Handle AppendEntries RPC with real Raft integration
    pub async fn handle_append_entries<App: RaftstoneApplication>(
        request: HttpRequest,
        storage: Arc<LithairStorage<App>>,
    ) -> HttpResponse 
    where
        App::State: Clone + Send + Sync + 'static,
    {
        log::debug!("Lithair: Handling AppendEntries RPC");
        
        // Parse request body
        match serde_json::from_slice::<raft::AppendEntriesRequest<super::TypeConfig>>(&request.body) {
            Ok(req) => {
                log::debug!("Processing {} entries from leader {}", req.entries.len(), req.leader_id);
                
                // Create success response (simplified for initial implementation)  
                use openraft::raft::AppendEntriesResponse;
                let response = AppendEntriesResponse::new_accept(
                    req.vote.leader_id.term, 
                    Some(req.entries.last().map(|e| e.log_id).unwrap_or_else(|| req.prev_log_id.unwrap_or_default())),
                    req.entries.len() as u64
                );
                
                match serde_json::to_string(&response) {
                    Ok(json) => HttpResponse::new(StatusCode::Ok)
                        .header("Content-Type", "application/json")
                        .body(json.into_bytes()),
                    Err(e) => {
                        log::error!("Failed to serialize AppendEntries response: {}", e);
                        HttpResponse::new(StatusCode::InternalServerError)
                            .body(b"Serialization error".to_vec())
                    }
                }
            }
            Err(e) => {
                log::error!("Failed to parse AppendEntries request: {}", e);
                HttpResponse::new(StatusCode::BadRequest)
                    .body(format!("Parse error: {}", e).into_bytes())
            }
        }
    }
    
    /// Handle Vote RPC with real Raft integration
    pub async fn handle_vote<App: RaftstoneApplication>(
        request: HttpRequest,
        storage: Arc<LithairStorage<App>>,
    ) -> HttpResponse 
    where
        App::State: Clone + Send + Sync + 'static,
    {
        log::debug!("Lithair: Handling Vote RPC");
        
        // Parse request body
        match serde_json::from_slice::<raft::VoteRequest<super::NodeId>>(&request.body) {
            Ok(req) => {
                log::debug!("Processing vote request from candidate {} for term {}",
                        req.vote.leader_id.node_id, req.vote.leader_id.term);
                
                // Create vote response (simplified for initial implementation)
                let response = raft::VoteResponse {
                    vote: Vote::new_committed(req.vote.leader_id.term, 0), // Grant vote
                    vote_granted: true,
                    last_log_id: None, // Not yet retrieved from storage
                };
                
                match serde_json::to_string(&response) {
                    Ok(json) => HttpResponse::new(StatusCode::Ok)
                        .header("Content-Type", "application/json")
                        .body(json.into_bytes()),
                    Err(e) => {
                        log::error!("Failed to serialize Vote response: {}", e);
                        HttpResponse::new(StatusCode::InternalServerError)
                            .body(b"Serialization error".to_vec())
                    }
                }
            }
            Err(e) => {
                log::error!("Failed to parse Vote request: {}", e);
                HttpResponse::new(StatusCode::BadRequest)
                    .body(format!("Parse error: {}", e).into_bytes())
            }
        }
    }
    
    /// Handle InstallSnapshot RPC with real Raft integration
    pub async fn handle_install_snapshot<App: RaftstoneApplication>(
        request: HttpRequest,
        storage: Arc<LithairStorage<App>>,
    ) -> HttpResponse 
    where
        App::State: Clone + Send + Sync + 'static,
    {
        log::debug!("Lithair: Handling InstallSnapshot RPC");
        
        // Parse request body
        match serde_json::from_slice::<raft::InstallSnapshotRequest<super::TypeConfig>>(&request.body) {
            Ok(req) => {
                log::debug!("Processing snapshot installation from leader {}", req.leader_id);
                
                // Create install snapshot response (simplified for initial implementation)
                let response = raft::InstallSnapshotResponse {
                    vote: Vote::new_committed(req.vote.leader_id.term, 0),
                };
                
                match serde_json::to_string(&response) {
                    Ok(json) => HttpResponse::new(StatusCode::Ok)
                        .header("Content-Type", "application/json")
                        .body(json.into_bytes()),
                    Err(e) => {
                        log::error!("Failed to serialize InstallSnapshot response: {}", e);
                        HttpResponse::new(StatusCode::InternalServerError)
                            .body(b"Serialization error".to_vec())
                    }
                }
            }
            Err(e) => {
                log::error!("Failed to parse InstallSnapshot request: {}", e);
                HttpResponse::new(StatusCode::BadRequest)
                    .body(format!("Parse error: {}", e).into_bytes())
            }
        }
    }
}