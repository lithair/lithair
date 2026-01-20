# Lithair Rolling Upgrade Specification

> **Version**: 0.1.0 (Draft)
> **Status**: RFC
> **Author**: Claude + Human collaboration

## Executive Summary

This specification describes a **zero-downtime rolling upgrade system** for Lithair clusters. The key innovation is that schema migrations are **part of the consensus log**, ensuring atomic, ordered application across all nodes.

```
"Data defines infrastructure" → Migrations ARE data → Migrations go through Raft
```

---

## 1. Design Goals

| Goal | Description |
|------|-------------|
| **Zero Downtime** | Cluster always maintains quorum during upgrades |
| **Atomic Migrations** | Schema changes applied in same order on all nodes |
| **Canary Validation** | New version validated before cluster-wide rollout |
| **Safe Rollback** | Any failure → automatic rollback to previous version |
| **Binary-Schema Coupling** | Binary version = schema version (no drift possible) |

---

## 2. Node States

### 2.1 Extended Node Mode

```rust
/// Extended node operating mode for upgrade coordination
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeMode {
    /// Normal operation - accepts web traffic and participates in Raft
    Serving,

    /// Sync-only mode - participates in Raft but doesn't serve web traffic
    /// Used for: new nodes joining, canary nodes during upgrade
    SyncOnly,

    /// Draining mode - stops accepting new requests, finishes in-flight
    /// Used for: graceful shutdown before upgrade
    Draining { deadline: Instant },

    /// Upgrading mode - node is restarting with new binary
    /// Other nodes track this to know node will return
    Upgrading {
        target_version: Version,
        started_at: Instant,
    },

    /// Validation mode - new binary running, performing self-checks
    /// Not yet serving traffic
    Validating {
        version: Version,
        checks_completed: Vec<String>,
        checks_remaining: Vec<String>,
    },

    /// Ready mode - upgrade complete, waiting for cluster coordination
    Ready { version: Version },
}
```

### 2.2 Version Structure

```rust
/// Semantic version with schema hash for integrity
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
```

---

## 3. Cluster Messages

### 3.1 Extended Raft Messages

```rust
/// Extended cluster messages for upgrade coordination
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClusterMessage {
    // === Existing Raft Messages ===
    AppendEntries(AppendEntriesRequest),
    AppendEntriesResponse(AppendEntriesResponse),
    RequestVote(RequestVoteRequest),
    RequestVoteResponse(RequestVoteResponse),
    Heartbeat(HeartbeatMessage),

    // === New: Upgrade Coordination Messages ===

    /// Announce intention to upgrade (from operator/CLI)
    UpgradeProposal {
        proposed_by: u64,           // node_id proposing upgrade
        target_version: Version,
        binary_url: Option<String>, // URL to download new binary
        migration_plan: MigrationPlan,
    },

    /// Canary node reports validation status
    CanaryStatus {
        node_id: u64,
        version: Version,
        status: CanaryValidationStatus,
        checks: Vec<ValidationCheck>,
    },

    /// Leader approves cluster-wide rollout
    UpgradeApproved {
        version: Version,
        approved_at: u64,           // timestamp
        rollout_order: Vec<u64>,    // node_ids in upgrade order
    },

    /// Node reports its upgrade progress
    NodeUpgradeStatus {
        node_id: u64,
        status: NodeUpgradeState,
        version: Version,
    },

    /// Abort upgrade and rollback
    UpgradeAbort {
        reason: String,
        initiated_by: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CanaryValidationStatus {
    Syncing,                    // Catching up with log
    MigrationRunning,           // Applying migration
    MigrationComplete,          // Migration done
    ValidationRunning,          // Running checks
    ValidationPassed,           // All checks passed
    ValidationFailed(String),   // Check failed with reason
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationCheck {
    pub name: String,
    pub status: CheckStatus,
    pub duration_ms: u64,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CheckStatus {
    Pending,
    Running,
    Passed,
    Failed(String),
    Skipped(String),
}
```

---

## 4. Migration System

### 4.1 Migration as Consensus Log Entry

```rust
/// CRUD operations extended with migration support
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CrudOperation {
    Create { model_path: String, data: serde_json::Value },
    Update { model_path: String, id: String, data: serde_json::Value },
    Delete { model_path: String, id: String },

    // === New: Schema Migration Operations ===

    /// Begin migration transaction
    MigrationBegin {
        from_version: Version,
        to_version: Version,
        migration_id: Uuid,
    },

    /// Individual migration step (applied in order)
    MigrationStep {
        migration_id: Uuid,
        step_index: u32,
        operation: SchemaChange,
    },

    /// Commit migration (all steps succeeded)
    MigrationCommit {
        migration_id: Uuid,
        checksum: String,  // Hash of final state for verification
    },

    /// Rollback migration (step failed)
    MigrationRollback {
        migration_id: Uuid,
        failed_step: u32,
        reason: String,
    },
}

/// Schema change operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SchemaChange {
    /// Add a new model/table
    AddModel {
        name: String,
        schema: ModelSchema,
    },

    /// Remove a model/table
    RemoveModel {
        name: String,
        backup_path: Option<String>,
    },

    /// Add field to existing model
    AddField {
        model: String,
        field: FieldDefinition,
        default_value: Option<serde_json::Value>,
    },

    /// Remove field from model
    RemoveField {
        model: String,
        field: String,
    },

    /// Rename field
    RenameField {
        model: String,
        old_name: String,
        new_name: String,
    },

    /// Change field type (with transformation)
    ChangeFieldType {
        model: String,
        field: String,
        new_type: FieldType,
        transform: Option<String>,  // Expression for data transformation
    },

    /// Add index
    AddIndex {
        model: String,
        fields: Vec<String>,
        unique: bool,
    },

    /// Custom SQL/operation for complex migrations
    Custom {
        description: String,
        forward: String,   // Forward migration code/expression
        backward: String,  // Rollback code/expression
    },
}
```

### 4.2 Migration Plan

```rust
/// Complete migration plan generated from version diff
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationPlan {
    pub id: Uuid,
    pub from_version: Version,
    pub to_version: Version,
    pub steps: Vec<MigrationStep>,
    pub estimated_duration_ms: u64,
    pub requires_downtime: bool,  // true if migration can't be done online
    pub backup_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationStep {
    pub index: u32,
    pub operation: SchemaChange,
    pub reversible: bool,
    pub estimated_ms: u64,
    pub affects_models: Vec<String>,
}

impl MigrationPlan {
    /// Generate plan from version diff
    pub fn from_versions(from: &Version, to: &Version) -> Result<Self, MigrationError> {
        // Compare schema_hash to determine required changes
        // This would use schema introspection from the declarative models
        todo!("Generate migration plan from schema diff")
    }

    /// Validate plan is safe to execute
    pub fn validate(&self) -> Result<(), MigrationError> {
        // Check all steps are reversible
        // Check no data loss operations without backup
        // Check estimated duration is acceptable
        todo!("Validate migration plan")
    }
}
```

---

## 5. Upgrade Protocol

### 5.1 Phase 1: Proposal

```
┌─────────────────────────────────────────────────────────────────┐
│                     PHASE 1: PROPOSAL                           │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  Operator                     Leader                            │
│     │                           │                               │
│     │  POST /_cluster/upgrade   │                               │
│     │  { version, binary_url }  │                               │
│     │ ─────────────────────────>│                               │
│     │                           │                               │
│     │                           │ 1. Validate new version       │
│     │                           │ 2. Generate MigrationPlan     │
│     │                           │ 3. Check all nodes healthy    │
│     │                           │                               │
│     │                           │ 4. Broadcast UpgradeProposal  │
│     │                           │ ──────────────────────────>   │
│     │                           │         to all followers      │
│     │                           │                               │
│     │  { status: "proposed",    │                               │
│     │    migration_plan: ... }  │                               │
│     │ <─────────────────────────│                               │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### 5.2 Phase 2: Canary Deployment

```
┌─────────────────────────────────────────────────────────────────┐
│                  PHASE 2: CANARY DEPLOYMENT                     │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  1. Spawn canary node with new binary (NodeMode::SyncOnly)      │
│                                                                 │
│     Canary                      Leader         Other Nodes      │
│        │                          │                │            │
│        │  Join cluster (sync)     │                │            │
│        │ ────────────────────────>│                │            │
│        │                          │                │            │
│        │  Receive all log entries │                │            │
│        │ <────────────────────────│                │            │
│        │                          │                │            │
│        │  [Apply MigrationBegin]  │                │            │
│        │  [Apply MigrationSteps]  │                │            │
│        │  [Apply MigrationCommit] │                │            │
│        │                          │                │            │
│        │  CanaryStatus: Synced    │                │            │
│        │ ────────────────────────>│                │            │
│        │                          │                │            │
│                                                                 │
│  2. Run validation checks                                       │
│                                                                 │
│        │  [Run ValidationChecks]  │                │            │
│        │   - Schema integrity     │                │            │
│        │   - Data consistency     │                │            │
│        │   - API compatibility    │                │            │
│        │   - Performance baseline │                │            │
│        │                          │                │            │
│        │  CanaryStatus: Validated │                │            │
│        │ ────────────────────────>│ ─────────────>│            │
│        │                          │  (broadcast)  │            │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### 5.3 Phase 3: Rolling Upgrade

```
┌─────────────────────────────────────────────────────────────────┐
│                  PHASE 3: ROLLING UPGRADE                       │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  For each node in rollout_order (one at a time):               │
│                                                                 │
│  Step 3.1: Drain node                                          │
│     Node                        Leader                          │
│       │  NodeMode::Draining      │                              │
│       │ ────────────────────────>│                              │
│       │                          │                              │
│       │  [Finish in-flight req]  │                              │
│       │  [Stop accepting new]    │                              │
│       │                          │                              │
│                                                                 │
│  Step 3.2: Verify quorum maintained                            │
│       │                          │                              │
│       │                          │ Check: remaining_nodes >= ⌈n/2⌉ + 1 │
│       │                          │ If not: ABORT upgrade        │
│       │                          │                              │
│                                                                 │
│  Step 3.3: Upgrade node                                        │
│       │  NodeMode::Upgrading     │                              │
│       │ ────────────────────────>│                              │
│       │                          │                              │
│       │  [Shutdown]              │                              │
│       │  [Replace binary]        │                              │
│       │  [Restart]               │                              │
│       │                          │                              │
│       │  NodeMode::Validating    │                              │
│       │ ────────────────────────>│                              │
│       │                          │                              │
│       │  [Sync from leader]      │                              │
│       │  [Verify data integrity] │                              │
│       │                          │                              │
│       │  NodeMode::Ready         │                              │
│       │ ────────────────────────>│                              │
│       │                          │                              │
│                                                                 │
│  Step 3.4: Resume serving                                      │
│       │                          │                              │
│       │  UpgradeApproved (node)  │                              │
│       │ <────────────────────────│                              │
│       │                          │                              │
│       │  NodeMode::Serving       │                              │
│       │ ────────────────────────>│                              │
│       │                          │                              │
│  [Repeat for next node in rollout_order]                       │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### 5.4 Phase 4: Completion

```
┌─────────────────────────────────────────────────────────────────┐
│                    PHASE 4: COMPLETION                          │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  When all nodes upgraded:                                       │
│                                                                 │
│  1. Leader broadcasts: UpgradeComplete { version }              │
│  2. Remove canary node (or promote to permanent)                │
│  3. Clean up old binary artifacts                               │
│  4. Update cluster metadata with new version                    │
│                                                                 │
│  Verification:                                                  │
│  - All nodes report same version                               │
│  - All nodes report NodeMode::Serving                          │
│  - Cluster health check passes                                 │
│  - No data inconsistency detected                              │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

---

## 6. Rollback Scenarios

### 6.1 Canary Failure

```rust
/// Triggered when canary validation fails
async fn handle_canary_failure(
    cluster: &ClusterState,
    canary_id: u64,
    reason: String,
) -> Result<(), UpgradeError> {
    log::error!("Canary validation failed: {}", reason);

    // 1. Remove canary from cluster
    cluster.remove_node(canary_id).await?;

    // 2. Broadcast abort to all nodes
    cluster.broadcast(ClusterMessage::UpgradeAbort {
        reason: format!("Canary validation failed: {}", reason),
        initiated_by: cluster.leader_id(),
    }).await?;

    // 3. No rollback needed - cluster never changed
    log::info!("Upgrade aborted, cluster unchanged");

    Ok(())
}
```

### 6.2 Mid-Rollout Failure

```rust
/// Triggered when a node fails during rolling upgrade
async fn handle_rollout_failure(
    cluster: &ClusterState,
    failed_node: u64,
    reason: String,
    upgraded_nodes: &[u64],
) -> Result<(), UpgradeError> {
    log::error!("Node {} failed during upgrade: {}", failed_node, reason);

    // 1. Check if we have quorum with remaining nodes
    let healthy_count = cluster.healthy_node_count().await;
    let quorum = cluster.quorum_size();

    if healthy_count >= quorum {
        // Option A: Continue without failed node, repair later
        log::warn!("Continuing upgrade, will repair node {} later", failed_node);
        cluster.mark_node_unhealthy(failed_node).await?;
        return Ok(());
    }

    // 2. Must rollback - not enough healthy nodes
    log::error!("Must rollback: only {} healthy nodes, need {}", healthy_count, quorum);

    // 3. Rollback upgraded nodes in reverse order
    for node_id in upgraded_nodes.iter().rev() {
        cluster.send_to_node(*node_id, ClusterMessage::Rollback {
            target_version: cluster.previous_version(),
        }).await?;

        // Wait for node to restart with old binary
        cluster.wait_for_node_healthy(*node_id, Duration::from_secs(60)).await?;
    }

    // 4. Broadcast abort
    cluster.broadcast(ClusterMessage::UpgradeAbort {
        reason,
        initiated_by: cluster.leader_id(),
    }).await?;

    Ok(())
}
```

---

## 7. Validation Checks

### 7.1 Standard Checks

```rust
/// Standard validation checks for canary and upgraded nodes
pub struct ValidationSuite {
    checks: Vec<Box<dyn ValidationCheck>>,
}

impl Default for ValidationSuite {
    fn default() -> Self {
        Self {
            checks: vec![
                Box::new(SchemaIntegrityCheck),
                Box::new(DataConsistencyCheck),
                Box::new(ApiCompatibilityCheck),
                Box::new(PerformanceBaselineCheck),
                Box::new(ReplicationHealthCheck),
            ],
        }
    }
}

/// Check that schema matches expected hash
struct SchemaIntegrityCheck;
impl ValidationCheck for SchemaIntegrityCheck {
    fn name(&self) -> &str { "schema_integrity" }

    async fn run(&self, node: &NodeState) -> CheckResult {
        let expected = node.target_version().schema_hash;
        let actual = node.compute_schema_hash();

        if expected == actual {
            CheckResult::Passed
        } else {
            CheckResult::Failed(format!(
                "Schema hash mismatch: expected {}, got {}",
                expected, actual
            ))
        }
    }
}

/// Check that all data can be read and validates
struct DataConsistencyCheck;
impl ValidationCheck for DataConsistencyCheck {
    fn name(&self) -> &str { "data_consistency" }

    async fn run(&self, node: &NodeState) -> CheckResult {
        for model in node.models() {
            // Try to read all records
            let records = model.read_all().await?;

            // Validate each record against new schema
            for record in records {
                if let Err(e) = model.validate(&record) {
                    return CheckResult::Failed(format!(
                        "Record {} in {} failed validation: {}",
                        record.id, model.name, e
                    ));
                }
            }
        }
        CheckResult::Passed
    }
}

/// Check that API endpoints respond correctly
struct ApiCompatibilityCheck;
impl ValidationCheck for ApiCompatibilityCheck {
    fn name(&self) -> &str { "api_compatibility" }

    async fn run(&self, node: &NodeState) -> CheckResult {
        // Run API test suite against local endpoints
        let test_results = node.run_api_tests().await;

        if test_results.all_passed() {
            CheckResult::Passed
        } else {
            CheckResult::Failed(format!(
                "API tests failed: {:?}",
                test_results.failures()
            ))
        }
    }
}
```

---

## 8. CLI Interface

### 8.1 Upgrade Commands

```bash
# Propose an upgrade (interactive)
lithair cluster upgrade --to-version 1.2.0

# Propose upgrade with binary URL
lithair cluster upgrade --to-version 1.2.0 --binary-url https://releases.example.com/lithair-1.2.0

# Check upgrade status
lithair cluster upgrade status

# Abort ongoing upgrade
lithair cluster upgrade abort --reason "Found critical bug"

# Force upgrade a specific node (dangerous)
lithair cluster upgrade node --node-id 2 --force

# View upgrade history
lithair cluster upgrade history
```

### 8.2 Status Output

```
$ lithair cluster upgrade status

╔══════════════════════════════════════════════════════════════════╗
║                    CLUSTER UPGRADE STATUS                        ║
╠══════════════════════════════════════════════════════════════════╣
║ Current Version: 1.1.0 (schema: a1b2c3d4)                       ║
║ Target Version:  1.2.0 (schema: e5f6g7h8)                       ║
║ Phase:          Rolling Upgrade (3/5 nodes complete)            ║
║ Started:        2024-01-15 14:30:00 UTC                         ║
║ ETA:            ~5 minutes remaining                            ║
╠══════════════════════════════════════════════════════════════════╣
║ Node │ Status      │ Version │ Mode      │ Health              ║
╠══════╪═════════════╪═════════╪═══════════╪═════════════════════╣
║  0   │ ✓ Upgraded  │ 1.2.0   │ Serving   │ Healthy             ║
║  1   │ ✓ Upgraded  │ 1.2.0   │ Serving   │ Healthy             ║
║  2   │ ✓ Upgraded  │ 1.2.0   │ Serving   │ Healthy             ║
║  3   │ ⟳ Upgrading │ 1.2.0   │ Validating│ Checking...         ║
║  4   │ ○ Pending   │ 1.1.0   │ Serving   │ Healthy             ║
╚══════════════════════════════════════════════════════════════════╝

Migration Progress:
  [████████████████████░░░░░░░░░░░░] 65% (13/20 steps)
  Current: Adding index on users.email

Validation Checks (Node 3):
  ✓ schema_integrity      (0.2s)
  ✓ data_consistency      (3.4s)
  ⟳ api_compatibility     (running...)
  ○ performance_baseline  (pending)
  ○ replication_health    (pending)
```

---

## 9. Implementation Phases

### Phase 1: Foundation (Priority: High)
- [ ] Add `Version` struct with schema hash
- [ ] Add `NodeMode` enum to `RaftLeadershipState`
- [ ] Add version to cluster status endpoint
- [ ] Implement version compatibility checks

### Phase 2: Migration Log (Priority: High)
- [ ] Extend `CrudOperation` with migration variants
- [ ] Implement `MigrationPlan` generation from schema diff
- [ ] Add migration apply logic to `apply_crud_operation`
- [ ] Implement migration rollback

### Phase 3: Canary System (Priority: Medium)
- [ ] Implement `SyncOnly` node mode
- [ ] Add canary validation suite
- [ ] Implement canary coordination messages
- [ ] Add canary status reporting

### Phase 4: Rolling Upgrade (Priority: Medium)
- [ ] Implement node draining
- [ ] Implement rolling upgrade coordinator
- [ ] Add quorum monitoring during upgrade
- [ ] Implement automatic rollback

### Phase 5: CLI & Observability (Priority: Low)
- [ ] Add `lithair cluster upgrade` CLI
- [ ] Add upgrade status endpoint
- [ ] Add upgrade metrics/events
- [ ] Add upgrade history/audit log

---

## 10. Open Questions

1. **Binary Distribution**: How to distribute new binary to nodes?
   - Option A: External system (CI/CD pushes)
   - Option B: Built-in P2P transfer
   - Option C: Pull from artifact repository

2. **Schema Diff Generation**: How to generate migration from schema changes?
   - Option A: Manual migration files
   - Option B: Auto-diff from `#[derive(DeclarativeModel)]`
   - Option C: Both (auto-generate, allow manual override)

3. **Downtime Migrations**: Some migrations can't be done online (rename table, etc.)
   - Option A: Reject non-online migrations
   - Option B: Allow with explicit `--allow-downtime` flag
   - Option C: Blue/green deployment for these cases

4. **Multi-Version Compatibility Window**: How long to support old versions?
   - Read compatibility: N-1 versions
   - Write compatibility: Current version only
   - API compatibility: Configurable deprecation period

---

## Appendix A: Example Migration

```rust
// models.rs v1.0
#[derive(DeclarativeModel)]
pub struct User {
    #[db(primary_key)]
    pub id: Uuid,
    pub name: String,
    pub email: String,
}

// models.rs v1.1 - Added `created_at` field
#[derive(DeclarativeModel)]
pub struct User {
    #[db(primary_key)]
    pub id: Uuid,
    pub name: String,
    pub email: String,
    #[db(default = "now()")]
    pub created_at: DateTime<Utc>,  // NEW
}
```

Auto-generated migration:

```rust
MigrationPlan {
    from_version: Version { major: 1, minor: 0, patch: 0, .. },
    to_version: Version { major: 1, minor: 1, patch: 0, .. },
    steps: vec![
        MigrationStep {
            index: 0,
            operation: SchemaChange::AddField {
                model: "User".to_string(),
                field: FieldDefinition {
                    name: "created_at".to_string(),
                    field_type: FieldType::DateTime,
                    nullable: false,
                },
                default_value: Some(json!("now()")),
            },
            reversible: true,
            estimated_ms: 100,
            affects_models: vec!["User".to_string()],
        },
    ],
    estimated_duration_ms: 100,
    requires_downtime: false,
    backup_required: false,
}
```

---

## Appendix B: State Machine

```
                    ┌─────────────┐
                    │   STABLE    │
                    │  (v1.0.0)   │
                    └──────┬──────┘
                           │ UpgradeProposal
                           ▼
                    ┌─────────────┐
                    │  PROPOSED   │
                    │  (v1.1.0)   │
                    └──────┬──────┘
                           │ Canary deployed
                           ▼
                    ┌─────────────┐
          Abort ◄───│   CANARY    │
            │       │ VALIDATING  │
            │       └──────┬──────┘
            │              │ Validation passed
            │              ▼
            │       ┌─────────────┐
            │       │  APPROVED   │
            │       │  (v1.1.0)   │
            │       └──────┬──────┘
            │              │ Start rolling upgrade
            │              ▼
            │       ┌─────────────┐
            ├───────│  ROLLING    │───────┐
            │       │  UPGRADE    │       │ Node failure
            │       └──────┬──────┘       │
            │              │ All nodes    │
            │              │ upgraded     │
            │              ▼              ▼
            │       ┌─────────────┐ ┌───────────┐
            │       │  COMPLETE   │ │ ROLLBACK  │
            │       │  (v1.1.0)   │ │  (v1.0.0) │
            │       └─────────────┘ └─────┬─────┘
            │                             │
            └─────────────────────────────┘
```
