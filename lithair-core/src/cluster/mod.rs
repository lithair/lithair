//! Cluster module for Lithair distributed consensus
//!
//! This module provides Raft leadership state management for distributed clusters.
//! Use `LithairServer::with_raft_cluster()` to enable clustering.

use clap::Parser;
use reqwest::Client as HttpClient;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub mod simple_replication;
pub mod consensus_log;
pub mod wal;
pub mod replication_batcher;
pub mod snapshot;

pub use consensus_log::{ConsensusLog, CrudOperation, LogEntry, LogId, ApplyResult};
pub use wal::WriteAheadLog;
pub use replication_batcher::{ReplicationBatcher, BatcherConfig, FollowerHealth, FollowerStats};
pub use snapshot::{SnapshotManager, SnapshotData, SnapshotMeta, InstallSnapshotRequest, InstallSnapshotResponse};

/// Raft Node State for leader election and failover
#[derive(Debug, Clone, PartialEq)]
pub enum RaftNodeState {
    Follower,
    Candidate,
    Leader,
}

/// Raft Leadership State
///
/// Manages the Raft consensus state for a cluster node including:
/// - Leader election
/// - Heartbeat tracking
/// - State transitions (Follower -> Candidate -> Leader)
pub struct RaftLeadershipState {
    pub node_id: u64,
    pub self_port: u16,
    pub current_state: AtomicU64, // 0=Follower, 1=Candidate, 2=Leader
    pub is_leader: AtomicBool,
    pub current_leader_id: AtomicU64,
    pub leader_port: AtomicU16,
    pub peers: Vec<String>,
    pub last_heartbeat: std::sync::Mutex<Instant>,
    pub election_timeout: Duration,
}

impl RaftLeadershipState {
    /// Create a new RaftLeadershipState
    ///
    /// Uses static leader election: lowest node_id becomes leader initially.
    /// Dynamic election occurs when leader fails (heartbeat timeout).
    pub fn new(node_id: u64, self_port: u16, peers: Vec<String>) -> Self {
        // Simple leadership election: lowest node_id is leader
        // This is a static election - node_id 0 is always leader if present
        let is_leader = node_id == 0 || peers.is_empty();

        // Find the leader port
        // If we are leader, it's our port
        // If not, find the smallest port among peers (leader is node_id=0, first allocated)
        let leader_port = if is_leader {
            self_port
        } else {
            // Find the smallest port among peers (that's the leader)
            peers
                .iter()
                .filter_map(|peer| peer.split(':').nth(1))
                .filter_map(|port_str| port_str.parse::<u16>().ok())
                .min()
                .unwrap_or(self_port)
        };

        // Find the leader node_id (always 0 in static election)
        let current_leader_id = 0u64;

        Self {
            node_id,
            self_port,
            current_state: AtomicU64::new(if is_leader { 2 } else { 0 }), // 2=Leader, 0=Follower
            is_leader: AtomicBool::new(is_leader),
            current_leader_id: AtomicU64::new(current_leader_id),
            leader_port: AtomicU16::new(leader_port),
            peers,
            last_heartbeat: std::sync::Mutex::new(Instant::now()),
            election_timeout: Duration::from_secs(5), // 5 second timeout
        }
    }

    /// Check if this node is currently the leader
    pub fn is_leader(&self) -> bool {
        self.is_leader.load(Ordering::Relaxed)
    }

    /// Get the current Raft state (Follower, Candidate, or Leader)
    pub fn get_current_state(&self) -> RaftNodeState {
        match self.current_state.load(Ordering::Relaxed) {
            0 => RaftNodeState::Follower,
            1 => RaftNodeState::Candidate,
            2 => RaftNodeState::Leader,
            _ => RaftNodeState::Follower,
        }
    }

    /// Get the current leader's port
    pub fn get_leader_port(&self) -> u16 {
        self.leader_port.load(Ordering::Relaxed)
    }

    /// Check if the provided id matches the current authoritative leader id
    #[allow(dead_code)]
    pub(crate) fn is_authoritative_leader_id(&self, leader_id: u64) -> bool {
        self.current_leader_id.load(Ordering::Relaxed) == leader_id
    }

    /// Update the last heartbeat timestamp (called when receiving heartbeat from leader)
    pub fn update_heartbeat(&self) {
        if let Ok(mut heartbeat) = self.last_heartbeat.lock() {
            *heartbeat = Instant::now();
        }
    }

    /// Check if election should be started (heartbeat timeout exceeded)
    pub fn should_start_election(&self) -> bool {
        if self.is_leader() {
            return false;
        }

        if let Ok(heartbeat) = self.last_heartbeat.lock() {
            heartbeat.elapsed() > self.election_timeout
        } else {
            false
        }
    }

    /// Become the leader - called when this node wins an election
    pub fn become_leader(&self) {
        self.is_leader.store(true, Ordering::SeqCst);
        self.current_state.store(2, Ordering::SeqCst); // 2 = Leader
        self.current_leader_id.store(self.node_id, Ordering::SeqCst);
        self.leader_port.store(self.self_port, Ordering::SeqCst);
        self.update_heartbeat();
        println!("👑 Node {} is now the LEADER", self.node_id);
    }

    /// Become a follower - called when a new leader is detected
    pub fn become_follower(&self, new_leader_id: u64, new_leader_port: u16) {
        self.is_leader.store(false, Ordering::SeqCst);
        self.current_state.store(0, Ordering::SeqCst); // 0 = Follower
        self.current_leader_id.store(new_leader_id, Ordering::SeqCst);
        self.leader_port.store(new_leader_port, Ordering::SeqCst);
        self.update_heartbeat();
        println!(
            "👥 Node {} is now a FOLLOWER (leader: node {} on port {})",
            self.node_id, new_leader_id, new_leader_port
        );
    }

    /// Start election process - find the lowest available node_id to be leader
    ///
    /// Returns (should_become_leader, new_leader_id, new_leader_port)
    pub async fn start_election(&self) -> (bool, u64, u16) {
        println!("🗳️ Node {} starting election...", self.node_id);
        self.current_state.store(1, Ordering::SeqCst); // 1 = Candidate

        let client = HttpClient::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap_or_else(|_| HttpClient::new());

        // Check which peers are alive
        let mut alive_peers: Vec<(u64, u16)> = Vec::new();

        for peer in &self.peers {
            let url = format!("http://{}/status", peer);
            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(status) = resp.json::<serde_json::Value>().await {
                        if let Some(raft) = status.get("raft") {
                            let peer_id = raft
                                .get("node_id")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(u64::MAX);
                            let peer_port = peer
                                .split(':')
                                .nth(1)
                                .and_then(|p| p.parse::<u16>().ok())
                                .unwrap_or(0);
                            alive_peers.push((peer_id, peer_port));
                            println!("   ✅ Peer {} (node {}) is alive", peer, peer_id);
                        }
                    }
                }
                _ => {
                    println!("   ❌ Peer {} is not responding", peer);
                }
            }
        }

        // Find the lowest node_id among alive nodes (including self)
        let mut candidates: Vec<(u64, u16)> = alive_peers;
        candidates.push((self.node_id, self.self_port));
        candidates.sort_by_key(|(id, _)| *id);

        let (winner_id, winner_port) = candidates[0];
        let should_become_leader = winner_id == self.node_id;

        println!(
            "🗳️ Election result: node {} wins (port {})",
            winner_id, winner_port
        );

        (should_become_leader, winner_id, winner_port)
    }

    /// Get time since last heartbeat
    pub fn time_since_heartbeat(&self) -> Duration {
        if let Ok(heartbeat) = self.last_heartbeat.lock() {
            heartbeat.elapsed()
        } else {
            Duration::from_secs(0)
        }
    }
}

/// Standard command line arguments for Lithair cluster applications
///
/// Use with clap to parse cluster node configuration from CLI.
///
/// # Example
/// ```rust,ignore
/// use clap::Parser;
/// use lithair_core::cluster::ClusterArgs;
///
/// let args = ClusterArgs::parse();
/// let peers: Vec<String> = args.peers
///     .unwrap_or_default()
///     .iter()
///     .map(|p| format!("127.0.0.1:{}", p))
///     .collect();
///
/// LithairServer::new()
///     .with_port(args.port)
///     .with_raft_cluster(args.node_id, peers)
///     .build()?
///     .serve()
///     .await?;
/// ```
#[derive(Parser, Debug)]
#[command(name = "lithair-cluster")]
#[command(about = "Lithair Cluster Node")]
pub struct ClusterArgs {
    /// Unique node identifier (0 = initial leader)
    #[arg(long)]
    pub node_id: u64,

    /// Port to listen on
    #[arg(long)]
    pub port: u16,

    /// Comma-separated list of peer ports
    #[arg(long, value_delimiter = ',')]
    pub peers: Option<Vec<u16>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raft_state_leader_election() {
        // Node 0 should be leader
        let state = RaftLeadershipState::new(0, 8080, vec!["127.0.0.1:8081".to_string()]);
        assert!(state.is_leader());
        assert_eq!(state.get_current_state(), RaftNodeState::Leader);
        assert_eq!(state.get_leader_port(), 8080);
    }

    #[test]
    fn test_raft_state_follower() {
        // Node 1 should be follower
        let state = RaftLeadershipState::new(1, 8081, vec!["127.0.0.1:8080".to_string()]);
        assert!(!state.is_leader());
        assert_eq!(state.get_current_state(), RaftNodeState::Follower);
        assert_eq!(state.get_leader_port(), 8080);
    }

    #[test]
    fn test_raft_state_single_node() {
        // Single node should be leader
        let state = RaftLeadershipState::new(5, 9000, vec![]);
        assert!(state.is_leader());
        assert_eq!(state.get_leader_port(), 9000);
    }

    #[test]
    fn test_become_leader() {
        let state = RaftLeadershipState::new(1, 8081, vec!["127.0.0.1:8080".to_string()]);
        assert!(!state.is_leader());

        state.become_leader();
        assert!(state.is_leader());
        assert_eq!(state.get_current_state(), RaftNodeState::Leader);
        assert_eq!(state.get_leader_port(), 8081);
    }

    #[test]
    fn test_become_follower() {
        let state = RaftLeadershipState::new(0, 8080, vec!["127.0.0.1:8081".to_string()]);
        assert!(state.is_leader());

        state.become_follower(1, 8081);
        assert!(!state.is_leader());
        assert_eq!(state.get_current_state(), RaftNodeState::Follower);
        assert_eq!(state.get_leader_port(), 8081);
    }

    #[test]
    fn test_heartbeat_timeout() {
        let state = RaftLeadershipState::new(1, 8081, vec!["127.0.0.1:8080".to_string()]);

        // Fresh state should not trigger election
        assert!(!state.should_start_election());

        // Update heartbeat
        state.update_heartbeat();
        assert!(!state.should_start_election());
    }
}
