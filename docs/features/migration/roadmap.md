# Migration System: Gaps & Roadmap

Current implementation status and planned improvements.

## Implementation Status

| Component | Status | Notes |
|-----------|--------|-------|
| SchemaChangeDetector | ✅ Complete | All change types detected |
| MigrationManager | ✅ Complete | Begin/commit/rollback working |
| MigrationContext | ✅ Complete | State tracking with rollback log |
| DeclarativeSpecExtractor | ✅ Complete | Trait + macro implementation |
| ModelSpec extraction | ✅ Complete | Fields, indexes, FKs extracted |
| SQL generation | ⚠️ Partial | Templates exist, types TODO |
| Version attribute | ❌ Missing | Hardcoded to 1 |
| Startup validation | ❌ Missing | No stored vs current comparison |
| Migration execution | ❌ Missing | Detection only, no apply |
| Schema persistence | ❌ Missing | Specs not saved to disk |
| Cluster coordination | ❌ Missing | Consensus not integrated |

## Gap 1: Schema Version Attribute

### Current State

```rust
// lithair-macros/src/declarative_simple.rs:996
fn schema_version(&self) -> u32 {
    1 // TODO: extraire depuis un attribut #[schema(version = 2)]
}
```

### Required

Support for explicit version declaration:

```rust
#[derive(DeclarativeModel)]
#[schema(version = 2)]
pub struct Product {
    // ...
}
```

### Implementation Steps

1. Add `#[schema(version = N)]` attribute parsing in macro
2. Extract version during proc-macro expansion
3. Use extracted version in `schema_version()` implementation
4. Add compile-time validation (version must increase)

### Files to Modify

- `lithair-macros/src/declarative_simple.rs` - Parse attribute
- `lithair-macros/src/lib.rs` - Export schema attribute

---

## Gap 2: Type Inference

### Current State

```rust
// lithair-core/src/schema/mod.rs:132
new_type: Some("inferred".to_string()), // TODO: type inference
```

SQL generation uses placeholder:

```rust
// lithair-core/src/schema/mod.rs:394
format!("ALTER TABLE ADD COLUMN {} {} ...", field, "TYPE", ...)
```

### Required

Map Rust types to storage types:

| Rust Type | Storage Type |
|-----------|--------------|
| `String` | `TEXT` |
| `i32` | `INTEGER` |
| `i64` | `BIGINT` |
| `f64` | `DOUBLE` |
| `bool` | `BOOLEAN` |
| `Uuid` | `UUID` |
| `DateTime<Utc>` | `TIMESTAMP` |
| `Option<T>` | `T NULL` |
| `Vec<T>` | `JSONB` |

### Implementation Steps

1. Add `field_type: String` to `FieldConstraints`
2. Extract type info during macro expansion
3. Store type in `ModelSpec.fields`
4. Use type in SQL generation

### Files to Modify

- `lithair-macros/src/declarative_simple.rs` - Extract field types
- `lithair-core/src/schema/mod.rs` - FieldConstraints, SQL generation

---

## Gap 3: Startup Schema Validation

### Current State

No automatic comparison at server startup.

### Required

```rust
// Pseudocode for startup flow
async fn on_server_start() {
    for model in registered_models {
        let stored = load_stored_spec(model.name())?;
        let current = model.extract_schema_spec();

        let changes = SchemaChangeDetector::detect_changes(&stored, &current);

        if !changes.is_empty() {
            if auto_migrate_enabled {
                run_migration(changes).await?;
            } else {
                panic!("Schema mismatch for {}: {:?}", model.name(), changes);
            }
        }
    }
}
```

### Implementation Steps

1. Add schema storage path to `LithairServerBuilder`
2. Implement `save_schema_spec()` and `load_schema_spec()`
3. Add startup hook in `LithairServer::serve()`
4. Add config option: `auto_migrate: bool`

### Files to Modify

- `lithair-core/src/app/builder.rs` - Add schema path config
- `lithair-core/src/app/mod.rs` - Add startup validation hook
- `lithair-core/src/schema/mod.rs` - Add persistence functions

---

## Gap 4: Migration Execution

### Current State

`SchemaChangeDetector` produces `DetectedSchemaChange` with SQL, but nothing executes it.

### Required

```rust
async fn execute_change(change: &DetectedSchemaChange) -> Result<(), Error> {
    match change.change_type {
        SchemaChangeType::AddField => {
            // 1. Update in-memory model registry
            // 2. For persisted data: update rkyv schema
            // 3. Record in WAL for replication
        }
        SchemaChangeType::RemoveField => {
            // 1. Mark field as deprecated
            // 2. Skip field during serialization
            // 3. Clean up on next compaction
        }
        // ...
    }
}
```

### Implementation Steps

1. Create `MigrationExecutor` struct
2. Implement handlers for each `SchemaChangeType`
3. Integrate with rkyv schema evolution
4. Add hooks for pre/post migration events

### Files to Create

- `lithair-core/src/cluster/migration_executor.rs`

### Files to Modify

- `lithair-core/src/cluster/mod.rs` - Export executor

---

## Gap 5: Schema Persistence

### Current State

`ModelSpec` is extracted at runtime but not stored.

### Required

```rust
// Save after successful migration
fn save_schema_spec(spec: &ModelSpec) -> Result<(), Error> {
    let path = schema_path(&spec.model_name);
    let json = serde_json::to_string_pretty(spec)?;
    std::fs::write(path, json)?;
    Ok(())
}

// Load at startup
fn load_schema_spec(model_name: &str) -> Result<Option<ModelSpec>, Error> {
    let path = schema_path(model_name);
    if path.exists() {
        let json = std::fs::read_to_string(path)?;
        Ok(Some(serde_json::from_str(&json)?))
    } else {
        Ok(None)  // First run, no stored spec
    }
}
```

### Implementation Steps

1. Define schema storage location (e.g., `{data_dir}/.schema/`)
2. Implement save/load functions
3. Call save after migration commit
4. Call load at startup

### Files to Modify

- `lithair-core/src/schema/mod.rs` - Add persistence functions

---

## Gap 6: Cluster Coordination

### Current State

`requires_consensus: bool` is set but not used.

### Required

For breaking changes in clustered mode:

```rust
if change.requires_consensus {
    // 1. Send prepare-migration to all followers
    // 2. Wait for all nodes to acknowledge
    // 3. Execute migration on leader
    // 4. Send commit-migration to followers
    // 5. Followers apply same migration
}
```

### Implementation Steps

1. Add `/_raft/prepare-migration` endpoint
2. Add `/_raft/commit-migration` endpoint
3. Integrate with `ReplicationBatcher`
4. Add migration state to `FollowerState`

### Files to Modify

- `lithair-core/src/app/mod.rs` - Add migration endpoints
- `lithair-core/src/cluster/mod.rs` - Add migration replication

---

## Priority Order

| Priority | Gap | Effort | Impact |
|----------|-----|--------|--------|
| 1 | Schema Persistence | Low | High - enables all other features |
| 2 | Startup Validation | Medium | High - catches mismatches |
| 3 | Version Attribute | Low | Medium - user experience |
| 4 | Type Inference | Medium | Medium - better SQL |
| 5 | Migration Execution | High | High - actually applies changes |
| 6 | Cluster Coordination | High | High - distributed consistency |

## Recommended Implementation Order

### Phase 1: Foundation (Gaps 1, 5, 3)

1. **Schema Persistence** - Save/load ModelSpec to disk
2. **Startup Validation** - Compare stored vs current at boot
3. **Version Attribute** - `#[schema(version = N)]` support

Outcome: System detects mismatches and warns developer.

### Phase 2: Execution (Gaps 4, 2)

1. **Type Inference** - Map Rust types to storage types
2. **Migration Execution** - Apply detected changes

Outcome: System can automatically migrate simple changes.

### Phase 3: Distribution (Gap 6)

1. **Cluster Coordination** - Consensus for breaking changes

Outcome: Full distributed migration support.

## Testing Strategy

### Unit Tests

- Version comparison and compatibility
- Schema change detection (all types)
- Migration context state transitions
- Rollback operation ordering

### Integration Tests

- Startup with matching schema (no migration)
- Startup with additive change (auto-migrate)
- Startup with breaking change (error or consensus)
- Rollback after failed migration step

### Cluster Tests

- Rolling upgrade with additive changes
- Coordinated migration with breaking changes
- Partial failure and recovery

## See Also

- [Migration Overview](./overview.md)
- [Schema Detection](./schema-detection.md)
- [Execution Flow](./execution-flow.md)
