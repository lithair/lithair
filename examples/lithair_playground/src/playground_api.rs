//! Playground API handlers
//!
//! Provides endpoints for cluster control, benchmarking, and status.

use anyhow::Result;
use http::Request;
use http_body_util::BodyExt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::benchmark::{BenchmarkConfig, BenchmarkEngine, BenchmarkStatus};
use crate::sse_events::SseEventBroadcaster;

/// Playground state shared across handlers
pub struct PlaygroundState {
    pub node_id: u64,
    pub port: u16,
    pub peer_ports: Vec<u16>,
    pub event_broadcaster: Arc<SseEventBroadcaster>,
    pub benchmark_engine: Arc<RwLock<Option<BenchmarkEngine>>>,
    pub last_benchmark_status: Arc<RwLock<Option<BenchmarkStatus>>>,
}

impl PlaygroundState {
    pub fn new(
        node_id: u64,
        port: u16,
        peer_ports: Vec<u16>,
        event_broadcaster: Arc<SseEventBroadcaster>,
    ) -> Self {
        Self {
            node_id,
            port,
            peer_ports,
            event_broadcaster,
            benchmark_engine: Arc::new(RwLock::new(None)),
            last_benchmark_status: Arc::new(RwLock::new(None)),
        }
    }

    /// Get comprehensive cluster status by querying all nodes
    pub async fn get_cluster_status(&self) -> Result<serde_json::Value> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()?;

        // Query local node health
        let local_health = self.get_local_health().await;

        // Query peer nodes
        let mut peer_statuses = Vec::new();
        for peer_port in &self.peer_ports {
            let url = format!("http://127.0.0.1:{}/_raft/health", peer_port);
            let status = match client.get(&url).send().await {
                Ok(resp) => {
                    if resp.status().is_success() {
                        match resp.json::<serde_json::Value>().await {
                            Ok(health) => PeerStatus {
                                port: *peer_port,
                                reachable: true,
                                health: Some(health),
                                error: None,
                            },
                            Err(e) => PeerStatus {
                                port: *peer_port,
                                reachable: true,
                                health: None,
                                error: Some(format!("Parse error: {}", e)),
                            },
                        }
                    } else {
                        PeerStatus {
                            port: *peer_port,
                            reachable: true,
                            health: None,
                            error: Some(format!("HTTP {}", resp.status())),
                        }
                    }
                }
                Err(e) => PeerStatus {
                    port: *peer_port,
                    reachable: false,
                    health: None,
                    error: Some(format!("Unreachable: {}", e)),
                },
            };
            peer_statuses.push(status);
        }

        // Find leader
        let leader_port = if local_health.get("is_leader").and_then(|v| v.as_bool()).unwrap_or(false) {
            Some(self.port)
        } else {
            peer_statuses.iter()
                .find(|p| p.health.as_ref()
                    .and_then(|h| h.get("is_leader"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false))
                .map(|p| p.port)
        };

        Ok(serde_json::json!({
            "node_id": self.node_id,
            "port": self.port,
            "local_health": local_health,
            "peers": peer_statuses,
            "leader_port": leader_port,
            "cluster_size": 1 + self.peer_ports.len(),
            "healthy_nodes": 1 + peer_statuses.iter().filter(|p| p.reachable).count(),
        }))
    }

    /// Get local node health (from /_raft/health)
    async fn get_local_health(&self) -> serde_json::Value {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(1))
            .build()
            .ok();

        if let Some(client) = client {
            let url = format!("http://127.0.0.1:{}/_raft/health", self.port);
            if let Ok(resp) = client.get(&url).send().await {
                if let Ok(health) = resp.json::<serde_json::Value>().await {
                    return health;
                }
            }
        }

        serde_json::json!({
            "error": "Could not get local health"
        })
    }

    /// Start a benchmark
    pub async fn start_benchmark(&self, mut req: Request<hyper::body::Incoming>) -> Result<serde_json::Value> {
        // Parse config from request body
        let body = req.body_mut().collect().await?.to_bytes();
        let config: BenchmarkConfig = if body.is_empty() {
            BenchmarkConfig::default()
        } else {
            serde_json::from_slice(&body)?
        };

        // Check if benchmark already running
        {
            let engine = self.benchmark_engine.read().await;
            if engine.is_some() {
                return Ok(serde_json::json!({
                    "error": "Benchmark already running",
                    "status": "running"
                }));
            }
        }

        // Create and start benchmark
        let engine = BenchmarkEngine::new(
            config.clone(),
            self.port,
            self.event_broadcaster.clone(),
        );

        let engine_clone = engine.clone();
        let status_storage = self.last_benchmark_status.clone();
        let engine_storage = self.benchmark_engine.clone();

        // Store engine
        {
            let mut guard = self.benchmark_engine.write().await;
            *guard = Some(engine);
        }

        // Run benchmark in background
        tokio::spawn(async move {
            let result = engine_clone.run().await;

            // Store final status
            {
                let mut guard = status_storage.write().await;
                *guard = Some(result);
            }

            // Clear engine
            {
                let mut guard = engine_storage.write().await;
                *guard = None;
            }
        });

        Ok(serde_json::json!({
            "started": true,
            "config": config,
            "message": "Benchmark started"
        }))
    }

    /// Get current benchmark status
    pub async fn get_benchmark_status(&self) -> serde_json::Value {
        // Check if running
        let engine = self.benchmark_engine.read().await;
        if let Some(ref eng) = *engine {
            return serde_json::json!({
                "running": true,
                "progress": eng.get_progress().await,
            });
        }
        drop(engine);

        // Check last result
        let last = self.last_benchmark_status.read().await;
        if let Some(ref status) = *last {
            return serde_json::json!({
                "running": false,
                "last_result": status,
            });
        }

        serde_json::json!({
            "running": false,
            "message": "No benchmark has been run yet"
        })
    }

    /// Stop running benchmark
    pub async fn stop_benchmark(&self) {
        let engine = self.benchmark_engine.read().await;
        if let Some(ref eng) = *engine {
            eng.stop().await;
        }
    }

    /// Check data consistency across all cluster nodes
    pub async fn check_consistency(&self) -> Result<serde_json::Value> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()?;

        let all_ports: Vec<u16> = std::iter::once(self.port)
            .chain(self.peer_ports.iter().copied())
            .collect();

        // Endpoints to check
        let endpoints = [
            ("/api/items", "items"),
            ("/api/orders", "orders"),
            ("/api/logs", "logs"),
        ];

        let mut results = Vec::new();
        let mut node_data: std::collections::HashMap<String, Vec<(u16, usize, String)>> =
            std::collections::HashMap::new();

        for (endpoint, name) in &endpoints {
            node_data.insert(name.to_string(), Vec::new());

            for port in &all_ports {
                let url = format!("http://127.0.0.1:{}{}", port, endpoint);
                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        if let Ok(data) = resp.json::<serde_json::Value>().await {
                            let count = data.as_array().map(|a| a.len()).unwrap_or(0);
                            // Create a hash of the data for comparison
                            let hash = format!("{:x}", md5_hash(&data.to_string()));
                            node_data.get_mut(*name).unwrap().push((*port, count, hash));
                        }
                    }
                    Ok(resp) => {
                        node_data.get_mut(*name).unwrap().push((*port, 0, format!("HTTP {}", resp.status())));
                    }
                    Err(e) => {
                        node_data.get_mut(*name).unwrap().push((*port, 0, format!("Error: {}", e)));
                    }
                }
            }
        }

        // Analyze consistency
        for (name, data) in &node_data {
            let hashes: Vec<&String> = data.iter().map(|(_, _, h)| h).collect();
            let counts: Vec<usize> = data.iter().map(|(_, c, _)| *c).collect();

            let first_hash = hashes.first().map(|h| h.as_str()).unwrap_or("");
            let all_same_hash = hashes.iter().all(|h| h.as_str() == first_hash);
            let first_count = counts.first().copied().unwrap_or(0);
            let all_same_count = counts.iter().all(|c| *c == first_count);

            let node_details: Vec<serde_json::Value> = data.iter()
                .map(|(port, count, hash)| serde_json::json!({
                    "port": port,
                    "count": count,
                    "hash": &hash[..8.min(hash.len())] // First 8 chars of hash
                }))
                .collect();

            results.push(serde_json::json!({
                "table": name,
                "consistent": all_same_hash && all_same_count,
                "total_count": first_count,
                "nodes": node_details,
            }));
        }

        let all_consistent = results.iter()
            .all(|r| r.get("consistent").and_then(|v| v.as_bool()).unwrap_or(false));

        Ok(serde_json::json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "cluster_size": all_ports.len(),
            "all_consistent": all_consistent,
            "tables": results,
        }))
    }
}

/// Simple hash function for data comparison
fn md5_hash(data: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    hasher.finish()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PeerStatus {
    port: u16,
    reachable: bool,
    health: Option<serde_json::Value>,
    error: Option<String>,
}

// ============================================================================
// Migration Testing API
// ============================================================================

/// Request to begin a migration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationBeginRequest {
    pub from_version: Option<String>,
    pub to_version: String,
    #[serde(default)]
    pub description: String,
}

/// Request to apply a migration step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationStepRequest {
    pub migration_id: String,
    pub step_type: String,  // "add_model", "add_field", "remove_field", "rename_field", etc.
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub field: Option<String>,
    #[serde(default)]
    pub new_name: Option<String>,
    #[serde(default)]
    pub field_type: Option<String>,
    #[serde(default)]
    pub schema: Option<serde_json::Value>,
}

/// Request to commit or rollback a migration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationFinalizeRequest {
    pub migration_id: String,
    #[serde(default)]
    pub reason: Option<String>,
}

impl PlaygroundState {
    /// Begin a new migration through the leader
    pub async fn migration_begin(&self, mut req: Request<hyper::body::Incoming>) -> Result<serde_json::Value> {
        let body = req.body_mut().collect().await?.to_bytes();
        let request: MigrationBeginRequest = serde_json::from_slice(&body)?;

        // Find leader and send migration begin
        let leader_port = self.find_leader_port().await?;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;

        // Create version structs
        let from_version = request.from_version.unwrap_or_else(|| "0.0.0".to_string());
        let migration_id = uuid::Uuid::new_v4();

        let operation = serde_json::json!({
            "MigrationBegin": {
                "from_version": {
                    "major": parse_version_part(&from_version, 0),
                    "minor": parse_version_part(&from_version, 1),
                    "patch": parse_version_part(&from_version, 2),
                    "schema_hash": "",
                    "build_id": ""
                },
                "to_version": {
                    "major": parse_version_part(&request.to_version, 0),
                    "minor": parse_version_part(&request.to_version, 1),
                    "patch": parse_version_part(&request.to_version, 2),
                    "schema_hash": format!("migration_{}", migration_id),
                    "build_id": request.description.clone()
                },
                "migration_id": migration_id.to_string()
            }
        });

        let url = format!("http://127.0.0.1:{}/_raft/migrate", leader_port);
        let resp = client.post(&url)
            .json(&operation)
            .send()
            .await?;

        if resp.status().is_success() {
            let result = resp.json::<serde_json::Value>().await?;
            Ok(serde_json::json!({
                "success": true,
                "migration_id": migration_id.to_string(),
                "from_version": from_version,
                "to_version": request.to_version,
                "leader_port": leader_port,
                "result": result
            }))
        } else {
            Ok(serde_json::json!({
                "success": false,
                "error": format!("HTTP {}", resp.status()),
                "leader_port": leader_port
            }))
        }
    }

    /// Apply a migration step through the leader
    pub async fn migration_step(&self, mut req: Request<hyper::body::Incoming>) -> Result<serde_json::Value> {
        let body = req.body_mut().collect().await?.to_bytes();
        let request: MigrationStepRequest = serde_json::from_slice(&body)?;

        let leader_port = self.find_leader_port().await?;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;

        // Build schema change based on step_type
        let schema_change = match request.step_type.as_str() {
            "add_model" => serde_json::json!({
                "type": "AddModel",
                "name": request.model.unwrap_or_default(),
                "schema": request.schema.unwrap_or(serde_json::json!({
                    "fields": [],
                    "primary_key": null
                }))
            }),
            "remove_model" => serde_json::json!({
                "type": "RemoveModel",
                "name": request.model.unwrap_or_default(),
                "backup_path": null
            }),
            "add_field" => serde_json::json!({
                "type": "AddField",
                "model": request.model.unwrap_or_default(),
                "field": {
                    "name": request.field.unwrap_or_default(),
                    "field_type": request.field_type.unwrap_or_else(|| "String".to_string()),
                    "nullable": true,
                    "indexed": false,
                    "unique": false
                },
                "default_value": null
            }),
            "remove_field" => serde_json::json!({
                "type": "RemoveField",
                "model": request.model.unwrap_or_default(),
                "field": request.field.unwrap_or_default()
            }),
            "rename_field" => serde_json::json!({
                "type": "RenameField",
                "model": request.model.unwrap_or_default(),
                "old_name": request.field.unwrap_or_default(),
                "new_name": request.new_name.unwrap_or_default()
            }),
            _ => serde_json::json!({
                "type": "Custom",
                "description": request.step_type,
                "forward": "noop",
                "backward": "noop"
            }),
        };

        // Get current step index (simplified - in real impl would query migration state)
        let step_index = 0u32;

        let operation = serde_json::json!({
            "MigrationStep": {
                "migration_id": request.migration_id,
                "step_index": step_index,
                "operation": schema_change
            }
        });

        let url = format!("http://127.0.0.1:{}/_raft/migrate", leader_port);
        let resp = client.post(&url)
            .json(&operation)
            .send()
            .await?;

        if resp.status().is_success() {
            let result = resp.json::<serde_json::Value>().await?;
            Ok(serde_json::json!({
                "success": true,
                "migration_id": request.migration_id,
                "step_type": request.step_type,
                "step_index": step_index,
                "leader_port": leader_port,
                "result": result
            }))
        } else {
            Ok(serde_json::json!({
                "success": false,
                "error": format!("HTTP {}", resp.status()),
                "leader_port": leader_port
            }))
        }
    }

    /// Commit a migration through the leader
    pub async fn migration_commit(&self, mut req: Request<hyper::body::Incoming>) -> Result<serde_json::Value> {
        let body = req.body_mut().collect().await?.to_bytes();
        let request: MigrationFinalizeRequest = serde_json::from_slice(&body)?;

        let leader_port = self.find_leader_port().await?;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;

        // Generate checksum (simplified - in real impl would compute from all steps)
        let checksum = format!("commit_{}", chrono::Utc::now().timestamp());

        let operation = serde_json::json!({
            "MigrationCommit": {
                "migration_id": request.migration_id,
                "checksum": checksum.clone()
            }
        });

        let url = format!("http://127.0.0.1:{}/_raft/migrate", leader_port);
        let resp = client.post(&url)
            .json(&operation)
            .send()
            .await?;

        if resp.status().is_success() {
            let result = resp.json::<serde_json::Value>().await?;
            Ok(serde_json::json!({
                "success": true,
                "migration_id": request.migration_id,
                "checksum": checksum,
                "leader_port": leader_port,
                "result": result
            }))
        } else {
            Ok(serde_json::json!({
                "success": false,
                "error": format!("HTTP {}", resp.status()),
                "leader_port": leader_port
            }))
        }
    }

    /// Rollback a migration through the leader
    pub async fn migration_rollback(&self, mut req: Request<hyper::body::Incoming>) -> Result<serde_json::Value> {
        let body = req.body_mut().collect().await?.to_bytes();
        let request: MigrationFinalizeRequest = serde_json::from_slice(&body)?;

        let leader_port = self.find_leader_port().await?;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;

        let operation = serde_json::json!({
            "MigrationRollback": {
                "migration_id": request.migration_id,
                "failed_step": 0u32,
                "reason": request.reason.clone().unwrap_or_else(|| "Manual rollback".to_string())
            }
        });

        let url = format!("http://127.0.0.1:{}/_raft/migrate", leader_port);
        let resp = client.post(&url)
            .json(&operation)
            .send()
            .await?;

        if resp.status().is_success() {
            let result = resp.json::<serde_json::Value>().await?;
            Ok(serde_json::json!({
                "success": true,
                "migration_id": request.migration_id,
                "reason": request.reason,
                "leader_port": leader_port,
                "result": result
            }))
        } else {
            Ok(serde_json::json!({
                "success": false,
                "error": format!("HTTP {}", resp.status()),
                "leader_port": leader_port
            }))
        }
    }

    /// Get migration status from the cluster
    pub async fn migration_status(&self) -> Result<serde_json::Value> {
        let leader_port = self.find_leader_port().await.ok();

        Ok(serde_json::json!({
            "node_id": self.node_id,
            "port": self.port,
            "leader_port": leader_port,
            "schema_version": crate::models::get_schema_version(),
            "migration_manager_enabled": true,
            "note": "Migration state is tracked in-memory on each node"
        }))
    }

    /// Find the current leader port
    async fn find_leader_port(&self) -> Result<u16> {
        // Single-node mode: local node is always the leader
        if self.peer_ports.is_empty() {
            return Ok(self.port);
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()?;

        // Check local node first
        let local_url = format!("http://127.0.0.1:{}/_raft/health", self.port);
        if let Ok(resp) = client.get(&local_url).send().await {
            if let Ok(health) = resp.json::<serde_json::Value>().await {
                if health.get("is_leader").and_then(|v| v.as_bool()).unwrap_or(false) {
                    return Ok(self.port);
                }
            }
        }

        // Check peers
        for peer_port in &self.peer_ports {
            let url = format!("http://127.0.0.1:{}/_raft/health", peer_port);
            if let Ok(resp) = client.get(&url).send().await {
                if let Ok(health) = resp.json::<serde_json::Value>().await {
                    if health.get("is_leader").and_then(|v| v.as_bool()).unwrap_or(false) {
                        return Ok(*peer_port);
                    }
                }
            }
        }

        anyhow::bail!("No leader found in cluster")
    }
}

/// Parse version string "1.2.3" into parts
fn parse_version_part(version: &str, index: usize) -> u32 {
    version
        .split('.')
        .nth(index)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}
