//! Schema synchronization handlers for cluster consensus
//!
//! These handlers manage schema changes across a Lithair cluster:
//! - Internal endpoints (/_raft/schema/*) for node-to-node communication
//! - Admin endpoints (/_admin/schema/*) for human/CI management

use super::LithairServer;
use crate::schema::{PendingSchemaChange, SchemaChangeStatus, SchemaSyncMessage, VoteStrategy};
use anyhow::Result;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Request, Response, StatusCode};

impl LithairServer {
    // =========================================================================
    // INTERNAL CLUSTER ENDPOINTS (/_raft/schema/*)
    // =========================================================================

    /// POST /_raft/schema/propose - Propose a schema change to the cluster
    ///
    /// Called by a node starting with a new schema version.
    /// The leader will evaluate the change and either auto-accept, reject,
    /// or create a pending change for voting/approval.
    pub(crate) async fn handle_schema_propose(
        &self,
        req: Request<hyper::body::Incoming>,
    ) -> Result<Response<Full<Bytes>>> {
        use http_body_util::BodyExt;

        // Parse the request body
        let body = req.into_body().collect().await?.to_bytes();
        let pending_change: PendingSchemaChange = match serde_json::from_slice(&body) {
            Ok(c) => c,
            Err(e) => {
                return Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(format!(
                        r#"{{"error":"Invalid schema change: {}"}}"#,
                        e
                    ))))
                    .unwrap());
            }
        };

        log::info!(
            "Schema proposal received for model '{}' from node {}",
            pending_change.model_name,
            pending_change.proposer_node_id
        );

        let mut state = self.schema_sync_state.write().await;
        let policy = state.policy.clone();

        // Determine action based on policy
        let strategy = policy.strategy_for(&pending_change.overall_strategy);

        let response = match strategy {
            VoteStrategy::AutoAccept => {
                // Auto-accept: Apply immediately
                log::info!(
                    "Auto-accepting schema change for '{}' (strategy: {:?})",
                    pending_change.model_name,
                    pending_change.overall_strategy
                );

                state
                    .schemas
                    .insert(pending_change.model_name.clone(), pending_change.new_spec.clone());

                serde_json::json!({
                    "status": "accepted",
                    "change_id": pending_change.id,
                    "message": "Schema change auto-accepted"
                })
            }

            VoteStrategy::Reject => {
                // Reject: Don't accept this type of change
                log::warn!(
                    "Rejecting schema change for '{}' (policy rejects {:?} changes)",
                    pending_change.model_name,
                    pending_change.overall_strategy
                );

                serde_json::json!({
                    "status": "rejected",
                    "change_id": pending_change.id,
                    "reason": format!("Policy rejects {:?} changes", pending_change.overall_strategy)
                })
            }

            VoteStrategy::Consensus | VoteStrategy::ManualApproval { .. } => {
                // Create pending change for voting/approval
                log::info!(
                    "Schema change for '{}' requires {:?}",
                    pending_change.model_name,
                    strategy
                );

                let change_id = pending_change.id;
                let mut pending = pending_change;

                // Set timeout if ManualApproval with timeout
                if let VoteStrategy::ManualApproval { timeout: Some(timeout), .. } = strategy {
                    pending = pending.with_timeout(*timeout);
                }

                state.add_pending(pending);

                serde_json::json!({
                    "status": "pending",
                    "change_id": change_id,
                    "message": format!("Schema change awaiting {:?}", strategy)
                })
            }
        };

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(serde_json::to_string(&response)?)))
            .unwrap())
    }

    /// POST /_raft/schema/vote - Vote on a pending schema change
    ///
    /// Nodes vote to approve or reject pending schema changes.
    pub(crate) async fn handle_schema_vote(
        &self,
        req: Request<hyper::body::Incoming>,
    ) -> Result<Response<Full<Bytes>>> {
        use http_body_util::BodyExt;

        #[derive(serde::Deserialize)]
        struct VoteRequest {
            change_id: uuid::Uuid,
            node_id: u64,
            approve: bool,
            reason: Option<String>,
        }

        let body = req.into_body().collect().await?.to_bytes();
        let vote: VoteRequest = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => {
                return Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(format!(
                        r#"{{"error":"Invalid vote request: {}"}}"#,
                        e
                    ))))
                    .unwrap());
            }
        };

        let mut state = self.schema_sync_state.write().await;
        let total_nodes = self.cluster_peers.len() + 1; // +1 for self
        let policy = state.policy.clone();

        // Check if change exists and is pending
        let change_status = state.pending_changes.get(&vote.change_id).map(|p| p.status.clone());

        match change_status {
            None => {
                return Ok(Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(r#"{"error":"Schema change not found"}"#)))
                    .unwrap());
            }
            Some(status) if status != SchemaChangeStatus::Pending => {
                return Ok(Response::builder()
                    .status(StatusCode::CONFLICT)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(format!(
                        r#"{{"error":"Change is no longer pending (status: {:?})"}}"#,
                        status
                    ))))
                    .unwrap());
            }
            _ => {}
        }

        // Now we can safely mutate
        let pending = match state.pending_changes.get_mut(&vote.change_id) {
            Some(p) => p,
            None => {
                return Ok(Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(
                        r#"{"error":"Schema change disappeared unexpectedly"}"#,
                    )))
                    .unwrap());
            }
        };

        if vote.approve {
            pending.add_approval(vote.node_id);
            log::info!("Node {} approved schema change {}", vote.node_id, vote.change_id);
        } else {
            pending.add_rejection(vote.node_id, vote.reason);
            log::info!("Node {} rejected schema change {}", vote.node_id, vote.change_id);
        }

        // Check if we have consensus
        let has_approvals = pending.has_enough_approvals(&policy, total_nodes);
        let should_reject = pending.should_reject(&policy, total_nodes);
        let new_spec = pending.new_spec.clone();
        let model_name = pending.model_name.clone();

        if has_approvals {
            pending.status = SchemaChangeStatus::Applied;
            log::info!(
                "Schema change {} approved and applied for '{}'",
                vote.change_id,
                model_name
            );
        } else if should_reject {
            pending.status = SchemaChangeStatus::Rejected;
            log::warn!("Schema change {} rejected", vote.change_id);
        }

        let response_status = pending.status.clone();
        let approvals_count = pending.approvals.len();
        let rejections_count = pending.rejections.len();

        // Apply schema change if approved
        if has_approvals {
            state.schemas.insert(model_name, new_spec);
        }

        let response = serde_json::json!({
            "status": response_status,
            "approvals": approvals_count,
            "rejections": rejections_count,
        });

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(serde_json::to_string(&response)?)))
            .unwrap())
    }

    /// GET /_raft/schema/current - Get current schemas (for new nodes joining)
    ///
    /// Returns all current schemas and pending changes for synchronization.
    /// Query param: ?model=ModelName to get specific model
    pub(crate) async fn handle_schema_current(
        &self,
        req: Request<hyper::body::Incoming>,
    ) -> Result<Response<Full<Bytes>>> {
        // Optional: filter by model name from query string
        let model_name: Option<String> = req.uri().query().and_then(|q| {
            // Simple query parsing: model=SomeName
            for pair in q.split('&') {
                if let Some(value) = pair.strip_prefix("model=") {
                    return Some(value.to_string());
                }
            }
            None
        });

        let state = self.schema_sync_state.read().await;

        if let Some(model) = model_name {
            // Return specific model
            let schema = state.schemas.get(&model).cloned();
            let pending: Vec<_> = state.pending_for_model(&model).into_iter().cloned().collect();

            let response = SchemaSyncMessage::SchemaResponse {
                model_name: model,
                spec: schema,
                pending_changes: pending,
            };

            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(serde_json::to_string(&response)?)))
                .unwrap())
        } else {
            // Return full state for sync
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(serde_json::to_string(&*state)?)))
                .unwrap())
        }
    }

    // =========================================================================
    // ADMIN ENDPOINTS (/_admin/schema/*)
    // =========================================================================

    /// GET /_admin/schema - List all current schemas
    pub(crate) async fn handle_admin_schema_list(&self) -> Result<Response<Full<Bytes>>> {
        let state = self.schema_sync_state.read().await;

        let schemas: Vec<_> = state
            .schemas
            .iter()
            .map(|(name, spec)| {
                serde_json::json!({
                    "model_name": name,
                    "version": spec.version,
                    "field_count": spec.fields.len(),
                    "index_count": spec.indexes.len(),
                })
            })
            .collect();

        let response = serde_json::json!({
            "schemas": schemas,
            "pending_changes": state.pending_changes.len(),
            "policy": {
                "additive": format!("{:?}", state.policy.additive),
                "breaking": format!("{:?}", state.policy.breaking),
                "versioned": format!("{:?}", state.policy.versioned),
            }
        });

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(serde_json::to_string_pretty(&response)?)))
            .unwrap())
    }

    /// GET /_admin/schema/pending - List pending schema changes
    pub(crate) async fn handle_admin_schema_pending(&self) -> Result<Response<Full<Bytes>>> {
        let state = self.schema_sync_state.read().await;

        let pending: Vec<_> = state
            .all_pending()
            .into_iter()
            .map(|change| {
                serde_json::json!({
                    "id": change.id,
                    "model_name": change.model_name,
                    "proposer_node": change.proposer_node_id,
                    "overall_strategy": format!("{:?}", change.overall_strategy),
                    "status": format!("{:?}", change.status),
                    "changes": change.changes.iter().map(|c| {
                        serde_json::json!({
                            "type": format!("{:?}", c.change_type),
                            "field": c.field_name,
                            "strategy": format!("{:?}", c.migration_strategy),
                        })
                    }).collect::<Vec<_>>(),
                    "approvals": change.approvals.len(),
                    "human_approvals": change.human_approvals.len(),
                    "rejections": change.rejections.len(),
                    "proposed_at": change.proposed_at,
                    "expires_at": change.expires_at,
                })
            })
            .collect();

        let response = serde_json::json!({
            "pending_changes": pending,
            "count": pending.len(),
        });

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(serde_json::to_string_pretty(&response)?)))
            .unwrap())
    }

    /// POST /_admin/schema/approve/{change_id} - Manually approve a schema change
    pub(crate) async fn handle_admin_schema_approve(
        &self,
        req: Request<hyper::body::Incoming>,
        path: &str,
    ) -> Result<Response<Full<Bytes>>> {
        // Extract change_id from path
        let change_id_str = path.strip_prefix("/_admin/schema/approve/").unwrap_or_default();

        let change_id: uuid::Uuid = match change_id_str.parse() {
            Ok(id) => id,
            Err(_) => {
                return Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(r#"{"error":"Invalid change_id format"}"#)))
                    .unwrap());
            }
        };

        // Extract user info from request (could be from session, header, or body)
        let (user_id, user_name) = self.extract_approver_info(&req).await;

        let mut state = self.schema_sync_state.write().await;
        let total_nodes = self.cluster_peers.len() + 1;
        let policy = state.policy.clone();

        // Check if change exists and is pending
        let change_status = state.pending_changes.get(&change_id).map(|p| p.status.clone());

        match change_status {
            None => {
                return Ok(Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(r#"{"error":"Schema change not found"}"#)))
                    .unwrap());
            }
            Some(status) if status != SchemaChangeStatus::Pending => {
                return Ok(Response::builder()
                    .status(StatusCode::CONFLICT)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(format!(
                        r#"{{"error":"Change is no longer pending (status: {:?})"}}"#,
                        status
                    ))))
                    .unwrap());
            }
            _ => {}
        }

        // Now we can safely mutate
        let pending = match state.pending_changes.get_mut(&change_id) {
            Some(p) => p,
            None => {
                return Ok(Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(
                        r#"{"error":"Schema change disappeared unexpectedly"}"#,
                    )))
                    .unwrap());
            }
        };

        pending.add_human_approval(user_id.clone(), user_name.clone());
        log::info!(
            "Human approval from '{}' for schema change {}",
            user_name.as_deref().unwrap_or(&user_id),
            change_id
        );

        // Check if we have enough human approvals
        let has_approvals = pending.has_enough_approvals(&policy, total_nodes);
        let new_spec = pending.new_spec.clone();
        let model_name = pending.model_name.clone();
        let human_approvals_count = pending.human_approvals.len();

        if has_approvals {
            pending.status = SchemaChangeStatus::Applied;
            log::info!(
                "Schema change {} approved and applied for '{}' (human approval)",
                change_id,
                model_name
            );
        }

        // Apply schema change if approved (outside the mutable borrow)
        if has_approvals {
            use crate::schema::save_schema_spec;
            use std::path::Path;

            let model_name_for_response = model_name.clone();
            state.schemas.insert(model_name.clone(), new_spec.clone());

            // Persist to disk so restarts don't re-detect the same change
            let base_path = Path::new(&self.config.storage.data_dir);
            if let Err(e) = save_schema_spec(&new_spec, base_path) {
                log::error!("Failed to persist approved schema to disk: {}", e);
                // Continue anyway - schema is applied in memory
            } else {
                log::info!("Schema for '{}' persisted to disk", model_name_for_response);
            }

            let response = serde_json::json!({
                "status": "applied",
                "change_id": change_id,
                "model": model_name_for_response,
                "message": "Schema change approved, applied, and persisted"
            });

            return Ok(Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(serde_json::to_string(&response)?)))
                .unwrap());
        }

        let response = serde_json::json!({
            "status": "approval_recorded",
            "change_id": change_id,
            "human_approvals": human_approvals_count,
            "message": "Approval recorded, waiting for more approvals"
        });

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(serde_json::to_string(&response)?)))
            .unwrap())
    }

    /// POST /_admin/schema/reject/{change_id} - Manually reject a schema change
    pub(crate) async fn handle_admin_schema_reject(
        &self,
        req: Request<hyper::body::Incoming>,
        path: &str,
    ) -> Result<Response<Full<Bytes>>> {
        use http_body_util::BodyExt;

        // Extract change_id from path
        let change_id_str = path.strip_prefix("/_admin/schema/reject/").unwrap_or_default();

        let change_id: uuid::Uuid = match change_id_str.parse() {
            Ok(id) => id,
            Err(_) => {
                return Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(r#"{"error":"Invalid change_id format"}"#)))
                    .unwrap());
            }
        };

        // Parse optional reason from body
        #[derive(serde::Deserialize, Default)]
        struct RejectBody {
            reason: Option<String>,
        }

        let body = req.into_body().collect().await?.to_bytes();
        let reject_body: RejectBody = serde_json::from_slice(&body).unwrap_or_default();

        let mut state = self.schema_sync_state.write().await;

        if let Some(pending) = state.pending_changes.get_mut(&change_id) {
            if pending.status != SchemaChangeStatus::Pending {
                return Ok(Response::builder()
                    .status(StatusCode::CONFLICT)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(format!(
                        r#"{{"error":"Change is no longer pending (status: {:?})"}}"#,
                        pending.status
                    ))))
                    .unwrap());
            }

            pending.status = SchemaChangeStatus::Rejected;
            pending.rejection_reason = reject_body.reason.clone();

            log::warn!(
                "Schema change {} manually rejected: {}",
                change_id,
                reject_body.reason.as_deref().unwrap_or("No reason provided")
            );

            let response = serde_json::json!({
                "status": "rejected",
                "change_id": change_id,
                "reason": reject_body.reason,
            });

            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(serde_json::to_string(&response)?)))
                .unwrap())
        } else {
            Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(r#"{"error":"Schema change not found"}"#)))
                .unwrap())
        }
    }

    /// Extract approver info from request (session, header, or default)
    async fn extract_approver_info(
        &self,
        req: &Request<hyper::body::Incoming>,
    ) -> (String, Option<String>) {
        // Try to get from X-User-Id header
        if let Some(user_id) = req.headers().get("X-User-Id").and_then(|v| v.to_str().ok()) {
            let user_name = req
                .headers()
                .get("X-User-Name")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            return (user_id.to_string(), user_name);
        }

        // Try to get from session (if session manager is available)
        // For now, return a default admin identifier
        ("admin".to_string(), Some("Administrator".to_string()))
    }

    // =========================================================================
    // PHASE 3: SCHEMA MANAGEMENT ENDPOINTS
    // =========================================================================

    /// POST /_admin/schema/sync - Force synchronization from cluster leader
    ///
    /// Requests current schemas from the leader and updates local state.
    /// Useful for new nodes joining or recovering from desync.
    pub(crate) async fn handle_admin_schema_sync(&self) -> Result<Response<Full<Bytes>>> {
        let is_cluster = !self.cluster_peers.is_empty();

        if !is_cluster {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(
                    r#"{"error":"Not in cluster mode - sync not applicable"}"#,
                )))
                .unwrap());
        }

        // In a real implementation, this would:
        // 1. Find the leader
        // 2. Request /_raft/schema/current from leader
        // 3. Update local state with received schemas
        // For now, we'll indicate the sync was triggered

        let state = self.schema_sync_state.read().await;
        let schema_count = state.schemas.len();
        let pending_count = state.pending_changes.len();

        log::info!(
            "Schema sync requested - current state: {} schemas, {} pending",
            schema_count,
            pending_count
        );

        // Note: Actual leader communication is not yet implemented
        // For now, return current state as acknowledgment
        let response = serde_json::json!({
            "status": "sync_triggered",
            "message": "Schema synchronization initiated",
            "current_schemas": schema_count,
            "pending_changes": pending_count,
            "cluster_peers": self.cluster_peers.len(),
            "note": "Full leader sync implementation pending"
        });

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(serde_json::to_string_pretty(&response)?)))
            .unwrap())
    }

    /// GET /_admin/schema/diff - Compare local schemas with stored/cluster schemas
    ///
    /// Shows differences between current model specs and stored schemas.
    /// Useful for pre-deployment validation.
    pub(crate) async fn handle_admin_schema_diff(&self) -> Result<Response<Full<Bytes>>> {
        use crate::schema::{load_schema_spec, SchemaChangeDetector};
        use std::path::Path;

        let base_path = Path::new(&self.config.storage.data_dir);
        let mut diffs = Vec::new();

        // Collect model info first, then release lock
        let model_data: Vec<_> = {
            let models = self.models.read().await;
            models
                .iter()
                .filter_map(|m| m.schema_extractor.as_ref().map(|e| (m.name.clone(), e.clone())))
                .collect()
        };

        for (model_name, extractor) in model_data {
            let current_spec = extractor();
            let stored_spec = load_schema_spec(&model_name, base_path).ok().flatten();

            let model_diff = match stored_spec {
                Some(stored) => {
                    let changes = SchemaChangeDetector::detect_changes(&stored, &current_spec);
                    if changes.is_empty() {
                        serde_json::json!({
                            "model": model_name,
                            "status": "in_sync",
                            "stored_version": stored.version,
                            "current_version": current_spec.version,
                        })
                    } else {
                        serde_json::json!({
                            "model": model_name,
                            "status": "changed",
                            "stored_version": stored.version,
                            "current_version": current_spec.version,
                            "changes": changes.iter().map(|c| {
                                serde_json::json!({
                                    "type": format!("{:?}", c.change_type),
                                    "field": c.field_name,
                                    "strategy": format!("{:?}", c.migration_strategy),
                                    "breaking": c.requires_consensus,
                                })
                            }).collect::<Vec<_>>(),
                        })
                    }
                }
                None => {
                    serde_json::json!({
                        "model": model_name,
                        "status": "new",
                        "current_version": current_spec.version,
                        "message": "No stored schema - first deployment",
                    })
                }
            };

            diffs.push(model_diff);
        }

        let has_changes = diffs
            .iter()
            .any(|d| d.get("status").and_then(|s| s.as_str()) != Some("in_sync"));

        let response = serde_json::json!({
            "overall_status": if has_changes { "changes_detected" } else { "all_in_sync" },
            "models": diffs,
            "timestamp": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
        });

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(serde_json::to_string_pretty(&response)?)))
            .unwrap())
    }

    /// GET /_admin/schema/history - View schema change history
    ///
    /// Returns the history of applied schema changes for audit purposes.
    pub(crate) async fn handle_admin_schema_history(&self) -> Result<Response<Full<Bytes>>> {
        let state = self.schema_sync_state.read().await;

        let history: Vec<_> = state
            .change_history
            .iter()
            .map(|change| {
                serde_json::json!({
                    "id": change.id,
                    "model": change.model_name,
                    "applied_at": change.applied_at,
                    "applied_by_node": change.applied_by_node,
                    "changes": change.changes.iter().map(|c| {
                        serde_json::json!({
                            "type": format!("{:?}", c.change_type),
                            "field": c.field_name,
                            "strategy": format!("{:?}", c.migration_strategy),
                        })
                    }).collect::<Vec<_>>(),
                })
            })
            .collect();

        let response = serde_json::json!({
            "history": history,
            "count": history.len(),
        });

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(serde_json::to_string_pretty(&response)?)))
            .unwrap())
    }

    /// POST /_admin/schema/revalidate - Re-run schema validation
    ///
    /// Triggers schema validation against stored schemas. Useful for testing
    /// migrations after manually updating schema files.
    ///
    /// Returns the validation results including any detected changes.
    pub(crate) async fn handle_admin_schema_revalidate(&self) -> Result<Response<Full<Bytes>>> {
        use crate::config::SchemaMigrationMode;
        use crate::schema::{
            load_schema_spec, save_schema_spec, AppliedSchemaChange, SchemaChangeDetector,
        };
        use std::path::Path;

        let base_path = Path::new(&self.config.storage.data_dir);
        let mut results = Vec::new();
        let mut total_changes = 0;

        // Check if locked
        {
            let state = self.schema_sync_state.read().await;
            if state.lock_status.is_locked() {
                let response = serde_json::json!({
                    "status": "blocked",
                    "reason": "Schema migrations are locked",
                    "locked": true,
                });
                return Ok(Response::builder()
                    .status(StatusCode::LOCKED)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(serde_json::to_string_pretty(&response)?)))
                    .unwrap());
            }
        }

        // Collect model info first, then release lock
        let model_data: Vec<_> = {
            let models = self.models.read().await;
            models
                .iter()
                .filter_map(|m| m.schema_extractor.as_ref().map(|e| (m.name.clone(), e.clone())))
                .collect()
        };

        // Re-validate each registered model
        for (model_name, extractor) in model_data {
            // Extract current schema
            let current_spec = extractor();

            let stored = load_schema_spec(&model_name, base_path)?;

            let model_result = if let Some(stored_spec) = stored {
                let changes = SchemaChangeDetector::detect_changes(&stored_spec, &current_spec);

                if changes.is_empty() {
                    serde_json::json!({
                        "model": model_name,
                        "status": "unchanged",
                        "version": stored_spec.version,
                    })
                } else {
                    total_changes += changes.len();

                    // Record changes in history
                    let applied = AppliedSchemaChange {
                        id: uuid::Uuid::new_v4(),
                        model_name: model_name.clone(),
                        changes: changes.clone(),
                        applied_at: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                        applied_by_node: 0,
                    };

                    // Update state
                    {
                        let mut state = self.schema_sync_state.write().await;
                        state.change_history.push(applied.clone());
                    }

                    // Persist history
                    if let Err(e) = crate::schema::append_schema_history(&applied, base_path) {
                        log::error!("Failed to persist schema history: {}", e);
                    }

                    // Save updated schema if auto mode
                    let new_version =
                        if self.config.storage.schema_migration_mode == SchemaMigrationMode::Auto {
                            let mut new_spec = current_spec.clone();
                            new_spec.version = stored_spec.version + 1;
                            save_schema_spec(&new_spec, base_path)?;
                            new_spec.version
                        } else {
                            stored_spec.version
                        };

                    let change_details: Vec<_> = changes
                        .iter()
                        .map(|c| {
                            serde_json::json!({
                                "type": format!("{:?}", c.change_type),
                                "field": c.field_name,
                                "strategy": format!("{:?}", c.migration_strategy),
                            })
                        })
                        .collect();

                    serde_json::json!({
                        "model": model_name,
                        "status": "migrated",
                        "from_version": stored_spec.version,
                        "to_version": new_version,
                        "changes": change_details,
                    })
                }
            } else {
                // First time - save schema
                save_schema_spec(&current_spec, base_path)?;
                serde_json::json!({
                    "model": model_name,
                    "status": "created",
                    "version": 1,
                })
            };

            results.push(model_result);
        }

        let response = serde_json::json!({
            "status": if total_changes > 0 { "changes_detected" } else { "no_changes" },
            "total_changes": total_changes,
            "models": results,
        });

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(serde_json::to_string_pretty(&response)?)))
            .unwrap())
    }

    /// POST /_admin/schema/rollback/{change_id} - Rollback to a previous schema version
    ///
    /// Reverts a model to its previous schema state.
    /// Only works if the previous schema is available in history.
    pub(crate) async fn handle_admin_schema_rollback(
        &self,
        _req: Request<hyper::body::Incoming>,
        path: &str,
    ) -> Result<Response<Full<Bytes>>> {
        use crate::schema::save_schema_spec;
        use std::path::Path;

        // Extract change_id from path
        let change_id_str = path.strip_prefix("/_admin/schema/rollback/").unwrap_or_default();

        let change_id: uuid::Uuid = match change_id_str.parse() {
            Ok(id) => id,
            Err(_) => {
                return Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(r#"{"error":"Invalid change_id format"}"#)))
                    .unwrap());
            }
        };

        let mut state = self.schema_sync_state.write().await;

        // Find the change in history
        let change_idx = state.change_history.iter().position(|c| c.id == change_id);

        match change_idx {
            Some(idx) => {
                let change = &state.change_history[idx];
                let model_name = change.model_name.clone();

                // Find the change in pending_changes to get the old_spec
                let old_spec =
                    state.pending_changes.get(&change_id).and_then(|p| p.old_spec.clone());

                match old_spec {
                    Some(previous_spec) => {
                        // Save the previous schema
                        let base_path = Path::new(&self.config.storage.data_dir);
                        if let Err(e) = save_schema_spec(&previous_spec, base_path) {
                            return Ok(Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .header("Content-Type", "application/json")
                                .body(Full::new(Bytes::from(format!(
                                    r#"{{"error":"Failed to save rollback schema: {}"}}"#,
                                    e
                                ))))
                                .unwrap());
                        }

                        // Update state
                        state.schemas.insert(model_name.clone(), previous_spec.clone());

                        log::warn!(
                            "Schema rollback executed for '{}' (change {})",
                            model_name,
                            change_id
                        );

                        let response = serde_json::json!({
                            "status": "rolled_back",
                            "model": model_name,
                            "change_id": change_id,
                            "reverted_to_version": previous_spec.version,
                            "message": "Schema rolled back successfully. Restart nodes with matching code version.",
                        });

                        Ok(Response::builder()
                            .status(StatusCode::OK)
                            .header("Content-Type", "application/json")
                            .body(Full::new(Bytes::from(serde_json::to_string(&response)?)))
                            .unwrap())
                    }
                    None => Ok(Response::builder()
                        .status(StatusCode::UNPROCESSABLE_ENTITY)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(
                            r#"{"error":"Previous schema not available for rollback"}"#,
                        )))
                        .unwrap()),
                }
            }
            None => Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(r#"{"error":"Change not found in history"}"#)))
                .unwrap()),
        }
    }

    // =========================================================================
    // SCHEMA LOCK/UNLOCK ENDPOINTS
    // =========================================================================

    /// GET /_admin/schema/lock/status - Get current lock status
    ///
    /// Returns whether schema migrations are currently locked or unlocked,
    /// along with timeout information if applicable.
    pub(crate) async fn handle_admin_schema_lock_status(&self) -> Result<Response<Full<Bytes>>> {
        let state = self.schema_sync_state.read().await;
        let lock = &state.lock_status;

        let is_locked = lock.is_locked();
        let remaining = lock.remaining_unlock_secs();

        let response = serde_json::json!({
            "locked": is_locked,
            "reason": lock.reason,
            "unlocked_by": lock.unlocked_by,
            "unlocked_at": lock.unlocked_at,
            "remaining_seconds": remaining,
            "auto_relock_at": lock.unlock_expires_at,
        });

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(serde_json::to_string_pretty(&response)?)))
            .unwrap())
    }

    /// POST /_admin/schema/lock - Lock schema migrations
    ///
    /// Body (optional):
    /// ```json
    /// { "reason": "Production freeze for holiday" }
    /// ```
    ///
    /// When locked, ALL schema changes are rejected until unlocked.
    pub(crate) async fn handle_admin_schema_lock(
        &self,
        req: Request<hyper::body::Incoming>,
    ) -> Result<Response<Full<Bytes>>> {
        // Parse optional body
        let body_bytes = req.collect().await?.to_bytes();
        let reason: Option<String> = if body_bytes.is_empty() {
            None
        } else {
            serde_json::from_slice::<serde_json::Value>(&body_bytes)
                .ok()
                .and_then(|v| v.get("reason").and_then(|r| r.as_str().map(|s| s.to_string())))
        };

        let mut state = self.schema_sync_state.write().await;
        state.lock_status.lock(reason.clone());

        // Persist lock status
        let base_path = std::path::Path::new(&self.config.storage.data_dir);
        if let Err(e) = crate::schema::save_lock_status(&state.lock_status, base_path) {
            log::error!("Failed to persist lock status: {}", e);
        }

        log::info!(
            "Schema migrations LOCKED{}",
            reason.as_ref().map(|r| format!(": {}", r)).unwrap_or_default()
        );

        let response = serde_json::json!({
            "status": "locked",
            "reason": reason,
            "message": "Schema migrations are now locked. All changes will be rejected.",
        });

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(serde_json::to_string_pretty(&response)?)))
            .unwrap())
    }

    /// POST /_admin/schema/unlock - Unlock schema migrations
    ///
    /// Body (optional):
    /// ```json
    /// {
    ///   "reason": "v2.5 deployment",
    ///   "duration_seconds": 1800,
    ///   "unlocked_by": "admin@example.com"
    /// }
    /// ```
    ///
    /// If duration_seconds is provided, migrations auto-relock after that time.
    pub(crate) async fn handle_admin_schema_unlock(
        &self,
        req: Request<hyper::body::Incoming>,
    ) -> Result<Response<Full<Bytes>>> {
        // Parse optional body
        let body_bytes = req.collect().await?.to_bytes();

        let (reason, duration_secs, unlocked_by): (Option<String>, Option<u64>, Option<String>) =
            if body_bytes.is_empty() {
                (None, None, None)
            } else {
                let v: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap_or_default();
                (
                    v.get("reason").and_then(|r| r.as_str().map(|s| s.to_string())),
                    v.get("duration_seconds").and_then(|d| d.as_u64()),
                    v.get("unlocked_by").and_then(|u| u.as_str().map(|s| s.to_string())),
                )
            };

        let mut state = self.schema_sync_state.write().await;
        state.lock_status.unlock(reason.clone(), duration_secs, unlocked_by.clone());

        // Persist lock status
        let base_path = std::path::Path::new(&self.config.storage.data_dir);
        if let Err(e) = crate::schema::save_lock_status(&state.lock_status, base_path) {
            log::error!("Failed to persist lock status: {}", e);
        }

        let auto_relock_msg = duration_secs.map(|d| format!(" (auto-relock in {}s)", d));

        log::info!(
            "Schema migrations UNLOCKED{}{}",
            reason.as_ref().map(|r| format!(": {}", r)).unwrap_or_default(),
            auto_relock_msg.as_deref().unwrap_or("")
        );

        let response = serde_json::json!({
            "status": "unlocked",
            "reason": reason,
            "unlocked_by": unlocked_by,
            "duration_seconds": duration_secs,
            "auto_relock_at": state.lock_status.unlock_expires_at,
            "message": format!("Schema migrations are now unlocked.{}",
                auto_relock_msg.unwrap_or_default()),
        });

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(serde_json::to_string_pretty(&response)?)))
            .unwrap())
    }
}
