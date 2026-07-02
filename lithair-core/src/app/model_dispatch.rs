//! Model request dispatch: routes `/api/{model}` traffic to the registered
//! `ModelHandler`, including leader write-redirection bookkeeping.

use super::*;
use anyhow::Result;
use bytes::Bytes;

impl LithairServer {
    /// Handle model request
    ///
    /// In cluster mode, write operations go through the Raft consensus log:
    /// 1. Leader appends operation to log
    /// 2. Leader replicates to followers (synchronous, waits for majority)
    /// 3. After majority acknowledgment, operation is committed
    /// 4. All nodes (including leader) apply committed entries in order
    ///
    /// In single-node mode, operations are applied directly without logging.
    pub(super) async fn handle_model_request(
        &self,
        req: hyper::Request<hyper::body::Incoming>,
        model: &ModelRegistration,
    ) -> Result<RouteResponse> {
        // Extract path segments after base_path (clone path first to avoid borrow issues)
        let path = req.uri().path().to_string();
        let method = req.method().clone();
        let segments: Vec<&str> = path
            .strip_prefix(&model.base_path)
            .unwrap_or("")
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        // Check if this is a write operation
        let is_create = method == hyper::Method::POST && segments.is_empty();
        let is_bulk_create = method == hyper::Method::POST && segments.first() == Some(&"_bulk");
        let is_update = (method == hyper::Method::PUT || method == hyper::Method::PATCH)
            && !segments.is_empty();
        let is_delete = method == hyper::Method::DELETE && !segments.is_empty();
        let is_write = is_create || is_bulk_create || is_update || is_delete;

        // Extract the resource ID for UPDATE and DELETE operations
        let resource_id =
            if is_update || is_delete { segments.first().map(|s| s.to_string()) } else { None };

        // ==================== CLUSTER MODE WITH CONSENSUS LOG ====================
        // If we have a consensus log (cluster mode), write operations go through Raft
        if is_write && !self.cluster_peers.is_empty() {
            if let Some(ref consensus_log_ref) = self.consensus_log {
                log::debug!(
                    "CLUSTER MODE: {} {} (create={}, update={}, delete={})",
                    method,
                    path,
                    is_create,
                    is_update,
                    is_delete
                );

                // Check if we are the leader
                let is_leader = self.raft_state.as_ref().map(|s| s.is_leader()).unwrap_or(false);

                if !is_leader {
                    // Redirect to leader (if we know the leader's port)
                    if let Some(ref raft_state) = self.raft_state {
                        let leader_port = raft_state.get_leader_port();
                        if leader_port == 0 {
                            // Leader port not yet discovered (haven't received first heartbeat).
                            // Return 503 instead of redirecting to port 0.
                            return Ok(Self::leader_port_unknown_503());
                        }
                        return Ok(hyper::Response::builder()
                        .status(307) // Temporary Redirect
                        .header("Location", format!("http://127.0.0.1:{}{}", leader_port, path))
                        .header("X-Raft-Leader", format!("{}", leader_port))
                        .body(boxed_full(Bytes::from(
                            serde_json::json!({
                                "error": "Not leader",
                                "leader_port": leader_port
                            })
                            .to_string(),
                        )))
                        .expect("valid HTTP response"));
                    }
                }

                // We are the leader - process through consensus log
                let consensus_log = consensus_log_ref;

                // Read request body for write operations
                use http_body_util::BodyExt;
                let (_parts, body) = req.into_parts();
                let body_bytes = body.collect().await?.to_bytes();
                // CREATE/UPDATE consume the body: it must parse as a JSON
                // OBJECT. Anything else is a 400 — a non-object (`42`, `[..]`)
                // would PANIC in the `data["id"] = …` IndexMut inserts below,
                // and malformed JSON silently became an empty entity via the
                // old `unwrap_or(Null)` (both found by review on #164/#166).
                // DELETE ignores the body entirely (typically empty), so it
                // gets no such constraint.
                let body_json: serde_json::Value = if is_create || is_update {
                    match serde_json::from_slice(&body_bytes) {
                        Ok(v @ serde_json::Value::Object(_)) => v,
                        Ok(_) => {
                            return Ok(response::json(
                                StatusCode::BAD_REQUEST,
                                r#"{"error":"Request body must be a JSON object"}"#,
                            ));
                        }
                        Err(e) => {
                            return Ok(response::json_value(
                                StatusCode::BAD_REQUEST,
                                &serde_json::json!({
                                    "error": format!("Invalid JSON in request body: {}", e)
                                }),
                            ));
                        }
                    }
                } else {
                    serde_json::Value::Null
                };

                // Create the CRUD operation
                // For CREATE operations: generate ID on leader to ensure all nodes have same ID
                // Also inject timestamps to ensure consistency across all nodes
                let now = chrono::Utc::now().to_rfc3339();
                let operation = if is_create {
                    let mut data = body_json.clone();
                    // Generate ID on leader if not provided, so followers get the same ID
                    if data.get("id").is_none() || data.get("id") == Some(&serde_json::Value::Null)
                    {
                        data["id"] = serde_json::Value::String(uuid::Uuid::new_v4().to_string());
                    }
                    // Add timestamps for consistency across all nodes
                    if data.get("created_at").is_none() {
                        data["created_at"] = serde_json::Value::String(now.clone());
                    }
                    if data.get("updated_at").is_none() {
                        data["updated_at"] = serde_json::Value::String(now.clone());
                    }
                    crate::cluster::CrudOperation::Create {
                        model_path: model.base_path.clone(),
                        data,
                    }
                } else if is_update {
                    let id = resource_id.clone().unwrap_or_default();
                    log::debug!("CLUSTER: Creating UPDATE operation for id={}", id);
                    // For UPDATE: merge delta with existing item to send complete object
                    // This ensures followers can deserialize the full item
                    let existing = model.handler.get_item_json(&id).await;
                    let mut merged_data = if let Some(mut existing_json) = existing {
                        // Merge delta into existing (delta overwrites existing fields)
                        if let Some(obj) = existing_json.as_object_mut() {
                            if let Some(delta_obj) = body_json.as_object() {
                                for (key, value) in delta_obj {
                                    obj.insert(key.clone(), value.clone());
                                }
                            }
                        }
                        existing_json
                    } else {
                        // Item doesn't exist - use delta as-is (will likely fail on follower too)
                        body_json.clone()
                    };
                    // Always update the updated_at timestamp for consistency
                    merged_data["updated_at"] = serde_json::Value::String(now);
                    crate::cluster::CrudOperation::Update {
                        model_path: model.base_path.clone(),
                        id,
                        data: merged_data,
                    }
                } else if is_delete {
                    let id = resource_id.clone().unwrap_or_default();
                    log::debug!("CLUSTER: Creating DELETE operation for id={}", id);
                    crate::cluster::CrudOperation::Delete {
                        model_path: model.base_path.clone(),
                        id,
                    }
                } else {
                    // Bulk create - currently handled as a single operation
                    // Note: Proper BatchOperation support is not yet available
                    let mut data = body_json.clone();
                    if data.get("id").is_none() || data.get("id") == Some(&serde_json::Value::Null)
                    {
                        data["id"] = serde_json::Value::String(uuid::Uuid::new_v4().to_string());
                    }
                    // Add timestamps for consistency across all nodes
                    if data.get("created_at").is_none() {
                        data["created_at"] = serde_json::Value::String(now.clone());
                    }
                    if data.get("updated_at").is_none() {
                        data["updated_at"] = serde_json::Value::String(now);
                    }
                    crate::cluster::CrudOperation::Create {
                        model_path: model.base_path.clone(),
                        data,
                    }
                };

                // ── WRITE PATH: CONSENSUS + REPLICATION ────────────────────
                //
                // The write path for a cluster leader follows this sequence:
                //
                //   1. Append to ConsensusLog (in-memory, atomic index assignment)
                //   2. Queue for ReplicationBatcher (follower health tracking)
                //   3. In parallel:
                //      a. Write to WAL (disk durability, group commit batches fsync)
                //      b. Send this single entry to all followers via HTTP
                //   4. Wait for majority acknowledgment (quorum = peers/2 + 1)
                //   5. Commit: update commit_index in the ConsensusLog
                //   6. Apply: execute the operation on the local state machine
                //   7. Fire-and-forget: notify remaining followers of new commit_index
                //
                // The background catch-up task (100ms interval) handles lagging
                // followers by sending only their missing entries using per-follower
                // match_index tracking.

                // Step 1: Append to local consensus log (in-memory, fast)
                let log_entry = consensus_log.append(operation.clone()).await;
                let entry_index = log_entry.log_id.index;
                let term = consensus_log.current_term();
                let node_id = self.node_id.unwrap_or(0);
                log::debug!("Appended to log: index={}, term={}", entry_index, term);

                // Step 2: Queue for batcher (for lagging followers tracking)
                if let Some(ref batcher) = self.replication_batcher {
                    batcher.queue_entry(log_entry.clone()).await;
                }

                // Step 3: PARALLEL - WAL durability + Replication to followers
                // We use tokio::join! to run both concurrently and wait for both to complete.
                // This reduces latency since WAL fsync and network I/O happen simultaneously.
                let wal_clone = self.wal.clone();
                let log_entry_clone = log_entry.clone();
                let peers_clone = self.cluster_peers.clone();
                let port_clone = self.config.server.port;
                let batcher_clone = self.replication_batcher.clone();

                // WAL write task (uses group commit for batching)
                let wal_future = async {
                    if let Some(ref wal) = wal_clone {
                        // Use buffered append for group commit (higher throughput)
                        wal.append_buffered(&log_entry_clone).await
                    } else {
                        Ok(())
                    }
                };

                // Replication task (returns when majority responds)
                // Send only the new entry to followers. The background catch-up task
                // handles lagging followers using per-follower match_index tracking.
                let replication_future = async move {
                    let entries_to_send = vec![log_entry.clone()];

                    if entries_to_send.is_empty() {
                        return Ok(entry_index);
                    }

                    log::debug!(
                        "Replicating {} entries (window {} to {}), target_commit={}",
                        entries_to_send.len(),
                        entries_to_send.first().map(|e| e.log_id.index).unwrap_or(0),
                        entries_to_send.last().map(|e| e.log_id.index).unwrap_or(0),
                        entry_index
                    );

                    Self::replicate_log_entries_to_followers(
                        &peers_clone,
                        entries_to_send,
                        entry_index, // Commit up to this entry if majority responds
                        term,
                        node_id,
                        port_clone,
                        batcher_clone,
                    )
                    .await
                };

                // Run WAL and replication in parallel
                let (wal_result, replication_result) = tokio::join!(wal_future, replication_future);

                // Check WAL result first (must succeed for durability)
                if let Err(e) = wal_result {
                    log::error!("WAL write failed: {}", e);
                    return Ok(response::json_value(
                        StatusCode::SERVICE_UNAVAILABLE,
                        &serde_json::json!({"error": format!("WAL write failed: {}", e)}),
                    ));
                }
                log::debug!("WAL entry durable: index={}", entry_index);

                // Check replication result
                match replication_result {
                    Ok(new_commit_index) => {
                        // Step 4: Commit the entry (majority achieved)
                        consensus_log.commit(new_commit_index);
                        log::debug!("Committed index: {}", new_commit_index);

                        // Step 4.5: Send commit notification to followers IN PARALLEL (fire-and-forget)
                        // Include the window of entries so followers get both data and commit in one shot
                        let peers_for_notify = self.cluster_peers.clone();
                        let commit_index_to_notify = new_commit_index;
                        let term_for_notify = term;
                        let node_id_for_notify = node_id;
                        let port_for_notify = self.config.server.port;
                        let notify_prev_log_index = entry_index;
                        let notify_prev_log_term = term;
                        tokio::spawn(async move {
                            // Send commit notification with leader's log position
                            // so followers can detect divergence.
                            let client = reqwest::Client::builder()
                                .timeout(std::time::Duration::from_secs(1))
                                .build()
                                .ok();
                            if let Some(client) = client {
                                let request = crate::cluster::consensus_log::AppendEntriesRequest {
                                    term: term_for_notify,
                                    leader_id: node_id_for_notify,
                                    leader_port: port_for_notify,
                                    prev_log_index: notify_prev_log_index,
                                    prev_log_term: notify_prev_log_term,
                                    entries: vec![], // Commit notification only, catch-up via background task
                                    leader_commit: commit_index_to_notify,
                                };
                                // Send to ALL peers IN PARALLEL
                                let futures: Vec<_> = peers_for_notify
                                    .iter()
                                    .map(|peer| {
                                        let endpoint = format!("http://{}/_raft/append", peer);
                                        let client = client.clone();
                                        let request = request.clone();
                                        async move {
                                            let _ =
                                                client.post(&endpoint).json(&request).send().await;
                                        }
                                    })
                                    .collect();
                                futures::future::join_all(futures).await;
                            }
                        });

                        // Step 5: Apply to local state machine
                        // CRITICAL: Wait for all earlier entries to be applied first.
                        // Without this, entries can be applied out of order when commits happen
                        // out of order, causing data inconsistency (e.g., DELETE before CREATE).
                        //
                        // Example race without this fix:
                        // 1. Entry 100 (CREATE X) appended, replication starts
                        // 2. Entry 101 (DELETE X) appended, replication starts
                        // 3. Entry 101 replication completes, commits 101
                        // 4. Entry 101 applies (DELETE X - but X doesn't exist yet!)
                        // 5. Entry 100 replication completes, commits 100
                        // 6. Entry 100 applies (CREATE X - X now exists!)
                        // Result: Leader has X, but followers applied in correct order (no X)

                        // Wait for earlier entries to be COMMITTED first
                        // This handles the case where entry N+1 commits before entry N
                        // (due to faster replication). We must wait for N to commit before applying N+1.
                        let expected_prior = entry_index.saturating_sub(1);
                        let mut commit_waited = 0u32;
                        while consensus_log.commit_index() < expected_prior {
                            if commit_waited > 50000 {
                                // 50000 * 100µs = 5 seconds max wait for commit
                                log::error!(
                                    "Waited 5s for earlier entry {} to commit (current commit={})",
                                    expected_prior,
                                    consensus_log.commit_index()
                                );
                                // Return error - something is seriously wrong if commit takes this long
                                return Ok(response::json_value(
                                    StatusCode::SERVICE_UNAVAILABLE,
                                    &serde_json::json!({
                                        "error": format!(
                                            "Commit ordering timeout: entry {} waiting for {}",
                                            entry_index, expected_prior
                                        )
                                    }),
                                ));
                            }
                            tokio::time::sleep(std::time::Duration::from_micros(100)).await;
                            commit_waited += 1;
                        }

                        // Now wait for earlier entry to be APPLIED
                        // Once it's committed, its handler will apply it (no timeout - it WILL apply)
                        let mut apply_waited = 0u32;
                        while consensus_log.applied_index() < expected_prior {
                            if apply_waited > 100000 {
                                // 100000 * 100µs = 10 seconds max wait for apply
                                // This should never happen if commit succeeded - log but continue waiting
                                log::warn!(
                                    "Slow apply: entry {} waiting for {} (commit={}, applied={})",
                                    entry_index,
                                    expected_prior,
                                    consensus_log.commit_index(),
                                    consensus_log.applied_index()
                                );
                                apply_waited = 0; // Reset counter to keep waiting
                            }
                            tokio::time::sleep(std::time::Duration::from_micros(100)).await;
                            apply_waited += 1;
                        }

                        // Now safe to acquire lock and apply
                        let _apply_guard = consensus_log.lock_apply().await;

                        // Now apply our entry
                        match self.apply_crud_operation(&operation).await {
                            Ok(result) => {
                                consensus_log.mark_applied(entry_index);
                                // _apply_guard dropped here
                                let response_body = serde_json::to_vec(&result).unwrap_or_default();
                                return Ok(hyper::Response::builder()
                                    .status(if is_create { 201 } else { 200 })
                                    .header("Content-Type", "application/json")
                                    .header("X-Raft-Index", entry_index.to_string())
                                    .body(boxed_full(Bytes::from(response_body)))
                                    .expect("valid HTTP response"));
                            }
                            Err(e) => {
                                log::error!("Failed to apply operation: {}", e);
                                return Ok(response::json_value(
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    &serde_json::json!({
                                        "error": format!("Apply failed: {}", e)
                                    }),
                                ));
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to replicate: {}", e);
                        return Ok(hyper::Response::builder()
                        .status(503) // Service Unavailable
                        .body(boxed_full(Bytes::from(
                            serde_json::json!({
                                "error": format!("Replication failed: {}", e)
                            })
                            .to_string(),
                        )))
                        .expect("valid HTTP response"));
                    }
                }
            }
        }

        // ==================== SINGLE-NODE MODE OR READ OPERATIONS ====================
        // No cluster or read operation - delegate directly to model handler
        match model.handler.handle_request(req, &segments).await {
            Ok(resp) => {
                // Pass BoxBody through directly — no collection.
                // This is critical for SSE streams (issue #93): collecting
                // the body would buffer the entire infinite stream before
                // sending anything to the client.
                Ok(resp)
            }
            Err(_) => Ok(response::json(
                StatusCode::INTERNAL_SERVER_ERROR,
                r#"{"error":"Internal error"}"#,
            )),
        }
    }

    // NOTE: The old fire-and-forget replication methods (replicate_to_followers,
    // replicate_update_to_followers, replicate_delete_to_followers) have been removed.
    // They were replaced by the Raft consensus log approach which guarantees ordering.
    // See: replicate_log_entries_to_followers() and handle_raft_append_entries()
}
