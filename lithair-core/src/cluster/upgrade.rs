//! Upgrade types for rolling upgrade system
//!
//! These types support zero-downtime rolling upgrades with schema migrations
//! that flow through the Raft consensus log.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Semantic version with schema hash for integrity verification
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    /// SHA256 hash of compiled schema (ensures binary matches schema)
    pub schema_hash: String,
    /// Git commit or build identifier
    pub build_id: String,
}

impl Version {
    /// Create a new Version
    pub fn new(major: u32, minor: u32, patch: u32, schema_hash: String, build_id: String) -> Self {
        Self { major, minor, patch, schema_hash, build_id }
    }

    /// Check if this version can read data from another version
    pub fn can_read_from(&self, other: &Version) -> bool {
        // Same major version = backward compatible reads
        self.major == other.major && self.minor >= other.minor
    }

    /// Check if migration is required
    pub fn requires_migration_from(&self, other: &Version) -> bool {
        self.schema_hash != other.schema_hash
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Extended node operating mode for upgrade coordination
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub enum NodeMode {
    /// Normal operation - accepts web traffic and participates in Raft
    #[default]
    Serving,
    /// Sync-only mode - participates in Raft but doesn't serve web traffic
    SyncOnly,
    /// Draining mode - stops accepting new requests, finishes in-flight
    Draining,
    /// Upgrading mode - node is restarting with new binary
    Upgrading,
    /// Validation mode - new binary running, performing self-checks
    Validating,
    /// Ready mode - upgrade complete, waiting for cluster coordination
    Ready,
}

/// Field type for schema definitions
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldType {
    String,
    Integer,
    Float,
    Boolean,
    DateTime,
    Uuid,
    Json,
    Binary,
}

/// Field definition for schema changes
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldDefinition {
    pub name: String,
    pub field_type: FieldType,
    pub nullable: bool,
    #[serde(default)]
    pub unique: bool,
    #[serde(default)]
    pub indexed: bool,
}

/// Model schema for AddModel operations
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelSchema {
    pub name: String,
    pub fields: Vec<FieldDefinition>,
    #[serde(default)]
    pub primary_key: Option<String>,
}

/// Schema change operations for migrations
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SchemaChange {
    /// Add a new model/table
    AddModel { name: String, schema: ModelSchema },
    /// Remove a model/table
    RemoveModel {
        name: String,
        #[serde(default)]
        backup_path: Option<String>,
    },
    /// Add field to existing model
    AddField {
        model: String,
        field: FieldDefinition,
        #[serde(default)]
        default_value: Option<serde_json::Value>,
    },
    /// Remove field from model
    RemoveField { model: String, field: String },
    /// Rename field
    RenameField { model: String, old_name: String, new_name: String },
    /// Change field type (with transformation)
    ChangeFieldType {
        model: String,
        field: String,
        new_type: FieldType,
        #[serde(default)]
        transform: Option<String>,
    },
    /// Add index
    AddIndex {
        model: String,
        fields: Vec<String>,
        #[serde(default)]
        unique: bool,
    },
    /// Custom operation for complex migrations
    Custom { description: String, forward: String, backward: String },
}

/// Status of a migration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MigrationStatus {
    /// Migration is in progress
    InProgress,
    /// Migration completed successfully
    Committed,
    /// Migration was rolled back
    RolledBack,
    /// Migration failed
    Failed { reason: String },
}

/// Rollback operation for undoing a schema change
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackOp {
    pub step_index: u32,
    pub operation: SchemaChange,
    /// JSON snapshot of affected data before the change
    pub data_snapshot: Option<serde_json::Value>,
}

/// Context for tracking an in-progress migration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationContext {
    pub id: Uuid,
    pub from_version: Version,
    pub to_version: Version,
    pub status: MigrationStatus,
    pub current_step: u32,
    pub total_steps: u32,
    pub rollback_log: Vec<RollbackOp>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

impl MigrationContext {
    /// Create a new migration context
    pub fn new(id: Uuid, from_version: Version, to_version: Version) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        Self {
            id,
            from_version,
            to_version,
            status: MigrationStatus::InProgress,
            current_step: 0,
            total_steps: 0,
            rollback_log: Vec::new(),
            created_at_ms: now,
            updated_at_ms: now,
        }
    }

    /// Record a completed step with its rollback operation
    pub fn record_step(
        &mut self,
        step_index: u32,
        rollback_op: SchemaChange,
        data_snapshot: Option<serde_json::Value>,
    ) {
        self.rollback_log
            .push(RollbackOp { step_index, operation: rollback_op, data_snapshot });
        self.current_step = step_index + 1;
        self.updated_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
    }

    /// Mark migration as committed
    pub fn commit(&mut self) {
        self.status = MigrationStatus::Committed;
        self.updated_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
    }

    /// Mark migration as rolled back
    pub fn rollback(&mut self) {
        self.status = MigrationStatus::RolledBack;
        self.updated_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
    }

    /// Mark migration as failed
    pub fn fail(&mut self, reason: String) {
        self.status = MigrationStatus::Failed { reason };
        self.updated_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
    }

    /// Get rollback operations in reverse order (for undoing changes)
    pub fn get_rollback_ops(&self) -> Vec<&RollbackOp> {
        self.rollback_log.iter().rev().collect()
    }
}

/// Manager for tracking active migrations across the cluster
pub struct MigrationManager {
    /// Active migrations by ID
    active: RwLock<HashMap<Uuid, MigrationContext>>,
    /// Completed migrations (kept for audit)
    completed: RwLock<Vec<MigrationContext>>,
    /// Current schema version
    current_version: RwLock<Version>,
}

impl MigrationManager {
    /// Create a new migration manager
    pub fn new(initial_version: Version) -> Self {
        Self {
            active: RwLock::new(HashMap::new()),
            completed: RwLock::new(Vec::new()),
            current_version: RwLock::new(initial_version),
        }
    }

    /// Start a new migration
    pub async fn begin_migration(
        &self,
        id: Uuid,
        from_version: Version,
        to_version: Version,
    ) -> Result<(), String> {
        let mut active = self.active.write().await;

        // Check if there's already an active migration
        if !active.is_empty() {
            return Err("Another migration is already in progress".to_string());
        }

        // Verify from_version matches current
        let current = self.current_version.read().await;
        if *current != from_version {
            return Err(format!(
                "Version mismatch: current is {}, migration expects {}",
                current, from_version
            ));
        }
        drop(current);

        let context = MigrationContext::new(id, from_version, to_version);
        active.insert(id, context);

        log::info!("🔄 Migration {} started", id);
        Ok(())
    }

    /// Get an active migration context
    pub async fn get_migration(&self, id: &Uuid) -> Option<MigrationContext> {
        let active = self.active.read().await;
        active.get(id).cloned()
    }

    /// Update a migration step
    pub async fn record_step(
        &self,
        id: &Uuid,
        step_index: u32,
        rollback_op: SchemaChange,
        data_snapshot: Option<serde_json::Value>,
    ) -> Result<(), String> {
        let mut active = self.active.write().await;
        let context = active.get_mut(id).ok_or_else(|| format!("Migration {} not found", id))?;

        context.record_step(step_index, rollback_op, data_snapshot);
        log::debug!("📝 Migration {} step {} recorded", id, step_index);
        Ok(())
    }

    /// Commit a migration
    pub async fn commit_migration(&self, id: &Uuid, new_version: Version) -> Result<(), String> {
        let mut active = self.active.write().await;
        let mut context = active.remove(id).ok_or_else(|| format!("Migration {} not found", id))?;

        context.commit();

        // Update current version
        let mut current = self.current_version.write().await;
        *current = new_version;
        drop(current);

        // Move to completed
        let mut completed = self.completed.write().await;
        completed.push(context);

        log::info!("✅ Migration {} committed", id);
        Ok(())
    }

    /// Rollback a migration
    pub async fn rollback_migration(&self, id: &Uuid) -> Result<Vec<RollbackOp>, String> {
        let mut active = self.active.write().await;
        let mut context = active.remove(id).ok_or_else(|| format!("Migration {} not found", id))?;

        let rollback_ops: Vec<RollbackOp> = context.rollback_log.drain(..).rev().collect();
        context.rollback();

        // Move to completed
        let mut completed = self.completed.write().await;
        completed.push(context);

        log::warn!("⚠️ Migration {} rolled back", id);
        Ok(rollback_ops)
    }

    /// Get current schema version
    pub async fn current_version(&self) -> Version {
        self.current_version.read().await.clone()
    }

    /// Check if a migration is active
    pub async fn has_active_migration(&self) -> bool {
        !self.active.read().await.is_empty()
    }
}

impl Default for MigrationManager {
    fn default() -> Self {
        Self::new(Version::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_display() {
        let v = Version::new(1, 2, 3, "abc123".to_string(), "git-xyz".to_string());
        assert_eq!(v.to_string(), "1.2.3");
    }

    #[test]
    fn test_version_can_read_from() {
        let v1 = Version::new(1, 2, 0, "hash1".to_string(), "build1".to_string());
        let v2 = Version::new(1, 1, 0, "hash2".to_string(), "build2".to_string());
        let v3 = Version::new(2, 0, 0, "hash3".to_string(), "build3".to_string());

        // v1 (1.2) can read from v2 (1.1) - same major, higher minor
        assert!(v1.can_read_from(&v2));

        // v2 (1.1) cannot read from v1 (1.2) - lower minor
        assert!(!v2.can_read_from(&v1));

        // v3 (2.0) cannot read from v1 (1.2) - different major
        assert!(!v3.can_read_from(&v1));
    }

    #[test]
    fn test_version_requires_migration() {
        let v1 = Version::new(1, 0, 0, "hash_a".to_string(), "build1".to_string());
        let v2 = Version::new(1, 1, 0, "hash_b".to_string(), "build2".to_string());
        let v3 = Version::new(1, 2, 0, "hash_a".to_string(), "build3".to_string());

        // Different schema hash = migration required
        assert!(v2.requires_migration_from(&v1));

        // Same schema hash = no migration (even with different version)
        assert!(!v3.requires_migration_from(&v1));
    }

    #[test]
    fn test_node_mode_default() {
        assert_eq!(NodeMode::default(), NodeMode::Serving);
    }

    #[test]
    fn test_schema_change_serialization() {
        let change = SchemaChange::AddField {
            model: "User".to_string(),
            field: FieldDefinition {
                name: "email".to_string(),
                field_type: FieldType::String,
                nullable: false,
                unique: true,
                indexed: true,
            },
            default_value: None,
        };

        let json = serde_json::to_string(&change).unwrap();
        let parsed: SchemaChange = serde_json::from_str(&json).unwrap();
        assert_eq!(change, parsed);
    }

    #[test]
    fn test_migration_context_creation() {
        let id = Uuid::new_v4();
        let from = Version::new(1, 0, 0, "hash1".to_string(), "build1".to_string());
        let to = Version::new(1, 1, 0, "hash2".to_string(), "build2".to_string());

        let ctx = MigrationContext::new(id, from.clone(), to.clone());

        assert_eq!(ctx.id, id);
        assert_eq!(ctx.from_version, from);
        assert_eq!(ctx.to_version, to);
        assert_eq!(ctx.status, MigrationStatus::InProgress);
        assert_eq!(ctx.current_step, 0);
    }

    #[test]
    fn test_migration_context_record_step() {
        let id = Uuid::new_v4();
        let from = Version::new(1, 0, 0, "hash1".to_string(), "build1".to_string());
        let to = Version::new(1, 1, 0, "hash2".to_string(), "build2".to_string());

        let mut ctx = MigrationContext::new(id, from, to);

        // Record a step with rollback operation
        let rollback_op =
            SchemaChange::RemoveField { model: "User".to_string(), field: "email".to_string() };
        ctx.record_step(0, rollback_op.clone(), None);

        assert_eq!(ctx.current_step, 1);
        assert_eq!(ctx.rollback_log.len(), 1);
        assert_eq!(ctx.rollback_log[0].step_index, 0);
    }

    #[test]
    fn test_migration_context_commit() {
        let id = Uuid::new_v4();
        let from = Version::new(1, 0, 0, "hash1".to_string(), "build1".to_string());
        let to = Version::new(1, 1, 0, "hash2".to_string(), "build2".to_string());

        let mut ctx = MigrationContext::new(id, from, to);
        ctx.commit();

        assert_eq!(ctx.status, MigrationStatus::Committed);
    }

    #[test]
    fn test_migration_context_rollback() {
        let id = Uuid::new_v4();
        let from = Version::new(1, 0, 0, "hash1".to_string(), "build1".to_string());
        let to = Version::new(1, 1, 0, "hash2".to_string(), "build2".to_string());

        let mut ctx = MigrationContext::new(id, from, to);
        ctx.rollback();

        assert_eq!(ctx.status, MigrationStatus::RolledBack);
    }

    #[tokio::test]
    async fn test_migration_manager_begin() {
        let initial_version = Version::new(1, 0, 0, "hash1".to_string(), "build1".to_string());
        let manager = MigrationManager::new(initial_version.clone());

        let id = Uuid::new_v4();
        let to_version = Version::new(1, 1, 0, "hash2".to_string(), "build2".to_string());

        // Should succeed
        let result = manager.begin_migration(id, initial_version.clone(), to_version).await;
        assert!(result.is_ok());
        assert!(manager.has_active_migration().await);

        // Should fail - another migration in progress
        let id2 = Uuid::new_v4();
        let to_version2 = Version::new(1, 2, 0, "hash3".to_string(), "build3".to_string());
        let result2 = manager.begin_migration(id2, initial_version, to_version2).await;
        assert!(result2.is_err());
    }

    #[tokio::test]
    async fn test_migration_manager_commit() {
        let initial_version = Version::new(1, 0, 0, "hash1".to_string(), "build1".to_string());
        let manager = MigrationManager::new(initial_version.clone());

        let id = Uuid::new_v4();
        let to_version = Version::new(1, 1, 0, "hash2".to_string(), "build2".to_string());

        manager.begin_migration(id, initial_version, to_version.clone()).await.unwrap();
        manager.commit_migration(&id, to_version.clone()).await.unwrap();

        assert!(!manager.has_active_migration().await);
        assert_eq!(manager.current_version().await, to_version);
    }

    #[tokio::test]
    async fn test_migration_manager_rollback() {
        let initial_version = Version::new(1, 0, 0, "hash1".to_string(), "build1".to_string());
        let manager = MigrationManager::new(initial_version.clone());

        let id = Uuid::new_v4();
        let to_version = Version::new(1, 1, 0, "hash2".to_string(), "build2".to_string());

        manager.begin_migration(id, initial_version.clone(), to_version).await.unwrap();

        // Record some steps
        let rollback_op1 =
            SchemaChange::RemoveField { model: "User".to_string(), field: "email".to_string() };
        let rollback_op2 =
            SchemaChange::RemoveField { model: "User".to_string(), field: "phone".to_string() };
        manager.record_step(&id, 0, rollback_op1, None).await.unwrap();
        manager.record_step(&id, 1, rollback_op2, None).await.unwrap();

        // Rollback should return ops in reverse order
        let rollback_ops = manager.rollback_migration(&id).await.unwrap();
        assert_eq!(rollback_ops.len(), 2);
        assert_eq!(rollback_ops[0].step_index, 1); // step 1 first (reverse order)
        assert_eq!(rollback_ops[1].step_index, 0); // step 0 second

        // Version should remain unchanged after rollback
        assert_eq!(manager.current_version().await, initial_version);
    }
}
