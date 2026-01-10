# Schema Change Detection

Deep dive into how Lithair detects schema changes between model versions.

## Overview

The `SchemaChangeDetector` compares two `ModelSpec` instances and produces a list of `DetectedSchemaChange` objects describing every difference.

```rust
let changes = SchemaChangeDetector::detect_changes(&old_spec, &new_spec);
```

## Detection Algorithm

### 1. Field Addition Detection

```rust
// For each field in new_spec that doesn't exist in old_spec
for (field_name, new_constraints) in &new_spec.fields {
    if !old_spec.fields.contains_key(field_name) {
        // → AddField change detected
    }
}
```

**Output:**
```rust
DetectedSchemaChange {
    change_type: SchemaChangeType::AddField,
    field_name: Some("description"),
    migration_strategy: Additive,  // if nullable
    requires_consensus: false,
    migration_sql: Some("ALTER TABLE ADD COLUMN description TYPE NULL"),
    rollback_sql: Some("ALTER TABLE DROP COLUMN description"),
}
```

### 2. Field Removal Detection

```rust
// For each field in old_spec that doesn't exist in new_spec
for (field_name, old_constraints) in &old_spec.fields {
    if !new_spec.fields.contains_key(field_name) {
        // → RemoveField change detected (always Breaking)
    }
}
```

**Output:**
```rust
DetectedSchemaChange {
    change_type: SchemaChangeType::RemoveField,
    migration_strategy: Breaking,
    requires_consensus: true,
    // rollback_sql recreates the field
}
```

### 3. Constraint Change Detection

For fields that exist in both specs, compare constraints:

```rust
if old_constraints != new_constraints {
    detect_constraint_changes(model, field, old, new)
}
```

#### Retention Policy Changes

```rust
if old.retention != new.retention {
    // → ModifyRetentionPolicy (Additive, no consensus)
}
```

#### Permission Changes

```rust
if old.permissions != new.permissions {
    // → ModifyPermissions (Additive, no consensus)
}
```

#### Index Changes

```rust
if old.indexed != new.indexed {
    if new.indexed {
        // → AddIndex (Additive)
    } else {
        // → RemoveIndex (Breaking, requires consensus)
    }
}
```

### 4. Composite Index Detection

Compares `IndexSpec` arrays between specs:

```rust
// Added indexes
for new_index in &new_spec.indexes {
    if !old_spec.indexes.iter().any(|idx| idx.name == new_index.name) {
        // → AddIndex
    }
}

// Removed indexes
for old_index in &old_spec.indexes {
    if !new_spec.indexes.iter().any(|idx| idx.name == old_index.name) {
        // → RemoveIndex
    }
}
```

### 5. Foreign Key Detection

```rust
for new_fk in &new_spec.foreign_keys {
    if !old_spec.foreign_keys.iter().any(|fk| fk.field == new_fk.field) {
        // → AddForeignKey (Breaking, requires consensus)
    }
}
```

## Migration Strategy Determination

### For Added Fields

```rust
fn determine_migration_strategy_for_add(constraints: &FieldConstraints) -> MigrationStrategy {
    if constraints.primary_key || constraints.unique {
        MigrationStrategy::Breaking      // Must validate uniqueness
    } else if constraints.nullable {
        MigrationStrategy::Additive      // Can be NULL, safe
    } else {
        MigrationStrategy::Versioned     // Needs default value
    }
}
```

### Consensus Requirements

```rust
fn requires_consensus_for_add(constraints: &FieldConstraints) -> bool {
    constraints.primary_key ||    // Affects identity
    constraints.unique ||         // Needs cluster-wide validation
    !constraints.nullable         // Needs default or data migration
}
```

## SQL Generation

### Add Field

```rust
fn generate_add_field_sql(field: &str, constraints: &FieldConstraints) -> String {
    format!(
        "ALTER TABLE ADD COLUMN {} {} {}",
        field,
        "TYPE",  // TODO: infer from Rust type
        if constraints.nullable { "NULL" } else { "NOT NULL" }
    )
}
```

### Remove Field

```rust
fn generate_remove_field_sql(field: &str) -> String {
    format!("ALTER TABLE DROP COLUMN {}", field)
}
```

### Index Operations

```rust
fn generate_index_sql(field: &str, create: bool) -> String {
    if create {
        format!("CREATE INDEX idx_{} ON table ({})", field, field)
    } else {
        format!("DROP INDEX idx_{}", field)
    }
}
```

### Composite Index

```rust
format!(
    "CREATE {} INDEX {} ON {} ({})",
    if unique { "UNIQUE" } else { "" },
    index_name,
    table_name,
    fields.join(", ")
)
```

### Foreign Key

```rust
format!(
    "ALTER TABLE {} ADD CONSTRAINT fk_{}_{} FOREIGN KEY ({}) REFERENCES {} ({})",
    table, table.to_lowercase(), field, field, ref_table, ref_field
)
```

## Default Value Generation

When adding a non-nullable field, a default value is needed:

```rust
fn generate_default_value(constraints: &FieldConstraints) -> Option<String> {
    if constraints.nullable {
        Some("NULL")
    } else {
        Some("DEFAULT")  // TODO: type-specific defaults
    }
}
```

## Example: Complete Change Detection

Given these models:

**v1:**
```rust
pub struct Product {
    pub id: Uuid,
    pub name: String,
    pub price: f64,
}
```

**v2:**
```rust
pub struct Product {
    pub id: Uuid,
    pub name: String,
    pub price: f64,
    pub description: Option<String>,  // Added (nullable)
    pub sku: String,                   // Added (not null)
    // removed: legacy_code
}
```

**Detected Changes:**

| # | Type | Field | Strategy | Consensus |
|---|------|-------|----------|-----------|
| 1 | AddField | description | Additive | No |
| 2 | AddField | sku | Versioned | Yes |
| 3 | RemoveField | legacy_code | Breaking | Yes |

## Source Code Reference

- `lithair-core/src/schema/mod.rs` - SchemaChangeDetector implementation
- `lithair-core/src/schema/mod.rs:118-411` - All detection methods
- `lithair-macros/src/declarative_simple.rs:989-1001` - DeclarativeSpecExtractor impl

## See Also

- [Migration Overview](./overview.md)
- [Execution Flow](./execution-flow.md)
