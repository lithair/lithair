# Migration Execution Flow

How migrations are executed, tracked, and rolled back.

## Overview

The `MigrationManager` orchestrates migration execution with full transaction support and rollback capability.

```text
┌──────────────┐    ┌──────────────┐    ┌──────────────┐
│ begin_       │───▶│ record_step  │───▶│ commit_      │
│ migration    │    │ (repeat)     │    │ migration    │
└──────────────┘    └──────┬───────┘    └──────────────┘
                          │
                          ▼ (on failure)
                   ┌──────────────┐
                   │ rollback_    │
                   │ migration    │
                   └──────────────┘
```

## MigrationManager

### Structure

```rust
pub struct MigrationManager {
    active: Option<MigrationContext>,
    completed: Vec<MigrationContext>,
    current_version: Version,
}
```

### Starting a Migration

```rust
impl MigrationManager {
    pub fn begin_migration(
        &mut self,
        from_version: Version,
        to_version: Version,
    ) -> Result<Uuid, String> {
        // Check no active migration
        if self.has_active_migration() {
            return Err("Migration already in progress");
        }

        // Create new context
        let context = MigrationContext::new(from_version, to_version);
        let id = context.id;
        self.active = Some(context);
        Ok(id)
    }
}
```

### Recording Steps

Each migration step is recorded with rollback information:

```rust
pub fn record_step(
    &mut self,
    migration_id: Uuid,
    operation: String,
) -> Result<(), String> {
    if let Some(ctx) = &mut self.active {
        if ctx.id == migration_id {
            ctx.record_step(operation);
            return Ok(());
        }
    }
    Err("Migration not found or not active")
}
```

### MigrationContext Step Recording

```rust
impl MigrationContext {
    pub fn record_step(&mut self, operation: String) {
        self.rollback_log.push(RollbackOp {
            step_index: self.current_step,
            operation,
            data_snapshot: None,  // Optional data backup
        });
        self.current_step += 1;
        self.updated_at_ms = now_ms();
    }
}
```

## Commit vs Rollback

### Successful Commit

```rust
pub fn commit_migration(&mut self, migration_id: Uuid) -> Result<(), String> {
    if let Some(mut ctx) = self.active.take() {
        if ctx.id == migration_id {
            ctx.commit();  // Sets status to Committed
            self.current_version = ctx.to_version.clone();
            self.completed.push(ctx);
            return Ok(());
        }
        self.active = Some(ctx);  // Put back if wrong ID
    }
    Err("Migration not found")
}
```

### Rollback on Failure

```rust
pub fn rollback_migration(&mut self, migration_id: Uuid) -> Result<(), String> {
    if let Some(mut ctx) = self.active.take() {
        if ctx.id == migration_id {
            // Get rollback operations in reverse order
            let ops = ctx.get_rollback_ops();  // Reversed

            // Execute rollback (caller must implement)
            for op in ops {
                // Apply rollback operation...
            }

            ctx.rollback();  // Sets status to RolledBack
            self.completed.push(ctx);
            return Ok(());
        }
        self.active = Some(ctx);
    }
    Err("Migration not found")
}
```

## MigrationContext States

```rust
impl MigrationContext {
    pub fn commit(&mut self) {
        self.status = MigrationStatus::Committed;
        self.updated_at_ms = now_ms();
    }

    pub fn rollback(&mut self) {
        self.status = MigrationStatus::RolledBack;
        self.updated_at_ms = now_ms();
    }

    pub fn fail(&mut self, reason: String) {
        self.status = MigrationStatus::Failed { reason };
        self.updated_at_ms = now_ms();
    }
}
```

## Version Management

### Version Structure

```rust
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub schema_hash: Option<String>,
    pub build_id: Option<String>,
}
```

### Compatibility Checks

```rust
impl Version {
    /// Can this version read data from another version?
    pub fn can_read_from(&self, other: &Version) -> bool {
        self.major == other.major  // Same major = compatible
    }

    /// Does data from other version require migration?
    pub fn requires_migration_from(&self, other: &Version) -> bool {
        self.major != other.major ||
        self.minor != other.minor ||
        self.schema_hash != other.schema_hash
    }
}
```

## Rollback Operations

### RollbackOp Structure

```rust
pub struct RollbackOp {
    pub step_index: usize,
    pub operation: String,
    pub data_snapshot: Option<Vec<u8>>,
}
```

### Getting Rollback Operations

```rust
impl MigrationContext {
    pub fn get_rollback_ops(&self) -> Vec<RollbackOp> {
        let mut ops = self.rollback_log.clone();
        ops.reverse();  // LIFO order for rollback
        ops
    }
}
```

## Complete Migration Flow Example

```rust
use lithair_core::cluster::{MigrationManager, Version};
use lithair_core::schema::SchemaChangeDetector;

async fn migrate_model<T: DeclarativeSpecExtractor>(
    manager: &mut MigrationManager,
    stored_spec: &ModelSpec,
) -> Result<(), String> {
    // 1. Detect changes
    let current_spec = T::extract_schema_spec();
    let changes = SchemaChangeDetector::detect_changes(stored_spec, &current_spec);

    if changes.is_empty() {
        return Ok(());  // No migration needed
    }

    // 2. Check for breaking changes requiring consensus
    let needs_consensus = changes.iter().any(|c| c.requires_consensus);
    if needs_consensus {
        // Coordinate with cluster...
        request_cluster_consensus().await?;
    }

    // 3. Begin migration
    let from = Version::new(stored_spec.version, 0, 0, None, None);
    let to = Version::new(current_spec.version, 0, 0, None, None);
    let migration_id = manager.begin_migration(from, to)?;

    // 4. Execute each change
    for change in &changes {
        let step_name = format!("{:?}_{}",
            change.change_type,
            change.field_name.as_deref().unwrap_or("model")
        );

        // Execute the change
        if let Err(e) = execute_change(change).await {
            // Rollback on failure
            manager.rollback_migration(migration_id)?;
            return Err(format!("Migration failed at {}: {}", step_name, e));
        }

        // Record successful step
        manager.record_step(migration_id, step_name)?;
    }

    // 5. Commit
    manager.commit_migration(migration_id)?;

    // 6. Persist new schema spec
    save_schema_spec(&current_spec)?;

    Ok(())
}
```

## Integration with Replication

For clustered deployments, breaking changes must coordinate:

```text
Leader                              Followers
  │                                    │
  │  1. Detect breaking change         │
  │  2. Begin migration                │
  │                                    │
  │  /_raft/prepare-migration          │
  ├───────────────────────────────────▶│
  │                                    │ Pause writes
  │  {ready: true}                     │
  │◀───────────────────────────────────┤
  │                                    │
  │  3. Execute migration              │
  │  4. Commit migration               │
  │                                    │
  │  /_raft/commit-migration           │
  ├───────────────────────────────────▶│
  │                                    │ Apply same migration
  │  {success: true}                   │ Resume writes
  │◀───────────────────────────────────┤
```

## Status Queries

```rust
impl MigrationManager {
    pub fn has_active_migration(&self) -> bool {
        self.active.is_some()
    }

    pub fn current_version(&self) -> &Version {
        &self.current_version
    }

    pub fn get_migration(&self, id: Uuid) -> Option<&MigrationContext> {
        if let Some(ref ctx) = self.active {
            if ctx.id == id {
                return Some(ctx);
            }
        }
        self.completed.iter().find(|c| c.id == id)
    }
}
```

## Source Code Reference

- `lithair-core/src/cluster/upgrade.rs:264-390` - MigrationManager
- `lithair-core/src/cluster/upgrade.rs:171-257` - MigrationContext
- `lithair-core/src/cluster/upgrade.rs:158-169` - MigrationStatus
- `lithair-core/src/cluster/upgrade.rs:14-67` - Version

## See Also

- [Migration Overview](./overview.md)
- [Schema Detection](./schema-detection.md)
- [Clustering Overview](../clustering/overview.md)
