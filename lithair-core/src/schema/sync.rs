//! Schema synchronization types for cluster consensus
//!
//! This module provides types and logic for synchronizing schema changes
//! across a Lithair cluster using Raft consensus.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

use super::{DetectedSchemaChange, MigrationStrategy, ModelSpec};

// =============================================================================
// VOTE STRATEGY
// =============================================================================

/// Strategy for handling schema changes in a cluster
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum VoteStrategy {
    /// Auto-accept without voting - change applied immediately
    AutoAccept,

    /// Auto-reject - change is never accepted
    Reject,

    /// Require majority consensus from cluster nodes
    #[default]
    Consensus,

    /// Require manual approval via admin API
    ManualApproval {
        /// Timeout before auto-reject (None = wait forever)
        #[serde(default)]
        #[serde(with = "option_duration_serde")]
        timeout: Option<Duration>,
        /// Minimum number of approvers required
        #[serde(default = "default_min_approvers")]
        min_approvers: u32,
    },
}

fn default_min_approvers() -> u32 {
    1
}


// =============================================================================
// SCHEMA VOTE POLICY
// =============================================================================

/// Policy defining how different types of schema changes are handled
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaVotePolicy {
    /// Policy for additive changes (nullable field, new index, field with default)
    /// These are generally safe and can be auto-accepted
    #[serde(default = "default_additive_policy")]
    pub additive: VoteStrategy,

    /// Policy for breaking changes (remove field, add required field without default)
    /// These are dangerous and typically require manual approval
    #[serde(default = "default_breaking_policy")]
    pub breaking: VoteStrategy,

    /// Policy for versioned changes (type change, rename with migration)
    /// These require careful handling and cluster consensus
    #[serde(default = "default_versioned_policy")]
    pub versioned: VoteStrategy,
}

fn default_additive_policy() -> VoteStrategy {
    VoteStrategy::AutoAccept
}

fn default_breaking_policy() -> VoteStrategy {
    VoteStrategy::ManualApproval { timeout: None, min_approvers: 1 }
}

fn default_versioned_policy() -> VoteStrategy {
    VoteStrategy::Consensus
}

impl Default for SchemaVotePolicy {
    fn default() -> Self {
        Self {
            additive: default_additive_policy(),
            breaking: default_breaking_policy(),
            versioned: default_versioned_policy(),
        }
    }
}

impl SchemaVotePolicy {
    /// Create a strict policy that rejects all breaking changes
    pub fn strict() -> Self {
        Self {
            additive: VoteStrategy::AutoAccept,
            breaking: VoteStrategy::Reject,
            versioned: VoteStrategy::Consensus,
        }
    }

    /// Create a permissive policy that auto-accepts everything
    pub fn permissive() -> Self {
        Self {
            additive: VoteStrategy::AutoAccept,
            breaking: VoteStrategy::AutoAccept,
            versioned: VoteStrategy::AutoAccept,
        }
    }

    /// Create a policy requiring manual approval for all changes
    pub fn manual() -> Self {
        Self {
            additive: VoteStrategy::ManualApproval { timeout: None, min_approvers: 1 },
            breaking: VoteStrategy::ManualApproval { timeout: None, min_approvers: 1 },
            versioned: VoteStrategy::ManualApproval { timeout: None, min_approvers: 1 },
        }
    }

    /// Get the vote strategy for a given migration strategy
    pub fn strategy_for(&self, migration: &MigrationStrategy) -> &VoteStrategy {
        match migration {
            MigrationStrategy::Additive => &self.additive,
            MigrationStrategy::Breaking => &self.breaking,
            MigrationStrategy::Versioned => &self.versioned,
        }
    }
}

// =============================================================================
// PENDING SCHEMA CHANGE
// =============================================================================

/// Status of a pending schema change
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SchemaChangeStatus {
    /// Waiting for votes/approval
    Pending,
    /// Approved and ready to apply
    Approved,
    /// Rejected by policy or votes
    Rejected,
    /// Applied to cluster
    Applied,
    /// Expired (timeout reached)
    Expired,
}

/// A schema change waiting for approval or consensus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingSchemaChange {
    /// Unique identifier for this change
    pub id: Uuid,

    /// Model being changed
    pub model_name: String,

    /// Node that proposed the change
    pub proposer_node_id: u64,

    /// The detected changes
    pub changes: Vec<DetectedSchemaChange>,

    /// The new schema spec (after changes)
    pub new_spec: ModelSpec,

    /// Previous schema spec (before changes)
    pub old_spec: Option<ModelSpec>,

    /// Overall migration strategy (worst case from all changes)
    pub overall_strategy: MigrationStrategy,

    /// Current status
    pub status: SchemaChangeStatus,

    /// Timestamp when proposed (Unix millis)
    pub proposed_at: u64,

    /// Timeout timestamp if applicable (Unix millis)
    pub expires_at: Option<u64>,

    /// Nodes that have voted to approve
    pub approvals: Vec<SchemaApproval>,

    /// Nodes that have voted to reject
    pub rejections: Vec<SchemaRejection>,

    /// Human approvers (for ManualApproval strategy)
    pub human_approvals: Vec<HumanApproval>,

    /// Reason for rejection if rejected
    pub rejection_reason: Option<String>,
}

impl PendingSchemaChange {
    /// Create a new pending schema change
    pub fn new(
        model_name: String,
        proposer_node_id: u64,
        changes: Vec<DetectedSchemaChange>,
        new_spec: ModelSpec,
        old_spec: Option<ModelSpec>,
    ) -> Self {
        // Determine overall strategy (most restrictive wins)
        let overall_strategy = changes.iter().map(|c| &c.migration_strategy).fold(
            MigrationStrategy::Additive,
            |acc, s| match (&acc, s) {
                (MigrationStrategy::Breaking, _) => MigrationStrategy::Breaking,
                (_, MigrationStrategy::Breaking) => MigrationStrategy::Breaking,
                (MigrationStrategy::Versioned, _) => MigrationStrategy::Versioned,
                (_, MigrationStrategy::Versioned) => MigrationStrategy::Versioned,
                _ => MigrationStrategy::Additive,
            },
        );

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Self {
            id: Uuid::new_v4(),
            model_name,
            proposer_node_id,
            changes,
            new_spec,
            old_spec,
            overall_strategy,
            status: SchemaChangeStatus::Pending,
            proposed_at: now,
            expires_at: None,
            approvals: Vec::new(),
            rejections: Vec::new(),
            human_approvals: Vec::new(),
            rejection_reason: None,
        }
    }

    /// Set expiration timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.expires_at = Some(now + timeout.as_millis() as u64);
        self
    }

    /// Check if this change has expired
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            now >= expires_at
        } else {
            false
        }
    }

    /// Add a node approval
    pub fn add_approval(&mut self, node_id: u64) {
        if !self.approvals.iter().any(|a| a.node_id == node_id) {
            self.approvals.push(SchemaApproval { node_id, timestamp: current_timestamp() });
        }
    }

    /// Add a node rejection
    pub fn add_rejection(&mut self, node_id: u64, reason: Option<String>) {
        if !self.rejections.iter().any(|r| r.node_id == node_id) {
            self.rejections.push(SchemaRejection {
                node_id,
                timestamp: current_timestamp(),
                reason: reason.clone(),
            });
            if self.rejection_reason.is_none() {
                self.rejection_reason = reason;
            }
        }
    }

    /// Add a human approval
    pub fn add_human_approval(&mut self, user_id: String, user_name: Option<String>) {
        if !self.human_approvals.iter().any(|a| a.user_id == user_id) {
            self.human_approvals.push(HumanApproval {
                user_id,
                user_name,
                timestamp: current_timestamp(),
            });
        }
    }

    /// Check if change has enough approvals based on policy
    pub fn has_enough_approvals(&self, policy: &SchemaVotePolicy, total_nodes: usize) -> bool {
        let strategy = policy.strategy_for(&self.overall_strategy);
        match strategy {
            VoteStrategy::AutoAccept => true,
            VoteStrategy::Reject => false,
            VoteStrategy::Consensus => {
                // Majority of nodes must approve
                let majority = (total_nodes / 2) + 1;
                self.approvals.len() >= majority
            }
            VoteStrategy::ManualApproval { min_approvers, .. } => {
                self.human_approvals.len() >= *min_approvers as usize
            }
        }
    }

    /// Check if change should be rejected based on policy
    pub fn should_reject(&self, policy: &SchemaVotePolicy, total_nodes: usize) -> bool {
        let strategy = policy.strategy_for(&self.overall_strategy);
        match strategy {
            VoteStrategy::Reject => true,
            VoteStrategy::Consensus => {
                // Majority of nodes rejected
                let majority = (total_nodes / 2) + 1;
                self.rejections.len() >= majority
            }
            VoteStrategy::ManualApproval { timeout, .. } => {
                // Expired without enough approvals
                if timeout.is_some() && self.is_expired() {
                    return true;
                }
                false
            }
            VoteStrategy::AutoAccept => false,
        }
    }
}

/// Node approval record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaApproval {
    pub node_id: u64,
    pub timestamp: u64,
}

/// Node rejection record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaRejection {
    pub node_id: u64,
    pub timestamp: u64,
    pub reason: Option<String>,
}

/// Human approval record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanApproval {
    pub user_id: String,
    pub user_name: Option<String>,
    pub timestamp: u64,
}

// =============================================================================
// SCHEMA SYNC MESSAGES (for Raft)
// =============================================================================

/// Messages for schema synchronization via Raft
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SchemaSyncMessage {
    /// Propose a schema change to the cluster
    ProposeChange(Box<PendingSchemaChange>),

    /// Vote to approve a pending change
    ApproveChange { change_id: Uuid, node_id: u64 },

    /// Vote to reject a pending change
    RejectChange { change_id: Uuid, node_id: u64, reason: Option<String> },

    /// Human approval via admin API
    HumanApprove { change_id: Uuid, user_id: String, user_name: Option<String> },

    /// Finalize and apply a change (leader broadcasts after consensus)
    ApplyChange { change_id: Uuid, new_spec: ModelSpec },

    /// Request current schema from leader (for new nodes)
    RequestSchema { model_name: String, requesting_node_id: u64 },

    /// Response with current schema
    SchemaResponse {
        model_name: String,
        spec: Option<ModelSpec>,
        pending_changes: Vec<PendingSchemaChange>,
    },
}

// =============================================================================
// SCHEMA SYNC STATE
// =============================================================================

/// Schema migration lock status
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SchemaLockStatus {
    /// Whether schema migrations are locked (blocked)
    pub locked: bool,

    /// Reason for current lock/unlock state
    pub reason: Option<String>,

    /// When the unlock expires (auto-relock), timestamp in millis
    pub unlock_expires_at: Option<u64>,

    /// Who unlocked (for audit trail)
    pub unlocked_by: Option<String>,

    /// When was it unlocked
    pub unlocked_at: Option<u64>,
}


impl SchemaLockStatus {
    /// Check if currently locked (considering timeout)
    pub fn is_locked(&self) -> bool {
        if !self.locked {
            // Check if unlock has expired
            if let Some(expires_at) = self.unlock_expires_at {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                if now >= expires_at {
                    return true; // Auto-relocked
                }
            }
            false
        } else {
            true
        }
    }

    /// Lock schema migrations
    pub fn lock(&mut self, reason: Option<String>) {
        self.locked = true;
        self.reason = reason;
        self.unlock_expires_at = None;
        self.unlocked_by = None;
        self.unlocked_at = None;
    }

    /// Unlock schema migrations with optional timeout
    pub fn unlock(
        &mut self,
        reason: Option<String>,
        duration_secs: Option<u64>,
        unlocked_by: Option<String>,
    ) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        self.locked = false;
        self.reason = reason;
        self.unlocked_by = unlocked_by;
        self.unlocked_at = Some(now);

        if let Some(duration) = duration_secs {
            self.unlock_expires_at = Some(now + (duration * 1000));
        } else {
            self.unlock_expires_at = None;
        }
    }

    /// Get remaining unlock time in seconds (if any)
    pub fn remaining_unlock_secs(&self) -> Option<u64> {
        if self.locked {
            return None;
        }
        self.unlock_expires_at.map(|expires_at| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            if now >= expires_at {
                0
            } else {
                (expires_at - now) / 1000
            }
        })
    }
}

/// State for schema synchronization in a cluster
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SchemaSyncState {
    /// Current schemas by model name
    pub schemas: HashMap<String, ModelSpec>,

    /// Pending schema changes awaiting approval
    pub pending_changes: HashMap<Uuid, PendingSchemaChange>,

    /// Vote policy
    #[serde(default)]
    pub policy: SchemaVotePolicy,

    /// History of applied changes (for audit)
    pub change_history: Vec<AppliedSchemaChange>,

    /// Lock status for schema migrations
    #[serde(default)]
    pub lock_status: SchemaLockStatus,
}

/// Record of an applied schema change
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppliedSchemaChange {
    pub id: Uuid,
    pub model_name: String,
    pub changes: Vec<DetectedSchemaChange>,
    pub applied_at: u64,
    pub applied_by_node: u64,
}

impl SchemaSyncState {
    /// Create new state with a policy
    pub fn with_policy(policy: SchemaVotePolicy) -> Self {
        Self { policy, ..Default::default() }
    }

    /// Get pending changes for a model
    pub fn pending_for_model(&self, model_name: &str) -> Vec<&PendingSchemaChange> {
        self.pending_changes
            .values()
            .filter(|c| c.model_name == model_name && c.status == SchemaChangeStatus::Pending)
            .collect()
    }

    /// Get all pending changes
    pub fn all_pending(&self) -> Vec<&PendingSchemaChange> {
        self.pending_changes
            .values()
            .filter(|c| c.status == SchemaChangeStatus::Pending)
            .collect()
    }

    /// Add a pending change
    pub fn add_pending(&mut self, change: PendingSchemaChange) {
        self.pending_changes.insert(change.id, change);
    }

    /// Update a pending change status
    pub fn update_status(&mut self, change_id: Uuid, status: SchemaChangeStatus) {
        if let Some(change) = self.pending_changes.get_mut(&change_id) {
            change.status = status;
        }
    }

    /// Apply a change and update schema
    pub fn apply_change(&mut self, change_id: Uuid, applied_by_node: u64) -> Option<ModelSpec> {
        if let Some(change) = self.pending_changes.get_mut(&change_id) {
            change.status = SchemaChangeStatus::Applied;

            let new_spec = change.new_spec.clone();
            let model_name = change.model_name.clone();

            // Update current schema
            self.schemas.insert(model_name.clone(), new_spec.clone());

            // Add to history
            self.change_history.push(AppliedSchemaChange {
                id: change_id,
                model_name,
                changes: change.changes.clone(),
                applied_at: current_timestamp(),
                applied_by_node,
            });

            Some(new_spec)
        } else {
            None
        }
    }

    /// Clean up expired pending changes
    pub fn cleanup_expired(&mut self) {
        for change in self.pending_changes.values_mut() {
            if change.status == SchemaChangeStatus::Pending && change.is_expired() {
                change.status = SchemaChangeStatus::Expired;
                change.rejection_reason = Some("Timeout expired".to_string());
            }
        }
    }
}

// =============================================================================
// HELPERS
// =============================================================================

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Custom serde module for Option<Duration>
mod option_duration_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Option<Duration>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match duration {
            Some(d) => d.as_secs().serialize(serializer),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt: Option<u64> = Option::deserialize(deserializer)?;
        Ok(opt.map(Duration::from_secs))
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_policy() {
        let policy = SchemaVotePolicy::default();
        assert_eq!(policy.additive, VoteStrategy::AutoAccept);
        assert!(matches!(policy.breaking, VoteStrategy::ManualApproval { .. }));
        assert_eq!(policy.versioned, VoteStrategy::Consensus);
    }

    #[test]
    fn test_strict_policy() {
        let policy = SchemaVotePolicy::strict();
        assert_eq!(policy.additive, VoteStrategy::AutoAccept);
        assert_eq!(policy.breaking, VoteStrategy::Reject);
        assert_eq!(policy.versioned, VoteStrategy::Consensus);
    }

    #[test]
    fn test_pending_change_overall_strategy() {
        use crate::schema::{DetectedSchemaChange, SchemaChangeType};

        let additive_change = DetectedSchemaChange {
            model: "Test".to_string(),
            change_type: SchemaChangeType::AddField,
            field_name: Some("foo".to_string()),
            old_type: None,
            new_type: Some("i32".to_string()),
            old_constraints: None,
            new_constraints: None,
            migration_strategy: MigrationStrategy::Additive,
            default_value: Some("0".to_string()),
            requires_consensus: false,
            migration_sql: None,
            rollback_sql: None,
        };

        let breaking_change = DetectedSchemaChange {
            model: "Test".to_string(),
            change_type: SchemaChangeType::RemoveField,
            field_name: Some("bar".to_string()),
            old_type: Some("String".to_string()),
            new_type: None,
            old_constraints: None,
            new_constraints: None,
            migration_strategy: MigrationStrategy::Breaking,
            default_value: None,
            requires_consensus: true,
            migration_sql: None,
            rollback_sql: None,
        };

        let spec = ModelSpec {
            model_name: "Test".to_string(),
            version: 2,
            fields: std::collections::HashMap::new(),
            indexes: vec![],
            foreign_keys: vec![],
        };

        // Only additive → Additive
        let pending = PendingSchemaChange::new(
            "Test".to_string(),
            1,
            vec![additive_change.clone()],
            spec.clone(),
            None,
        );
        assert_eq!(pending.overall_strategy, MigrationStrategy::Additive);

        // Additive + Breaking → Breaking (most restrictive)
        let pending = PendingSchemaChange::new(
            "Test".to_string(),
            1,
            vec![additive_change, breaking_change],
            spec,
            None,
        );
        assert_eq!(pending.overall_strategy, MigrationStrategy::Breaking);
    }

    #[test]
    fn test_consensus_approval() {
        let spec = ModelSpec {
            model_name: "Test".to_string(),
            version: 2,
            fields: std::collections::HashMap::new(),
            indexes: vec![],
            foreign_keys: vec![],
        };

        let mut pending = PendingSchemaChange::new("Test".to_string(), 1, vec![], spec, None);
        pending.overall_strategy = MigrationStrategy::Versioned;

        let policy = SchemaVotePolicy::default(); // Versioned = Consensus

        // 3 node cluster, need 2 approvals
        assert!(!pending.has_enough_approvals(&policy, 3));

        pending.add_approval(1);
        assert!(!pending.has_enough_approvals(&policy, 3));

        pending.add_approval(2);
        assert!(pending.has_enough_approvals(&policy, 3));
    }
}
