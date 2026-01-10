# Migration System Overview

Lithair provides a built-in schema migration system for safely evolving data models over time while maintaining data integrity and cluster consistency.

## Architecture

```text
┌─────────────────────────────────────────────────────────────────┐
│                    DeclarativeModel (macro)                      │
│  #[derive(DeclarativeModel)]                                     │
│  pub struct Product { id: Uuid, name: String, price: f64 }       │
└─────────────────────────────┬───────────────────────────────────┘
                              │ generates impl DeclarativeSpecExtractor
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                        ModelSpec                                 │
│  Extracted specification of model structure:                     │
│  - model_name, version, fields, indexes, foreign_keys           │
└─────────────────────────────┬───────────────────────────────────┘
                              │ compare(stored_spec, current_spec)
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                  SchemaChangeDetector                            │
│  Detects differences and generates migration plan:               │
│  - Change type (add/remove/modify)                               │
│  - Migration strategy (additive/breaking/versioned)              │
│  - SQL statements + rollback statements                          │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                  MigrationManager                                │
│  Orchestrates migration execution:                               │
│  - Transaction management                                        │
│  - Step-by-step execution with rollback log                      │
│  - Cluster coordination for breaking changes                     │
└─────────────────────────────────────────────────────────────────┘
```

## Core Concepts

### Schema Change Types

| Type | Description | Example |
|------|-------------|---------|
| `AddField` | New field added to model | `+ description: String` |
| `RemoveField` | Field removed from model | `- legacy_field` |
| `ModifyFieldType` | Field type changed | `count: i32 → i64` |
| `ModifyFieldConstraints` | Constraints changed | `nullable → not null` |
| `AddIndex` | New index created | `+ INDEX(email)` |
| `RemoveIndex` | Index dropped | `- INDEX(old_field)` |
| `AddForeignKey` | New FK relationship | `+ FK(user_id → users.id)` |
| `RemoveForeignKey` | FK relationship removed | `- FK(category_id)` |
| `ModifyRetentionPolicy` | Retention rules changed | `30d → 90d` |
| `ModifyPermissions` | RBAC permissions changed | `Admin → Public` |

### Migration Strategies

| Strategy | Description | Consensus Required |
|----------|-------------|-------------------|
| `Additive` | Backward compatible (add nullable field) | No |
| `Breaking` | Requires data migration (remove field, add NOT NULL) | Yes |
| `Versioned` | Multiple versions coexist during transition | Partial |

### Migration Status

```rust
pub enum MigrationStatus {
    InProgress,           // Migration is running
    Committed,            // Successfully completed
    RolledBack,           // Rolled back to previous state
    Failed { reason },    // Failed with error
}
```

## Components

### ModelSpec

Represents the complete specification of a model extracted from annotations:

```rust
pub struct ModelSpec {
    pub model_name: String,
    pub version: u32,
    pub fields: HashMap<String, FieldConstraints>,
    pub indexes: Vec<IndexSpec>,
    pub foreign_keys: Vec<ForeignKeySpec>,
}
```

### FieldConstraints

All constraints that can be applied to a field:

```rust
pub struct FieldConstraints {
    pub primary_key: bool,
    pub unique: bool,
    pub indexed: bool,
    pub foreign_key: Option<String>,
    pub nullable: bool,
    pub immutable: bool,
    pub audited: bool,
    pub versioned: bool,
    pub retention: String,
    pub snapshot_only: bool,
    pub validation_rules: Vec<String>,
    pub permissions: Option<FieldPermissions>,
}
```

### DetectedSchemaChange

Rich description of a detected change with migration metadata:

```rust
pub struct DetectedSchemaChange {
    pub model: String,
    pub change_type: SchemaChangeType,
    pub field_name: Option<String>,
    pub old_type: Option<String>,
    pub new_type: Option<String>,
    pub old_constraints: Option<FieldConstraints>,
    pub new_constraints: Option<FieldConstraints>,
    pub migration_strategy: MigrationStrategy,
    pub default_value: Option<String>,
    pub requires_consensus: bool,
    pub migration_sql: Option<String>,
    pub rollback_sql: Option<String>,
}
```

### MigrationContext

Tracks state of an active migration:

```rust
pub struct MigrationContext {
    pub id: Uuid,
    pub from_version: Version,
    pub to_version: Version,
    pub status: MigrationStatus,
    pub current_step: usize,
    pub total_steps: usize,
    pub rollback_log: Vec<RollbackOp>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}
```

## Usage

### Detecting Schema Changes

```rust
use lithair_core::schema::{SchemaChangeDetector, ModelSpec};

// Compare old and new specifications
let old_spec = load_stored_spec("Product")?;
let new_spec = Product::extract_schema_spec();

let changes = SchemaChangeDetector::detect_changes(&old_spec, &new_spec);

for change in &changes {
    println!("Detected: {:?} on {}", change.change_type, change.model);
    println!("  Strategy: {:?}", change.migration_strategy);
    println!("  Requires consensus: {}", change.requires_consensus);
    if let Some(sql) = &change.migration_sql {
        println!("  SQL: {}", sql);
    }
}
```

### Managing Migrations

```rust
use lithair_core::cluster::{MigrationManager, Version};

let mut manager = MigrationManager::new(Version::new(1, 0, 0, None, None));

// Begin a migration
let migration_id = manager.begin_migration(
    Version::new(1, 0, 0, None, None),
    Version::new(1, 1, 0, None, None),
)?;

// Record steps as they execute
manager.record_step(migration_id, "add_field_description".to_string())?;
manager.record_step(migration_id, "create_index_email".to_string())?;

// Commit or rollback
if all_steps_successful {
    manager.commit_migration(migration_id)?;
} else {
    manager.rollback_migration(migration_id)?;
}
```

## Decision Logic

### When Consensus is Required

Breaking changes require cluster-wide consensus before execution:

```rust
fn requires_consensus_for_add(constraints: &FieldConstraints) -> bool {
    constraints.primary_key ||    // New PK affects all nodes
    constraints.unique ||         // Unique constraint needs validation
    !constraints.nullable         // NOT NULL needs default value
}
```

### Migration Strategy Selection

```rust
fn determine_migration_strategy(constraints: &FieldConstraints) -> MigrationStrategy {
    if constraints.primary_key || constraints.unique {
        MigrationStrategy::Breaking    // Needs consensus
    } else if constraints.nullable {
        MigrationStrategy::Additive    // Safe, no consensus
    } else {
        MigrationStrategy::Versioned   // Needs default value
    }
}
```

## Rollback Support

Every migration step is recorded with rollback information:

```rust
pub struct RollbackOp {
    pub step_index: usize,
    pub operation: String,
    pub data_snapshot: Option<Vec<u8>>,
}
```

On failure, the `MigrationManager` can replay rollback operations in reverse order to restore the previous state.

## Integration Points

| Component | Integration |
|-----------|-------------|
| `LithairServer` | Holds `MigrationManager` instance |
| `DeclarativeModel` | Implements `DeclarativeSpecExtractor` |
| Replication | Breaking changes coordinate via Raft |
| Persistence | Schema version stored with data |

## See Also

- [Schema Detection](./schema-detection.md) - Detailed change detection logic
- [Execution Flow](./execution-flow.md) - Step-by-step migration execution
- [Clustering](../clustering/overview.md) - Consensus for breaking changes
- [Schema Evolution](../declarative/schema-evolution.md) - Best practices
