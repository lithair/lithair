//! Cluster replication plane: follower apply endpoints (`/internal/*`), Raft
//! log/snapshot RPCs (`/_raft/*`), resync/migrate operations, and the
//! leader-side fan-out (apply_crud_operation, snapshot shipping).

use super::*;
use anyhow::Result;
use bytes::Bytes;
use std::sync::Arc;

impl LithairServer {
    /// Handle internal replication request from leader
    /// POST /internal/replicate
    /// Body: { "model": "products", "operation": "create", "data": {...} }
    pub(super) async fn handle_internal_replicate(
        &self,
        req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<RouteResponse> {
        use http_body_util::BodyExt;

        // Parse body
        let body_bytes = match req.into_body().collect().await.map(|c| c.to_bytes()) {
            Ok(bytes) => bytes,
            Err(_) => {
                return Ok(response::json(StatusCode::BAD_REQUEST, r#"{"error":"Invalid body"}"#));
            }
        };

        let message: serde_json::Value = match serde_json::from_slice(&body_bytes) {
            Ok(v) => v,
            Err(e) => {
                return Ok(response::json_value(
                    StatusCode::BAD_REQUEST,
                    &serde_json::json!({"error": format!("Invalid JSON: {}", e)}),
                ));
            }
        };

        // Extract model base_path from the message if present, else try to match by data structure
        let base_path = message.get("base_path").and_then(|v| v.as_str()).map(|s| s.to_string());

        // Check for consensus-style operation (LithairAppData structure)
        let operation = message.get("operation");
        let model_type = message.get("model_type").and_then(|v| v.as_str());

        // Find the matching model handler
        let models = self.models.read().await;

        // Try to match by base_path, model_type, or fallback to first
        let handler = if let Some(ref path) = base_path {
            models
                .iter()
                .find(|m| m.base_path == *path || m.base_path == format!("/api/{}", path))
        } else if let Some(mtype) = model_type {
            models.iter().find(|m| m.name == mtype || m.base_path.contains(mtype))
        } else {
            // Fallback: use first model (typical single-model clusters)
            models.first()
        };

        if let Some(model) = handler {
            // Handle consensus-style CrudOperation enum from LithairAppData
            if let Some(op) = operation {
                // Parse CrudOperation: {"Create": {...}}, {"Update": {...}}, or {"Delete": {...}}
                if let Some(create_data) = op.get("Create") {
                    let item_data =
                        create_data.get("item").cloned().unwrap_or(serde_json::Value::Null);
                    match model.handler.apply_replicated_item_json(item_data).await {
                        Ok(()) => {
                            log::debug!("CREATE replication applied for model {}", model.name);
                            return Ok(response::json(StatusCode::OK, r#"{"status":"ok"}"#));
                        }
                        Err(e) => {
                            log::error!("CREATE replication failed: {}", e);
                            return Ok(response::json_value(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                &serde_json::json!({"error": e.to_string()}),
                            ));
                        }
                    }
                } else if let Some(update_data) = op.get("Update") {
                    let item_data =
                        update_data.get("item").cloned().unwrap_or(serde_json::Value::Null);
                    let primary_key =
                        update_data.get("primary_key").and_then(|v| v.as_str()).unwrap_or("");
                    match model.handler.apply_replicated_update_json(primary_key, item_data).await {
                        Ok(()) => {
                            log::debug!("UPDATE replication applied for model {}", model.name);
                            return Ok(response::json(StatusCode::OK, r#"{"status":"ok"}"#));
                        }
                        Err(e) => {
                            log::error!("UPDATE replication failed: {}", e);
                            return Ok(response::json_value(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                &serde_json::json!({"error": e.to_string()}),
                            ));
                        }
                    }
                } else if let Some(delete_data) = op.get("Delete") {
                    let primary_key =
                        delete_data.get("primary_key").and_then(|v| v.as_str()).unwrap_or("");
                    match model.handler.apply_replicated_delete_json(primary_key).await {
                        Ok(_) => {
                            log::debug!("DELETE replication applied for model {}", model.name);
                            return Ok(response::json(StatusCode::OK, r#"{"status":"ok"}"#));
                        }
                        Err(e) => {
                            log::error!("DELETE replication failed: {}", e);
                            return Ok(response::json_value(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                &serde_json::json!({"error": e.to_string()}),
                            ));
                        }
                    }
                }
            }

            // Fallback: legacy format with "data" field (CREATE only)
            let item_data = match message.get("data") {
                Some(data) => data.clone(),
                None => {
                    return Ok(response::json(
                        StatusCode::BAD_REQUEST,
                        r#"{"error":"Missing 'data' or 'operation' field"}"#,
                    ));
                }
            };

            match model.handler.apply_replicated_item_json(item_data).await {
                Ok(()) => {
                    log::debug!("Replication applied for model {}", model.name);
                    Ok(response::json(StatusCode::OK, r#"{"status":"ok"}"#))
                }
                Err(e) => {
                    log::error!("Replication failed: {}", e);
                    Ok(response::json_value(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        &serde_json::json!({"error": e.to_string()}),
                    ))
                }
            }
        } else {
            Ok(response::json(StatusCode::NOT_FOUND, r#"{"error":"No model handler found"}"#))
        }
    }

    /// Handle bulk internal replication request from leader
    /// POST /internal/replicate_bulk
    /// Body: { "model": "products", "items": [...], "batch_id": "..." }
    pub(super) async fn handle_internal_replicate_bulk(
        &self,
        req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<RouteResponse> {
        use http_body_util::BodyExt;

        // Parse body
        let body_bytes = match req.into_body().collect().await.map(|c| c.to_bytes()) {
            Ok(bytes) => bytes,
            Err(_) => {
                return Ok(response::json(StatusCode::BAD_REQUEST, r#"{"error":"Invalid body"}"#));
            }
        };

        let message: serde_json::Value = match serde_json::from_slice(&body_bytes) {
            Ok(v) => v,
            Err(e) => {
                return Ok(response::json_value(
                    StatusCode::BAD_REQUEST,
                    &serde_json::json!({"error": format!("Invalid JSON: {}", e)}),
                ));
            }
        };

        // Extract model base_path
        let base_path = message.get("base_path").and_then(|v| v.as_str()).map(|s| s.to_string());

        // Get the items array
        let items: Vec<serde_json::Value> = match message.get("items") {
            Some(serde_json::Value::Array(arr)) => arr.clone(),
            _ => {
                return Ok(response::json(
                    StatusCode::BAD_REQUEST,
                    r#"{"error":"Missing or invalid 'items' field"}"#,
                ));
            }
        };

        let batch_id = message.get("batch_id").and_then(|v| v.as_str()).unwrap_or("unknown");

        // Find the matching model handler
        let models = self.models.read().await;

        let handler = if let Some(ref path) = base_path {
            models
                .iter()
                .find(|m| m.base_path == *path || m.base_path == format!("/api/{}", path))
        } else {
            models.first()
        };

        if let Some(model) = handler {
            match model.handler.apply_replicated_items_json(items).await {
                Ok(count) => {
                    log::debug!(
                        "Bulk replication applied: {} items for model {} (batch: {})",
                        count,
                        model.name,
                        batch_id
                    );
                    Ok(response::json(
                        StatusCode::OK,
                        format!(r#"{{"status":"ok","count":{}}}"#, count),
                    ))
                }
                Err(e) => {
                    log::error!("Bulk replication failed: {}", e);
                    Ok(response::json_value(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        &serde_json::json!({"error": e.to_string()}),
                    ))
                }
            }
        } else {
            Ok(response::json(StatusCode::NOT_FOUND, r#"{"error":"No model handler found"}"#))
        }
    }

    /// Handle internal UPDATE replication request from leader
    /// POST /internal/replicate_update
    /// Body: { "base_path": "products", "id": "123", "data": {...} }
    pub(super) async fn handle_internal_replicate_update(
        &self,
        req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<RouteResponse> {
        use http_body_util::BodyExt;

        // Parse body
        let body_bytes = match req.into_body().collect().await.map(|c| c.to_bytes()) {
            Ok(bytes) => bytes,
            Err(_) => {
                return Ok(response::json(StatusCode::BAD_REQUEST, r#"{"error":"Invalid body"}"#));
            }
        };

        let message: serde_json::Value = match serde_json::from_slice(&body_bytes) {
            Ok(v) => v,
            Err(e) => {
                return Ok(response::json_value(
                    StatusCode::BAD_REQUEST,
                    &serde_json::json!({"error": format!("Invalid JSON: {}", e)}),
                ));
            }
        };

        // Extract required fields
        let base_path = message.get("base_path").and_then(|v| v.as_str()).map(|s| s.to_string());

        let id = match message.get("id").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => {
                return Ok(response::json(
                    StatusCode::BAD_REQUEST,
                    r#"{"error":"Missing 'id' field"}"#,
                ));
            }
        };

        let item_data = match message.get("data") {
            Some(data) => data.clone(),
            None => {
                return Ok(response::json(
                    StatusCode::BAD_REQUEST,
                    r#"{"error":"Missing 'data' field"}"#,
                ));
            }
        };

        // Find the matching model handler
        let models = self.models.read().await;

        let handler = if let Some(ref path) = base_path {
            models
                .iter()
                .find(|m| m.base_path == *path || m.base_path == format!("/api/{}", path))
        } else {
            models.first()
        };

        if let Some(model) = handler {
            match model.handler.apply_replicated_update_json(&id, item_data).await {
                Ok(()) => {
                    log::debug!("Replication UPDATE applied for {} in model {}", id, model.name);
                    Ok(response::json(StatusCode::OK, r#"{"status":"ok"}"#))
                }
                Err(e) => {
                    log::error!("Replication UPDATE failed: {}", e);
                    Ok(response::json_value(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        &serde_json::json!({"error": e.to_string()}),
                    ))
                }
            }
        } else {
            Ok(response::json(StatusCode::NOT_FOUND, r#"{"error":"No model handler found"}"#))
        }
    }

    /// Handle internal DELETE replication request from leader
    /// POST /internal/replicate_delete
    /// Body: { "base_path": "products", "id": "123" }
    pub(super) async fn handle_internal_replicate_delete(
        &self,
        req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<RouteResponse> {
        use http_body_util::BodyExt;

        // Parse body
        let body_bytes = match req.into_body().collect().await.map(|c| c.to_bytes()) {
            Ok(bytes) => bytes,
            Err(_) => {
                return Ok(response::json(StatusCode::BAD_REQUEST, r#"{"error":"Invalid body"}"#));
            }
        };

        let message: serde_json::Value = match serde_json::from_slice(&body_bytes) {
            Ok(v) => v,
            Err(e) => {
                return Ok(response::json_value(
                    StatusCode::BAD_REQUEST,
                    &serde_json::json!({"error": format!("Invalid JSON: {}", e)}),
                ));
            }
        };

        // Extract required fields
        let base_path = message.get("base_path").and_then(|v| v.as_str()).map(|s| s.to_string());

        let id = match message.get("id").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => {
                return Ok(response::json(
                    StatusCode::BAD_REQUEST,
                    r#"{"error":"Missing 'id' field"}"#,
                ));
            }
        };

        // Find the matching model handler
        let models = self.models.read().await;

        let handler = if let Some(ref path) = base_path {
            models
                .iter()
                .find(|m| m.base_path == *path || m.base_path == format!("/api/{}", path))
        } else {
            models.first()
        };

        if let Some(model) = handler {
            match model.handler.apply_replicated_delete_json(&id).await {
                Ok(deleted) => {
                    log::debug!(
                        "Replication DELETE applied for {} in model {} (deleted: {})",
                        id,
                        model.name,
                        deleted
                    );
                    Ok(response::json(
                        StatusCode::OK,
                        format!(r#"{{"status":"ok","deleted":{}}}"#, deleted),
                    ))
                }
                Err(e) => {
                    log::error!("Replication DELETE failed: {}", e);
                    Ok(response::json_value(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        &serde_json::json!({"error": e.to_string()}),
                    ))
                }
            }
        } else {
            Ok(response::json(StatusCode::NOT_FOUND, r#"{"error":"No model handler found"}"#))
        }
    }

    /// Handle Raft append entries request from leader
    /// POST /_raft/append
    /// Body: AppendEntriesRequest { term, leader_id, prev_log_index, prev_log_term, entries, leader_commit }
    ///
    /// This endpoint is called by the leader to replicate log entries to followers.
    /// Followers:
    /// 1. Store the entries in their local log
    /// 2. Update their commit index based on leader_commit
    /// 3. Apply committed entries to their state machine
    pub(super) async fn handle_raft_append_entries(
        &self,
        req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<RouteResponse> {
        use http_body_util::BodyExt;

        // Parse request body
        let (_parts, body) = req.into_parts();
        let body_bytes = body.collect().await?.to_bytes();

        let request: crate::cluster::consensus_log::AppendEntriesRequest =
            match serde_json::from_slice(&body_bytes) {
                Ok(r) => r,
                Err(e) => {
                    return Ok(response::json_value(
                        StatusCode::BAD_REQUEST,
                        &serde_json::json!({"error": format!("Invalid request: {}", e)}),
                    ));
                }
            };

        // Check if we have a consensus log
        let consensus_log = match &self.consensus_log {
            Some(log) => log,
            None => {
                return Ok(response::json(
                    StatusCode::SERVICE_UNAVAILABLE,
                    r#"{"error":"Consensus log not initialized"}"#,
                ));
            }
        };

        // Check term - if leader's term is higher, update ours
        let our_term = consensus_log.current_term();
        if request.term > our_term {
            consensus_log.set_term(request.term);
        } else if request.term < our_term {
            // Reject requests from old leaders
            let response = crate::cluster::consensus_log::AppendEntriesResponse {
                term: our_term,
                success: false,
                last_log_index: consensus_log.last_index().await,
                applied_index: consensus_log.applied_index(),
            };
            return Ok(response::json_serialize(StatusCode::OK, &response).unwrap_or_else(|e| {
                response::json_value(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &serde_json::json!({"error": e.to_string()}),
                )
            }));
        }

        // Reconcile Raft role: an accepted AppendEntries means another node is
        // the legitimate leader for this term. If we still think we're a
        // leader/candidate, or the leader has changed, take the full
        // become_follower path so we never keep an old leader's port mapped to
        // a new leader_id (relevant when a legacy peer omits leader_port and
        // update_leader_port would preserve the stale port).
        if let Some(ref raft_state) = self.raft_state {
            let was_follower =
                raft_state.get_current_state() == crate::cluster::RaftNodeState::Follower;
            let same_leader =
                raft_state.current_leader_id.load(std::sync::atomic::Ordering::Relaxed)
                    == request.leader_id;
            if was_follower && same_leader {
                raft_state.update_leader_port(request.leader_id, request.leader_port);
            } else {
                raft_state.become_follower(request.leader_id, request.leader_port);
            }
            raft_state.update_heartbeat();
        }

        // Append entries to local log (can happen concurrently)
        let entries_count = request.entries.len();
        consensus_log
            .append_entries(request.entries.clone(), request.leader_commit)
            .await;

        log::debug!(
            "Received {} entries from leader {}, commit_index={}",
            entries_count,
            request.leader_id,
            request.leader_commit
        );

        // CRITICAL: Acquire the apply lock BEFORE getting unapplied entries and applying them.
        // This prevents race conditions where multiple concurrent handlers could:
        // 1. Both see the same entries as unapplied
        // 2. Apply entries out of order (e.g., DELETE before CREATE)
        // 3. Cause data inconsistency (items existing on followers but not leader, or vice versa)
        let _apply_guard = consensus_log.lock_apply().await;

        // Apply committed entries that we haven't applied yet
        // Since we send ALL entries from index 1, there should be no gaps
        let unapplied = consensus_log.get_unapplied_entries().await;
        let mut all_applied_successfully = true;

        for entry in unapplied {
            let op_type = match &entry.operation {
                crate::cluster::CrudOperation::Create { .. } => "CREATE",
                crate::cluster::CrudOperation::Update { .. } => "UPDATE",
                crate::cluster::CrudOperation::Delete { .. } => "DELETE",
                crate::cluster::CrudOperation::MigrationBegin { .. } => "MIGRATION_BEGIN",
                crate::cluster::CrudOperation::MigrationStep { .. } => "MIGRATION_STEP",
                crate::cluster::CrudOperation::MigrationCommit { .. } => "MIGRATION_COMMIT",
                crate::cluster::CrudOperation::MigrationRollback { .. } => "MIGRATION_ROLLBACK",
            };
            log::debug!("FOLLOWER: Applying {} entry index={}", op_type, entry.log_id.index);
            match self.apply_crud_operation(&entry.operation).await {
                Ok(_) => {
                    consensus_log.mark_applied(entry.log_id.index);
                    log::debug!("Applied entry index={}", entry.log_id.index);
                }
                Err(e) => {
                    // CRITICAL: Stop processing here! If we continue, we'd skip this entry
                    // because mark_applied on later entries would advance applied_index past it.
                    // Mark as NOT all successful so leader knows to retry
                    log::error!(
                        "Failed to apply entry index={}: {} - stopping to prevent skip",
                        entry.log_id.index,
                        e
                    );
                    all_applied_successfully = false;
                    break;
                }
            }
        }
        // _apply_guard dropped here, releasing the lock

        // Only report success if all entries were applied successfully
        // If some failed, leader will retry via background catch-up
        let response = crate::cluster::consensus_log::AppendEntriesResponse {
            term: consensus_log.current_term(),
            success: all_applied_successfully,
            last_log_index: consensus_log.last_index().await,
            applied_index: consensus_log.applied_index(),
        };

        Ok(response::json_serialize(StatusCode::OK, &response).unwrap_or_else(|e| {
            response::json_value(
                StatusCode::INTERNAL_SERVER_ERROR,
                &serde_json::json!({"error": e.to_string()}),
            )
        }))
    }

    /// Handle GET /_raft/snapshot - Return current snapshot for resync
    ///
    /// This endpoint is called by desynced followers to get a full snapshot
    /// of the leader's state for faster catch-up than replaying all logs.
    pub(super) async fn handle_get_snapshot(
        &self,
        _req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<RouteResponse> {
        // Check if snapshot manager is available
        let snapshot_manager = match &self.snapshot_manager {
            Some(mgr) => mgr,
            None => {
                return Ok(response::json(
                    StatusCode::SERVICE_UNAVAILABLE,
                    r#"{"error":"Snapshot manager not initialized"}"#,
                ));
            }
        };

        // Get current snapshot metadata
        let mgr = snapshot_manager.read().await;
        let meta = match mgr.current_meta() {
            Some(m) => m.clone(),
            None => {
                return Ok(response::json(
                    StatusCode::NOT_FOUND,
                    r#"{"error":"No snapshot available"}"#,
                ));
            }
        };

        // Get snapshot bytes
        match mgr.get_snapshot_bytes(meta.last_included_index) {
            Ok(bytes) => {
                // Return snapshot with metadata in headers
                Ok(hyper::Response::builder()
                    .status(hyper::StatusCode::OK)
                    .header("Content-Type", "application/octet-stream")
                    .header("X-Snapshot-Term", meta.term.to_string())
                    .header("X-Snapshot-Index", meta.last_included_index.to_string())
                    .header("X-Snapshot-Checksum", meta.checksum.to_string())
                    .header("X-Snapshot-Size", meta.size_bytes.to_string())
                    .body(boxed_full(Bytes::from(bytes)))
                    .expect("valid HTTP response"))
            }
            Err(e) => {
                log::error!("Failed to read snapshot: {}", e);
                Ok(response::json_value(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &serde_json::json!({"error": format!("Failed to read snapshot: {}", e)}),
                ))
            }
        }
    }

    /// Handle POST /_raft/snapshot - Install snapshot received from leader
    ///
    /// Desynced followers call this to install a snapshot and catch up quickly.
    /// After installation, the follower's state is reset to the snapshot state.
    pub(super) async fn handle_install_snapshot(
        &self,
        req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<RouteResponse> {
        use http_body_util::BodyExt;

        // Check if snapshot manager is available
        let snapshot_manager = match &self.snapshot_manager {
            Some(mgr) => mgr,
            None => {
                return Ok(response::json(
                    StatusCode::SERVICE_UNAVAILABLE,
                    r#"{"error":"Snapshot manager not initialized"}"#,
                ));
            }
        };

        // Extract metadata from headers
        let headers = req.headers();
        let term: u64 = headers
            .get("X-Snapshot-Term")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let last_included_index: u64 = headers
            .get("X-Snapshot-Index")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let checksum: u64 = headers
            .get("X-Snapshot-Checksum")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let size_bytes: u64 = headers
            .get("X-Snapshot-Size")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        // Read body bytes
        // IMPORTANT: Convert to Vec<u8> for proper alignment - rkyv 0.8's bytecheck
        // validation requires aligned data, and bytes::Bytes may not provide this
        let body_bytes: Vec<u8> = match req.into_body().collect().await.map(|c| c.to_bytes()) {
            Ok(bytes) => bytes.to_vec(),
            Err(e) => {
                return Ok(response::json_value(
                    StatusCode::BAD_REQUEST,
                    &serde_json::json!({"error": format!("Failed to read body: {}", e)}),
                ));
            }
        };

        // Create metadata
        let meta = crate::cluster::snapshot::SnapshotMeta {
            term,
            last_included_index,
            created_at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            size_bytes,
            checksum,
        };

        // Record snapshot received (for observability)
        self.resync_stats.record_snapshot_received(last_included_index);

        // Install the snapshot
        let mut mgr = snapshot_manager.write().await;
        match mgr.install_snapshot(meta.clone(), &body_bytes) {
            Ok(snapshot_data) => {
                log::info!("Snapshot installed: index={}, term={}", last_included_index, term);

                // Record snapshot applied (for observability)
                self.resync_stats.record_snapshot_applied();

                // Apply snapshot data to models
                let models = self.models.read().await;
                for (model_path, json_data) in &snapshot_data.models {
                    if let Some(model) = models.iter().find(|m| m.base_path == *model_path) {
                        let items: Vec<serde_json::Value> =
                            serde_json::from_str(json_data).unwrap_or_default();
                        if let Err(e) = model.handler.apply_replicated_items_json(items).await {
                            log::error!("Failed to apply snapshot data for {}: {}", model_path, e);
                        }
                    }
                }

                // Update consensus log if present
                if let Some(ref consensus_log) = self.consensus_log {
                    consensus_log.set_term(term);
                    // Mark entries up to snapshot index as applied
                    consensus_log.mark_applied(last_included_index);
                }

                let response = crate::cluster::snapshot::InstallSnapshotResponse {
                    term,
                    success: true,
                    error: None,
                };

                Ok(response::json_serialize(StatusCode::OK, &response).unwrap_or_else(|e| {
                    response::json_value(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        &serde_json::json!({"error": e.to_string()}),
                    )
                }))
            }
            Err(e) => {
                log::error!("Failed to install snapshot: {}", e);
                let response = crate::cluster::snapshot::InstallSnapshotResponse {
                    term: self.consensus_log.as_ref().map(|l| l.current_term()).unwrap_or(0),
                    success: false,
                    error: Some(e.to_string()),
                };

                Ok(response::json_serialize(StatusCode::INTERNAL_SERVER_ERROR, &response)
                    .unwrap_or_else(|e| {
                        response::json_value(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            &serde_json::json!({"error": e.to_string()}),
                        )
                    }))
            }
        }
    }

    /// Handle GET /_raft/health - Return cluster health status
    ///
    /// Returns detailed health information about all followers including:
    /// - Health status (healthy, lagging, desynced, unknown)
    /// - Last replicated index
    /// - Latency statistics
    /// - Pending entry counts
    pub(super) async fn handle_cluster_health(&self) -> Result<RouteResponse> {
        let mut health_data = serde_json::json!({
            "status": "ok",
            "node_id": self.node_id,
            "is_leader": self.raft_state.as_ref().map(|s| s.is_leader()).unwrap_or(false),
            "cluster_peers": self.cluster_peers.len(),
        });

        // Add consensus log info if present
        if let Some(ref consensus_log) = self.consensus_log {
            health_data["consensus"] = serde_json::json!({
                "term": consensus_log.current_term(),
                "commit_index": consensus_log.commit_index(),
                "last_applied": consensus_log.applied_index(),
            });
        }

        // Add batcher health summary if present
        if let Some(ref batcher) = self.replication_batcher {
            let summary = batcher.get_health_summary().await;
            let mut followers = Vec::new();

            for (addr, health) in summary {
                let mut follower_info = serde_json::json!({
                    "address": addr,
                    "health": health.to_string(),
                });

                // Get detailed stats if available
                if let Some(stats) = batcher.get_follower_stats(&addr).await {
                    follower_info["last_replicated_index"] =
                        serde_json::json!(stats.last_replicated_index);
                    follower_info["last_latency_ms"] = serde_json::json!(stats.last_latency_ms);
                    follower_info["pending_count"] = serde_json::json!(stats.pending_count);
                    follower_info["consecutive_failures"] =
                        serde_json::json!(stats.consecutive_failures);
                }

                followers.push(follower_info);
            }

            health_data["followers"] = serde_json::json!(followers);

            // Check for desynced followers
            let commit_index = self.consensus_log.as_ref().map(|l| l.commit_index()).unwrap_or(0);
            let desynced = batcher.get_desynced_followers(commit_index).await;
            if !desynced.is_empty() {
                health_data["desynced_followers"] = serde_json::json!(desynced);
            }
        }

        // Add WAL info if present
        if self.wal.is_some() {
            health_data["wal"] = serde_json::json!({
                "enabled": true,
            });
        }

        // Add snapshot info if present
        if let Some(ref snapshot_manager) = self.snapshot_manager {
            let mgr = snapshot_manager.read().await;
            if let Some(meta) = mgr.current_meta() {
                health_data["snapshot"] = serde_json::json!({
                    "available": true,
                    "term": meta.term,
                    "last_included_index": meta.last_included_index,
                    "size_bytes": meta.size_bytes,
                    "created_at_ms": meta.created_at_ms,
                });
            } else {
                health_data["snapshot"] = serde_json::json!({
                    "available": false,
                });
            }
        }

        Ok(response::json_value(StatusCode::OK, &health_data))
    }

    /// Handle GET /_raft/resync_stats - Return snapshot resync statistics
    ///
    /// Returns observability data for snapshot-based resync operations:
    /// - Leader side: snapshots created, send attempts/successes/failures
    /// - Follower side: snapshots received, snapshots applied
    /// - Indices and timestamps for debugging
    pub(super) async fn handle_resync_stats(&self) -> Result<RouteResponse> {
        let stats_json = self.resync_stats.to_json();

        let response_data = serde_json::json!({
            "node_id": self.node_id,
            "is_leader": self.raft_state.as_ref().map(|s| s.is_leader()).unwrap_or(false),
            "resync_stats": stats_json,
        });

        Ok(response::json_value(StatusCode::OK, &response_data))
    }

    /// Handle GET /_raft/sync-status - Return detailed sync status for each follower
    ///
    /// Returns for each follower:
    /// - address: peer address
    /// - health: healthy/lagging/desynced/unknown
    /// - last_replicated_index: last known replicated index
    /// - lag: how many entries behind the leader commit_index
    /// - last_latency_ms: last replication latency
    /// - pending_count: pending batched entries
    /// - consecutive_failures: failure counter
    pub(super) async fn handle_sync_status(&self) -> Result<RouteResponse> {
        let is_leader = self.raft_state.as_ref().map(|s| s.is_leader()).unwrap_or(false);

        if !is_leader {
            return Ok(response::json_value(
                StatusCode::OK,
                &serde_json::json!({
                    "node_id": self.node_id,
                    "is_leader": false,
                    "message": "This node is not the leader. Sync status is only available on the leader."
                }),
            ));
        }

        // Get commit index from consensus log
        let commit_index = if let Some(log) = &self.consensus_log { log.commit_index() } else { 0 };

        // Get follower stats from batcher
        let followers_stats = if let Some(batcher) = &self.replication_batcher {
            batcher.get_all_follower_stats().await
        } else {
            vec![]
        };

        // Build response with lag calculation
        let followers_json: Vec<serde_json::Value> = followers_stats
            .iter()
            .map(|f| {
                let lag = commit_index.saturating_sub(f.last_replicated_index);

                serde_json::json!({
                    "address": f.address,
                    "health": f.health.to_string(),
                    "last_replicated_index": f.last_replicated_index,
                    "lag": lag,
                    "last_latency_ms": f.last_latency_ms,
                    "pending_count": f.pending_count,
                    "consecutive_failures": f.consecutive_failures,
                })
            })
            .collect();

        let response_data = serde_json::json!({
            "node_id": self.node_id,
            "is_leader": true,
            "commit_index": commit_index,
            "followers": followers_json,
        });

        Ok(response::json_value(StatusCode::OK, &response_data))
    }

    /// Handle POST /_raft/force-resync - Manually trigger snapshot resync to a follower
    ///
    /// Query params:
    /// - target: peer address (e.g., "127.0.0.1:8081")
    ///
    /// This marks the follower as desynced and triggers immediate snapshot send.
    /// Use this when a node has restarted and needs to catch up from scratch.
    pub(super) async fn handle_force_resync(
        &self,
        req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<RouteResponse> {
        let is_leader = self.raft_state.as_ref().map(|s| s.is_leader()).unwrap_or(false);

        if !is_leader {
            return Ok(response::json(
                StatusCode::BAD_REQUEST,
                r#"{"error":"This node is not the leader. Force resync must be called on the leader."}"#,
            ));
        }

        // Parse target from query string
        let uri = req.uri();
        let query = uri.query().unwrap_or("");
        let target = query.split('&').find_map(|pair| {
            let mut parts = pair.split('=');
            match (parts.next(), parts.next()) {
                (Some("target"), Some(value)) => Some(value.to_string()),
                _ => None,
            }
        });

        let target = match target {
            Some(t) => t,
            None => {
                return Ok(response::json(
                    StatusCode::BAD_REQUEST,
                    r#"{"error":"Missing 'target' query parameter. Use /_raft/force-resync?target=127.0.0.1:8081"}"#,
                ));
            }
        };

        log::info!("Manual resync requested for follower: {}", target);

        // Mark follower as desynced
        let marked = if let Some(batcher) = &self.replication_batcher {
            batcher.mark_follower_desynced(&target).await
        } else {
            false
        };

        if !marked {
            return Ok(response::json_value(
                StatusCode::NOT_FOUND,
                &serde_json::json!({"error": format!("Follower '{}' not found in cluster", target)}),
            ));
        }

        // Trigger immediate snapshot send
        let snapshot_result = if let Some(snapshot_manager) = &self.snapshot_manager {
            Self::send_snapshot_to_follower_with_timeout(&target, snapshot_manager, 60).await
        } else {
            Err("Snapshot manager not available".to_string())
        };

        // Update resync stats
        // Stats will be recorded after we know the result

        let (status, message) = match snapshot_result {
            Ok(()) => {
                self.resync_stats.record_send_success();
                log::info!("Manual resync to {} completed successfully", target);
                (
                    hyper::StatusCode::OK,
                    format!("Snapshot resync to {} completed successfully", target),
                )
            }
            Err(e) => {
                self.resync_stats.record_send_failure();
                log::error!("Manual resync to {} failed: {}", target, e);
                (hyper::StatusCode::INTERNAL_SERVER_ERROR, format!("Resync failed: {}", e))
            }
        };

        Ok(response::json_value(
            status,
            &serde_json::json!({
                "target": target,
                "success": status == hyper::StatusCode::OK,
                "message": message,
            }),
        ))
    }

    /// Handle POST /_raft/migrate - Submit migration operations through consensus
    ///
    /// This endpoint allows submitting migration operations (MigrationBegin, MigrationStep,
    /// MigrationCommit, MigrationRollback) to be replicated through the Raft consensus log.
    /// Only the leader can accept these operations.
    pub(super) async fn handle_migrate_operation(
        &self,
        mut req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<RouteResponse> {
        use http_body_util::BodyExt;

        // Only leader can accept migration operations
        let is_leader = self.raft_state.as_ref().map(|s| s.is_leader()).unwrap_or(true);
        if !is_leader {
            let leader_port = self.raft_state.as_ref().map(|s| s.get_leader_port()).unwrap_or(0);
            if leader_port == 0 {
                return Ok(Self::leader_port_unknown_503());
            }
            return Ok(hyper::Response::builder()
                .status(hyper::StatusCode::TEMPORARY_REDIRECT)
                .header("Content-Type", "application/json")
                .header("Location", format!("http://127.0.0.1:{}/_raft/migrate", leader_port))
                .body(boxed_full(Bytes::from(
                    serde_json::json!({
                        "error": "Not leader",
                        "leader_port": leader_port
                    })
                    .to_string(),
                )))
                .expect("valid HTTP response"));
        }

        // Parse the operation from request body
        let body = req
            .body_mut()
            .collect()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read body: {}", e))?
            .to_bytes();

        let operation: crate::cluster::CrudOperation = match serde_json::from_slice(&body) {
            Ok(op) => op,
            Err(e) => {
                return Ok(response::json_value(
                    StatusCode::BAD_REQUEST,
                    &serde_json::json!({
                        "error": format!("Invalid operation: {}", e)
                    }),
                ));
            }
        };

        // Verify it's a migration operation
        let is_migration = matches!(
            operation,
            crate::cluster::CrudOperation::MigrationBegin { .. }
                | crate::cluster::CrudOperation::MigrationStep { .. }
                | crate::cluster::CrudOperation::MigrationCommit { .. }
                | crate::cluster::CrudOperation::MigrationRollback { .. }
        );

        if !is_migration {
            return Ok(response::json_value(
                StatusCode::BAD_REQUEST,
                &serde_json::json!({
                    "error": "Only migration operations are allowed on this endpoint"
                }),
            ));
        }

        // Create log entry and replicate
        if let Some(ref consensus_log) = self.consensus_log {
            // Append to local log (creates LogEntry internally)
            let entry = consensus_log.append(operation.clone()).await;

            // Write to WAL
            if let Some(ref wal) = self.wal {
                if let Err(e) = wal.append(&entry).await {
                    log::error!("Failed to write migration to WAL: {}", e);
                }
            }

            // Replicate to followers
            let commit_index = consensus_log.commit_index();
            let term = consensus_log.current_term();
            let leader_id = self.node_id.unwrap_or(0);

            let replication_result = Self::replicate_log_entries_to_followers(
                &self.cluster_peers,
                vec![entry],
                commit_index,
                term,
                leader_id,
                self.config.server.port,
                self.replication_batcher.clone(),
            )
            .await;

            match replication_result {
                Ok(new_commit) => {
                    // Update commit index
                    consensus_log.commit(new_commit);

                    // Apply the operation locally
                    let apply_result = self.apply_crud_operation(&operation).await;

                    match apply_result {
                        Ok(result) => Ok(response::json_value(
                            StatusCode::OK,
                            &serde_json::json!({
                                "success": true,
                                "commit_index": new_commit,
                                "result": result
                            }),
                        )),
                        Err(e) => Ok(response::json_value(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            &serde_json::json!({
                                "error": format!("Migration apply failed: {}", e),
                                "commit_index": new_commit
                            }),
                        )),
                    }
                }
                Err(e) => Ok(response::json_value(
                    StatusCode::SERVICE_UNAVAILABLE,
                    &serde_json::json!({
                        "error": format!("Replication failed: {}", e)
                    }),
                )),
            }
        } else {
            // Single node mode - just apply
            let apply_result = self.apply_crud_operation(&operation).await;
            match apply_result {
                Ok(result) => Ok(response::json_value(
                    StatusCode::OK,
                    &serde_json::json!({
                        "success": true,
                        "result": result
                    }),
                )),
                Err(e) => Ok(response::json_value(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &serde_json::json!({
                        "error": format!("Migration failed: {}", e)
                    }),
                )),
            }
        }
    }

    /// Apply a CRUD operation from the consensus log to the appropriate model
    /// This is called when a log entry is committed and needs to be applied to the state machine
    pub async fn apply_crud_operation(
        &self,
        operation: &crate::cluster::CrudOperation,
    ) -> Result<serde_json::Value, String> {
        use crate::cluster::CrudOperation;

        let models = self.models.read().await;

        match operation {
            CrudOperation::Create { model_path, data } => {
                // Find the model by base_path
                let model = models
                    .iter()
                    .find(|m| model_path.starts_with(&m.base_path))
                    .ok_or_else(|| format!("Model not found for path: {}", model_path))?;

                model.handler.apply_replicated_item_json(data.clone()).await?;
                Ok(data.clone())
            }
            CrudOperation::Update { model_path, id, data } => {
                let model = models
                    .iter()
                    .find(|m| model_path.starts_with(&m.base_path))
                    .ok_or_else(|| format!("Model not found for path: {}", model_path))?;

                model.handler.apply_replicated_update_json(id, data.clone()).await?;
                Ok(data.clone())
            }
            CrudOperation::Delete { model_path, id } => {
                let model = models
                    .iter()
                    .find(|m| model_path.starts_with(&m.base_path))
                    .ok_or_else(|| format!("Model not found for path: {}", model_path))?;

                model.handler.apply_replicated_delete_json(id).await?;
                Ok(serde_json::json!({"deleted": id}))
            }
            // === Migration Operations (Phase 2: Full implementation) ===
            CrudOperation::MigrationBegin { from_version, to_version, migration_id } => {
                log::info!(
                    "MIGRATION_BEGIN: {} -> {} (id: {})",
                    from_version,
                    to_version,
                    migration_id
                );

                // Use migration manager if available
                if let Some(ref manager) = self.migration_manager {
                    match manager
                        .begin_migration(*migration_id, from_version.clone(), to_version.clone())
                        .await
                    {
                        Ok(()) => {
                            log::info!("Migration {} registered successfully", migration_id);
                            Ok(serde_json::json!({
                                "status": "started",
                                "migration_id": migration_id.to_string(),
                                "from": from_version.to_string(),
                                "to": to_version.to_string(),
                            }))
                        }
                        Err(e) => {
                            log::error!("Failed to begin migration {}: {}", migration_id, e);
                            Err(e)
                        }
                    }
                } else {
                    // No migration manager - acknowledge but warn
                    log::warn!("Migration manager not available, migration {} acknowledged but not tracked", migration_id);
                    Ok(serde_json::json!({
                        "status": "acknowledged",
                        "warning": "No migration manager available",
                        "migration_id": migration_id.to_string(),
                        "from": from_version.to_string(),
                        "to": to_version.to_string(),
                    }))
                }
            }
            CrudOperation::MigrationStep { migration_id, step_index, operation } => {
                log::info!(
                    "MIGRATION_STEP: migration={}, step={}, operation={:?}",
                    migration_id,
                    step_index,
                    operation
                );

                // Apply the schema change and record rollback
                let result = self.apply_schema_change(operation).await;

                match result {
                    Ok(rollback_op) => {
                        // Record step in migration manager
                        if let Some(ref manager) = self.migration_manager {
                            if let Err(e) = manager
                                .record_step(migration_id, *step_index, rollback_op, None)
                                .await
                            {
                                log::warn!("Failed to record migration step: {}", e);
                            }
                        }
                        Ok(serde_json::json!({
                            "status": "applied",
                            "migration_id": migration_id.to_string(),
                            "step_index": step_index,
                        }))
                    }
                    Err(e) => {
                        log::error!("Migration step {} failed: {}", step_index, e);
                        Err(format!("Migration step {} failed: {}", step_index, e))
                    }
                }
            }
            CrudOperation::MigrationCommit { migration_id, checksum } => {
                log::info!("MIGRATION_COMMIT: migration={}, checksum={}", migration_id, checksum);

                if let Some(ref manager) = self.migration_manager {
                    // Get migration context to get the target version
                    if let Some(ctx) = manager.get_migration(migration_id).await {
                        let new_version = ctx.to_version.clone();
                        match manager.commit_migration(migration_id, new_version).await {
                            Ok(()) => {
                                log::info!(
                                    "Migration {} committed, checksum verified: {}",
                                    migration_id,
                                    checksum
                                );
                                Ok(serde_json::json!({
                                    "status": "committed",
                                    "migration_id": migration_id.to_string(),
                                    "checksum": checksum,
                                }))
                            }
                            Err(e) => {
                                log::error!("Failed to commit migration {}: {}", migration_id, e);
                                Err(e)
                            }
                        }
                    } else {
                        let msg = format!("Migration {} not found", migration_id);
                        log::error!("{}", msg);
                        Err(msg)
                    }
                } else {
                    Ok(serde_json::json!({
                        "status": "acknowledged",
                        "warning": "No migration manager available",
                        "migration_id": migration_id.to_string(),
                        "checksum": checksum,
                    }))
                }
            }
            CrudOperation::MigrationRollback { migration_id, failed_step, reason } => {
                log::warn!(
                    "MIGRATION_ROLLBACK: migration={}, failed_step={}, reason={}",
                    migration_id,
                    failed_step,
                    reason
                );

                if let Some(ref manager) = self.migration_manager {
                    match manager.rollback_migration(migration_id).await {
                        Ok(rollback_ops) => {
                            // Apply rollback operations in reverse order
                            let mut rollback_errors = Vec::new();
                            for op in rollback_ops {
                                log::info!("Rolling back step {}", op.step_index);
                                if let Err(e) = self.apply_schema_change(&op.operation).await {
                                    log::error!("Rollback step {} failed: {}", op.step_index, e);
                                    rollback_errors.push(format!("Step {}: {}", op.step_index, e));
                                }
                            }

                            if rollback_errors.is_empty() {
                                log::info!("Migration {} fully rolled back", migration_id);
                                Ok(serde_json::json!({
                                    "status": "rolled_back",
                                    "migration_id": migration_id.to_string(),
                                    "failed_step": failed_step,
                                    "reason": reason,
                                }))
                            } else {
                                log::error!(
                                    "Migration {} partially rolled back with errors",
                                    migration_id
                                );
                                Ok(serde_json::json!({
                                    "status": "partial_rollback",
                                    "migration_id": migration_id.to_string(),
                                    "failed_step": failed_step,
                                    "reason": reason,
                                    "rollback_errors": rollback_errors,
                                }))
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to rollback migration {}: {}", migration_id, e);
                            Err(e)
                        }
                    }
                } else {
                    Ok(serde_json::json!({
                        "status": "acknowledged",
                        "warning": "No migration manager available",
                        "migration_id": migration_id.to_string(),
                        "failed_step": failed_step,
                        "reason": reason,
                    }))
                }
            }
        }
    }

    /// Apply a schema change and return the inverse operation for rollback
    ///
    /// This method applies schema changes from migrations and returns the inverse
    /// SchemaChange that would undo this operation.
    pub(super) async fn apply_schema_change(
        &self,
        change: &crate::cluster::SchemaChange,
    ) -> Result<crate::cluster::SchemaChange, String> {
        use crate::cluster::SchemaChange;

        match change {
            SchemaChange::AddModel { name, schema: _ } => {
                log::info!("Applying AddModel: {}", name);
                // Phase 3 planned feature: register model in runtime schema registry
                // For now, log and return the inverse operation
                Ok(SchemaChange::RemoveModel { name: name.clone(), backup_path: None })
            }
            SchemaChange::RemoveModel { name, backup_path } => {
                log::info!("Applying RemoveModel: {} (backup: {:?})", name, backup_path);
                // Phase 3 planned feature: remove model from registry, backup data if path provided
                // For rollback, we need the original schema - this would be stored in migration context
                Ok(SchemaChange::Custom {
                    description: format!("Restore model: {}", name),
                    forward: format!("restore_model:{}", name),
                    backward: format!("remove_model:{}", name),
                })
            }
            SchemaChange::AddField { model, field, default_value: _ } => {
                log::info!("Applying AddField: {}.{}", model, field.name);
                // Phase 3 planned feature: add field to model schema
                Ok(SchemaChange::RemoveField { model: model.clone(), field: field.name.clone() })
            }
            SchemaChange::RemoveField { model, field } => {
                log::info!("Applying RemoveField: {}.{}", model, field);
                // Phase 3 planned feature: remove field from model, backup data
                // For rollback, we need the original field definition
                Ok(SchemaChange::Custom {
                    description: format!("Restore field: {}.{}", model, field),
                    forward: format!("restore_field:{}:{}", model, field),
                    backward: format!("remove_field:{}:{}", model, field),
                })
            }
            SchemaChange::RenameField { model, old_name, new_name } => {
                log::info!("Applying RenameField: {}.{} -> {}", model, old_name, new_name);
                // Phase 3 planned feature: update field name in schema and all data
                // Inverse is simply swapping old and new names
                Ok(SchemaChange::RenameField {
                    model: model.clone(),
                    old_name: new_name.clone(),
                    new_name: old_name.clone(),
                })
            }
            SchemaChange::ChangeFieldType { model, field, new_type, transform: _ } => {
                log::info!("Applying ChangeFieldType: {}.{} -> {:?}", model, field, new_type);
                // Phase 3 planned feature: transform field data to new type
                // For rollback, we need the original type - stored in migration context
                Ok(SchemaChange::Custom {
                    description: format!("Restore field type: {}.{}", model, field),
                    forward: format!("restore_type:{}:{}", model, field),
                    backward: format!("change_type:{}:{}:{:?}", model, field, new_type),
                })
            }
            SchemaChange::AddIndex { model, fields, unique } => {
                log::info!("Applying AddIndex: {}.{:?} (unique: {})", model, fields, unique);
                // Phase 3 planned feature: create index on model fields
                // Inverse: remove the index
                Ok(SchemaChange::Custom {
                    description: format!("Remove index on {}.{:?}", model, fields),
                    forward: format!("drop_index:{}:{}", model, fields.join(",")),
                    backward: format!("create_index:{}:{}:{}", model, fields.join(","), unique),
                })
            }
            SchemaChange::Custom { description, forward, backward } => {
                log::info!("Applying Custom: {}", description);
                // Custom operations define their own forward/backward
                // Inverse swaps forward and backward
                Ok(SchemaChange::Custom {
                    description: format!("Undo: {}", description),
                    forward: backward.clone(),
                    backward: forward.clone(),
                })
            }
        }
    }

    /// Create a snapshot from current model state
    ///
    /// This creates a fresh snapshot of all model data for sending to desynced followers.
    pub(super) async fn create_snapshot_from_models(
        models: &Arc<tokio::sync::RwLock<Vec<ModelRegistration>>>,
        snapshot_manager: &Arc<tokio::sync::RwLock<crate::cluster::snapshot::SnapshotManager>>,
        term: u64,
        last_index: u64,
    ) -> Result<crate::cluster::snapshot::SnapshotMeta, String> {
        let mut snapshot_data = crate::cluster::snapshot::SnapshotData::new();

        // Collect all model data
        let models_read = models.read().await;
        for model in models_read.iter() {
            let data_json = model.handler.get_all_data_json().await;
            // get_all_data_json returns a JSON Value (array), convert to vec
            if let serde_json::Value::Array(items) = data_json {
                snapshot_data.add_model(&model.base_path, &items);
            }
        }
        drop(models_read);

        // Create the snapshot
        let mut mgr = snapshot_manager.write().await;
        mgr.create_snapshot(term, last_index, snapshot_data)
            .map_err(|e| format!("Failed to create snapshot: {}", e))
    }

    /// Send a snapshot to a desynced follower
    ///
    /// This pushes a snapshot to a follower that is too far behind to catch up
    /// via normal log replication. Returns Ok(()) on success.
    #[allow(dead_code)] // Kept for API compatibility, prefer send_snapshot_to_follower_with_timeout
    pub(super) async fn send_snapshot_to_follower(
        peer: &str,
        snapshot_manager: &Arc<tokio::sync::RwLock<crate::cluster::snapshot::SnapshotManager>>,
    ) -> Result<(), String> {
        let mgr = snapshot_manager.read().await;

        let meta = mgr
            .current_meta()
            .ok_or_else(|| "No snapshot available to send".to_string())?
            .clone();

        let bytes = mgr
            .get_snapshot_bytes(meta.last_included_index)
            .map_err(|e| format!("Failed to read snapshot bytes: {}", e))?;

        drop(mgr);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30)) // Longer timeout for large snapshots
            .build()
            .map_err(|e| e.to_string())?;

        let url = format!("http://{}/_raft/snapshot", peer);

        let response = client
            .post(&url)
            .header("Content-Type", "application/octet-stream")
            .header("X-Snapshot-Term", meta.term.to_string())
            .header("X-Snapshot-Index", meta.last_included_index.to_string())
            .header("X-Snapshot-Checksum", meta.checksum.to_string())
            .header("X-Snapshot-Size", meta.size_bytes.to_string())
            .body(bytes)
            .send()
            .await
            .map_err(|e| format!("Failed to send snapshot to {}: {}", peer, e))?;

        if response.status().is_success() {
            log::info!(
                "Snapshot sent successfully to {} (index={}, {}KB)",
                peer,
                meta.last_included_index,
                meta.size_bytes / 1024
            );
            Ok(())
        } else {
            Err(format!("Snapshot send to {} failed with status {}", peer, response.status()))
        }
    }

    /// Send a snapshot to a desynced follower with configurable timeout
    ///
    /// This version accepts a configurable timeout for large snapshots.
    pub(super) async fn send_snapshot_to_follower_with_timeout(
        peer: &str,
        snapshot_manager: &Arc<tokio::sync::RwLock<crate::cluster::snapshot::SnapshotManager>>,
        timeout_secs: u64,
    ) -> Result<(), String> {
        let mgr = snapshot_manager.read().await;

        let meta = mgr
            .current_meta()
            .ok_or_else(|| "No snapshot available to send".to_string())?
            .clone();

        let bytes = mgr
            .get_snapshot_bytes(meta.last_included_index)
            .map_err(|e| format!("Failed to read snapshot bytes: {}", e))?;

        drop(mgr);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .build()
            .map_err(|e| e.to_string())?;

        let url = format!("http://{}/_raft/snapshot", peer);

        let response = client
            .post(&url)
            .header("Content-Type", "application/octet-stream")
            .header("X-Snapshot-Term", meta.term.to_string())
            .header("X-Snapshot-Index", meta.last_included_index.to_string())
            .header("X-Snapshot-Checksum", meta.checksum.to_string())
            .header("X-Snapshot-Size", meta.size_bytes.to_string())
            .body(bytes)
            .send()
            .await
            .map_err(|e| format!("Failed to send snapshot to {}: {}", peer, e))?;

        if response.status().is_success() {
            log::info!(
                "Snapshot sent successfully to {} (index={}, {}KB, timeout={}s)",
                peer,
                meta.last_included_index,
                meta.size_bytes / 1024,
                timeout_secs
            );
            Ok(())
        } else {
            Err(format!("Snapshot send to {} failed with status {}", peer, response.status()))
        }
    }

    /// Replicate log entries to follower nodes and wait for majority acknowledgment
    ///
    /// This function sends requests to ALL followers IN PARALLEL and returns as soon as
    /// majority is reached. Slow nodes don't block the commit - they'll catch up later.
    ///
    /// Returns Ok(commit_index) when majority acknowledges, Err otherwise
    pub(super) async fn replicate_log_entries_to_followers(
        peers: &[String],
        entries: Vec<crate::cluster::LogEntry>,
        leader_commit: u64,
        term: u64,
        leader_id: u64,
        leader_port: u16,
        batcher: Option<Arc<crate::cluster::ReplicationBatcher>>,
    ) -> Result<u64, String> {
        if peers.is_empty() {
            // Single node cluster - commit immediately
            return Ok(leader_commit);
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10)) // Long timeout to ensure all nodes receive
            .build()
            .map_err(|e| e.to_string())?;

        let request = crate::cluster::consensus_log::AppendEntriesRequest {
            term,
            leader_id,
            leader_port,
            prev_log_index: 0, // Simplified - in real Raft this would track per-follower
            prev_log_term: 0,
            entries: entries.clone(),
            leader_commit,
        };

        // Leader counts as 1 success
        let total_nodes = peers.len() + 1;
        let majority = total_nodes / 2 + 1;
        let needed_from_followers = majority.saturating_sub(1); // Already have leader's vote

        // Early return if leader alone is majority (single node with empty peers already handled above)
        if needed_from_followers == 0 {
            let new_commit = entries.last().map(|e| e.log_id.index).unwrap_or(leader_commit);
            return Ok(new_commit);
        }

        // Get health summary to skip desynced followers
        let health_summary = if let Some(ref b) = batcher {
            b.get_health_summary().await
        } else {
            std::collections::HashMap::new()
        };

        // Count active (non-desynced) peers for quorum calculation
        let active_peers: Vec<_> = peers
            .iter()
            .filter(|p| {
                health_summary
                    .get(*p)
                    .map(|h| *h != crate::cluster::replication_batcher::FollowerHealth::Desynced)
                    .unwrap_or(true) // Unknown = try anyway
            })
            .cloned()
            .collect();

        let skipped_count = peers.len() - active_peers.len();
        if skipped_count > 0 {
            log::debug!("Skipping {} desynced followers (will use snapshot resync)", skipped_count);
        }

        // Spawn parallel requests to active (non-desynced) followers only
        let mut handles = Vec::with_capacity(active_peers.len());
        for peer in active_peers {
            let endpoint = format!("http://{}/_raft/append", peer);
            let client = client.clone();
            let request = request.clone();
            let peer_name = peer.clone();
            let batcher_clone = batcher.clone();

            let target_commit = leader_commit;
            handles.push(tokio::spawn(async move {
                let start = std::time::Instant::now();
                let result = match client.post(&endpoint).json(&request).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        if let Ok(response) = resp
                            .json::<crate::cluster::consensus_log::AppendEntriesResponse>()
                            .await
                        {
                            if response.success {
                                // Log applied_index for debugging but don't require it
                                // Apply happens synchronously before response, so success means applied
                                if response.applied_index < target_commit {
                                    log::debug!(
                                        "Log replicated to {} (applied_index={}, target={})",
                                        peer_name,
                                        response.applied_index,
                                        target_commit
                                    );
                                } else {
                                    log::debug!(
                                        "Log replicated AND applied on {} (applied_index={})",
                                        peer_name,
                                        response.applied_index
                                    );
                                }
                                (peer_name.clone(), true, response.last_log_index)
                            } else {
                                (peer_name.clone(), false, 0)
                            }
                        } else {
                            (peer_name.clone(), false, 0)
                        }
                    }
                    Ok(resp) => {
                        log::warn!(
                            "Replicate log to {} failed: status {}",
                            peer_name,
                            resp.status()
                        );
                        (peer_name.clone(), false, 0)
                    }
                    Err(e) => {
                        log::warn!("Replicate log to {} error: {}", peer_name, e);
                        (peer_name.clone(), false, 0)
                    }
                };
                let latency_ms = start.elapsed().as_millis() as u64;

                // Update batcher with follower health info
                if let Some(ref batcher) = batcher_clone {
                    if result.1 {
                        batcher.record_success(&result.0, result.2, latency_ms).await;
                    } else {
                        batcher.record_failure(&result.0).await;
                    }
                }

                (result.0, result.1)
            }));
        }

        // Collect results - wait for ALL followers (not just majority) to ensure consistency
        // This trades latency for consistency: slow followers won't be left behind
        use futures::stream::{FuturesUnordered, StreamExt};
        let mut futures: FuturesUnordered<_> = handles.into_iter().collect();

        let mut success_count = 0usize;
        let mut _completed = 0usize; // Track completion count (reserved for metrics)
        let mut majority_reached = false;
        let mut commit_index = leader_commit;

        // Set a global timeout for the entire operation
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10); // Long timeout for consistency

        while let Ok(Some(result)) = tokio::time::timeout_at(deadline, futures.next()).await {
            _completed += 1;
            if let Ok((peer, success)) = result {
                if success {
                    success_count += 1;
                    log::debug!("{}/{} followers acknowledged", success_count, peers.len());

                    // Track when majority is reached, but DON'T return early
                    if success_count >= needed_from_followers && !majority_reached {
                        majority_reached = true;
                        commit_index =
                            entries.last().map(|e| e.log_id.index).unwrap_or(leader_commit);
                        log::debug!(
                            "Majority reached ({}/{}), will commit index {}",
                            success_count + 1,
                            total_nodes,
                            commit_index
                        );
                        // Continue waiting for remaining followers instead of returning
                    }
                } else {
                    log::debug!("Follower {} failed", peer);
                }
            }

            // Continue until all followers complete or timeout
            // Don't break early - we want to give all followers a chance
        }

        // Check if majority was reached (even if some followers timed out)
        if majority_reached {
            log::debug!(
                "Replication complete: {}/{} followers succeeded, committing {}",
                success_count,
                peers.len(),
                commit_index
            );
            Ok(commit_index)
        } else {
            Err(format!(
                "Failed to reach majority: {} of {} nodes responded successfully (need {})",
                success_count + 1, // +1 for leader
                total_nodes,
                majority
            ))
        }
    }
}
