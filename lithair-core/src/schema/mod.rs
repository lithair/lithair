use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

// Module pour les relations et foreign keys
pub mod relations;

// Module pour la synchronisation de schéma en cluster
pub mod sync;
pub use sync::{
    AppliedSchemaChange, HumanApproval, PendingSchemaChange, SchemaApproval,
    SchemaChangeStatus, SchemaLockStatus, SchemaRejection, SchemaSyncMessage,
    SchemaSyncState, SchemaVotePolicy, VoteStrategy,
};
pub use relations::{
    CascadeStrategy, ModelRelationSpec, RelationRegistry, RelationSpec, RelationSpecExtractor,
    RelationType,
};
// Import avec alias pour éviter conflit avec l'ancien ForeignKeySpec
pub use relations::ForeignKeySpec as RelationForeignKeySpec;

/// Types de changements de schéma détectables
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SchemaChangeType {
    AddField,
    RemoveField,
    ModifyFieldType,
    ModifyFieldConstraints,
    AddIndex,
    RemoveIndex,
    ModifyRetentionPolicy,
    ModifyPermissions,
    AddForeignKey,
    RemoveForeignKey,
}

/// Stratégies de migration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MigrationStrategy {
    /// Changement additif - pas de consensus requis
    Additive,
    /// Changement breaking - consensus Raft requis
    Breaking,
    /// Support multi-version - conversion automatique
    Versioned,
}

/// Changement de schéma détecté
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Contraintes de champ extraites des attributs déclaratifs
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldConstraints {
    pub primary_key: bool,
    pub unique: bool,
    pub indexed: bool,
    pub foreign_key: Option<String>,
    pub nullable: bool,
    pub immutable: bool,
    pub audited: bool,
    pub versioned: u32,
    pub retention: usize,
    pub snapshot_only: bool,
    pub validation_rules: Vec<String>,
    pub permissions: FieldPermissions,
    /// Default value for migration (from #[db(default = X)])
    /// When present, adding this field is safe (non-breaking)
    #[serde(default)]
    pub default_value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldPermissions {
    pub read_permission: Option<String>,
    pub write_permission: Option<String>,
    pub owner_field: bool,
}

/// Spécification de modèle extraite des macros déclaratives
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSpec {
    pub model_name: String,
    pub version: u32,
    pub fields: HashMap<String, FieldConstraints>,
    pub indexes: Vec<IndexSpec>,
    pub foreign_keys: Vec<ForeignKeySpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexSpec {
    pub name: String,
    pub fields: Vec<String>,
    pub unique: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForeignKeySpec {
    pub field: String,
    pub references_table: String,
    pub references_field: String,
}

/// Trait pour extraire les spécifications depuis les macros déclaratives
pub trait DeclarativeSpecExtractor {
    /// Extraire la spécification complète du modèle
    fn extract_model_spec(&self) -> ModelSpec;

    /// Obtenir la version du schéma
    fn schema_version(&self) -> u32;

    /// Obtenir les contraintes pour un champ spécifique
    fn field_constraints(&self, field_name: &str) -> Option<FieldConstraints>;
}

/// Trait for static schema extraction (no instance required)
///
/// This is automatically implemented by the DeclarativeModel macro.
/// Used for schema migration detection at server startup.
pub trait HasSchemaSpec {
    /// Extract the model's schema specification (static method)
    fn schema_spec() -> ModelSpec;

    /// Get the model name for storage/lookup
    fn model_name() -> &'static str;
}

/// Détecteur de changements de schéma robuste
pub struct SchemaChangeDetector;

impl SchemaChangeDetector {
    /// Détecter les changements entre deux spécifications de modèle
    pub fn detect_changes(old_spec: &ModelSpec, new_spec: &ModelSpec) -> Vec<DetectedSchemaChange> {
        let mut changes = Vec::new();

        // Détecter les champs ajoutés
        for (field_name, new_constraints) in &new_spec.fields {
            if !old_spec.fields.contains_key(field_name) {
                changes.push(DetectedSchemaChange {
                    model: new_spec.model_name.clone(),
                    change_type: SchemaChangeType::AddField,
                    field_name: Some(field_name.clone()),
                    old_type: None,
                    new_type: Some("inferred".to_string()), // TODO: type inference
                    old_constraints: None,
                    new_constraints: Some(new_constraints.clone()),
                    migration_strategy: Self::determine_migration_strategy_for_add(new_constraints),
                    default_value: Self::generate_default_value(new_constraints),
                    requires_consensus: Self::requires_consensus_for_add(new_constraints),
                    migration_sql: Self::generate_add_field_sql(field_name, new_constraints),
                    rollback_sql: Self::generate_remove_field_sql(field_name),
                });
            }
        }

        // Détecter les champs supprimés
        for (field_name, old_constraints) in &old_spec.fields {
            if !new_spec.fields.contains_key(field_name) {
                changes.push(DetectedSchemaChange {
                    model: new_spec.model_name.clone(),
                    change_type: SchemaChangeType::RemoveField,
                    field_name: Some(field_name.clone()),
                    old_type: Some("inferred".to_string()),
                    new_type: None,
                    old_constraints: Some(old_constraints.clone()),
                    new_constraints: None,
                    migration_strategy: MigrationStrategy::Breaking,
                    default_value: None,
                    requires_consensus: true,
                    migration_sql: Self::generate_remove_field_sql(field_name),
                    rollback_sql: Self::generate_add_field_sql(field_name, old_constraints),
                });
            }
        }

        // Détecter les modifications de contraintes
        for (field_name, new_constraints) in &new_spec.fields {
            if let Some(old_constraints) = old_spec.fields.get(field_name) {
                if old_constraints != new_constraints {
                    changes.extend(Self::detect_constraint_changes(
                        &new_spec.model_name,
                        field_name,
                        old_constraints,
                        new_constraints,
                    ));
                }
            }
        }

        // Détecter les changements d'index
        changes.extend(Self::detect_index_changes(old_spec, new_spec));

        // Détecter les changements de clés étrangères
        changes.extend(Self::detect_foreign_key_changes(old_spec, new_spec));

        changes
    }

    /// Détecter les changements de contraintes sur un champ
    fn detect_constraint_changes(
        model_name: &str,
        field_name: &str,
        old_constraints: &FieldConstraints,
        new_constraints: &FieldConstraints,
    ) -> Vec<DetectedSchemaChange> {
        let mut changes = Vec::new();

        // Changements de politique de rétention
        if old_constraints.retention != new_constraints.retention {
            changes.push(DetectedSchemaChange {
                model: model_name.to_string(),
                change_type: SchemaChangeType::ModifyRetentionPolicy,
                field_name: Some(field_name.to_string()),
                old_type: Some(old_constraints.retention.to_string()),
                new_type: Some(new_constraints.retention.to_string()),
                old_constraints: Some(old_constraints.clone()),
                new_constraints: Some(new_constraints.clone()),
                migration_strategy: MigrationStrategy::Additive,
                default_value: None,
                requires_consensus: false,
                migration_sql: None,
                rollback_sql: None,
            });
        }

        // Changements de permissions
        if old_constraints.permissions != new_constraints.permissions {
            changes.push(DetectedSchemaChange {
                model: model_name.to_string(),
                change_type: SchemaChangeType::ModifyPermissions,
                field_name: Some(field_name.to_string()),
                old_type: None,
                new_type: None,
                old_constraints: Some(old_constraints.clone()),
                new_constraints: Some(new_constraints.clone()),
                migration_strategy: MigrationStrategy::Additive,
                default_value: None,
                requires_consensus: false,
                migration_sql: None,
                rollback_sql: None,
            });
        }

        // Changements d'index
        if old_constraints.indexed != new_constraints.indexed {
            let change_type = if new_constraints.indexed {
                SchemaChangeType::AddIndex
            } else {
                SchemaChangeType::RemoveIndex
            };

            changes.push(DetectedSchemaChange {
                model: model_name.to_string(),
                change_type,
                field_name: Some(field_name.to_string()),
                old_type: None,
                new_type: None,
                old_constraints: Some(old_constraints.clone()),
                new_constraints: Some(new_constraints.clone()),
                migration_strategy: if new_constraints.indexed {
                    MigrationStrategy::Additive
                } else {
                    MigrationStrategy::Breaking
                },
                default_value: None,
                requires_consensus: !new_constraints.indexed,
                migration_sql: Some(Self::generate_index_sql(field_name, new_constraints.indexed)),
                rollback_sql: Some(Self::generate_index_sql(field_name, old_constraints.indexed)),
            });
        }

        changes
    }

    /// Détecter les changements d'index
    fn detect_index_changes(
        old_spec: &ModelSpec,
        new_spec: &ModelSpec,
    ) -> Vec<DetectedSchemaChange> {
        let mut changes = Vec::new();

        // Index ajoutés
        for new_index in &new_spec.indexes {
            if !old_spec.indexes.iter().any(|idx| idx.name == new_index.name) {
                changes.push(DetectedSchemaChange {
                    model: new_spec.model_name.clone(),
                    change_type: SchemaChangeType::AddIndex,
                    field_name: Some(new_index.fields.join(", ")),
                    old_type: None,
                    new_type: Some(format!("INDEX({})", new_index.fields.join(", "))),
                    old_constraints: None,
                    new_constraints: None,
                    migration_strategy: MigrationStrategy::Additive,
                    default_value: None,
                    requires_consensus: false,
                    migration_sql: Some(format!(
                        "CREATE {} INDEX {} ON {} ({})",
                        if new_index.unique { "UNIQUE" } else { "" },
                        new_index.name,
                        new_spec.model_name,
                        new_index.fields.join(", ")
                    )),
                    rollback_sql: Some(format!("DROP INDEX {}", new_index.name)),
                });
            }
        }

        // Index supprimés
        for old_index in &old_spec.indexes {
            if !new_spec.indexes.iter().any(|idx| idx.name == old_index.name) {
                changes.push(DetectedSchemaChange {
                    model: new_spec.model_name.clone(),
                    change_type: SchemaChangeType::RemoveIndex,
                    field_name: Some(old_index.fields.join(", ")),
                    old_type: Some(format!("INDEX({})", old_index.fields.join(", "))),
                    new_type: None,
                    old_constraints: None,
                    new_constraints: None,
                    migration_strategy: MigrationStrategy::Breaking,
                    default_value: None,
                    requires_consensus: true,
                    migration_sql: Some(format!("DROP INDEX {}", old_index.name)),
                    rollback_sql: Some(format!(
                        "CREATE {} INDEX {} ON {} ({})",
                        if old_index.unique { "UNIQUE" } else { "" },
                        old_index.name,
                        new_spec.model_name,
                        old_index.fields.join(", ")
                    )),
                });
            }
        }

        changes
    }

    /// Détecter les changements de clés étrangères
    fn detect_foreign_key_changes(
        old_spec: &ModelSpec,
        new_spec: &ModelSpec,
    ) -> Vec<DetectedSchemaChange> {
        let mut changes = Vec::new();

        // Clés étrangères ajoutées
        for new_fk in &new_spec.foreign_keys {
            if !old_spec.foreign_keys.iter().any(|fk| fk.field == new_fk.field) {
                changes.push(DetectedSchemaChange {
                    model: new_spec.model_name.clone(),
                    change_type: SchemaChangeType::AddForeignKey,
                    field_name: Some(new_fk.field.clone()),
                    old_type: None,
                    new_type: Some(format!("FK -> {}.{}", new_fk.references_table, new_fk.references_field)),
                    old_constraints: None,
                    new_constraints: None,
                    migration_strategy: MigrationStrategy::Breaking,
                    default_value: None,
                    requires_consensus: true,
                    migration_sql: Some(format!(
                        "ALTER TABLE {} ADD CONSTRAINT fk_{}_{} FOREIGN KEY ({}) REFERENCES {} ({})",
                        new_spec.model_name,
                        new_spec.model_name.to_lowercase(),
                        new_fk.field,
                        new_fk.field,
                        new_fk.references_table,
                        new_fk.references_field
                    )),
                    rollback_sql: Some(format!(
                        "ALTER TABLE {} DROP CONSTRAINT fk_{}_{}",
                        new_spec.model_name,
                        new_spec.model_name.to_lowercase(),
                        new_fk.field
                    )),
                });
            }
        }

        changes
    }

    // Fonctions utilitaires
    fn determine_migration_strategy_for_add(constraints: &FieldConstraints) -> MigrationStrategy {
        // If a default value is specified, the migration is safe (Additive)
        // because serde will use the default for old events missing this field
        if constraints.default_value.is_some() {
            MigrationStrategy::Additive
        } else if constraints.primary_key || constraints.unique {
            MigrationStrategy::Breaking
        } else if constraints.nullable {
            MigrationStrategy::Additive
        } else {
            // Non-nullable without default = Breaking change
            MigrationStrategy::Versioned
        }
    }

    fn requires_consensus_for_add(constraints: &FieldConstraints) -> bool {
        // If default_value is present, no consensus needed (safe migration)
        if constraints.default_value.is_some() {
            return false;
        }
        constraints.primary_key || constraints.unique || !constraints.nullable
    }

    fn generate_default_value(constraints: &FieldConstraints) -> Option<String> {
        // Use the explicitly defined default if present
        if let Some(ref default) = constraints.default_value {
            return Some(default.clone());
        }
        if constraints.nullable {
            Some("NULL".to_string())
        } else {
            // Valeurs par défaut basées sur les contraintes
            Some("DEFAULT".to_string())
        }
    }

    fn generate_add_field_sql(field_name: &str, constraints: &FieldConstraints) -> Option<String> {
        Some(format!(
            "ALTER TABLE ADD COLUMN {} {} {}",
            field_name,
            "TYPE", // TODO: inférer le type
            if constraints.nullable { "NULL" } else { "NOT NULL" }
        ))
    }

    fn generate_remove_field_sql(field_name: &str) -> Option<String> {
        Some(format!("ALTER TABLE DROP COLUMN {}", field_name))
    }

    fn generate_index_sql(field_name: &str, create: bool) -> String {
        if create {
            format!("CREATE INDEX idx_{} ON table ({})", field_name, field_name)
        } else {
            format!("DROP INDEX idx_{}", field_name)
        }
    }
}

// Note: DeclarativeSpecExtractor est maintenant implémenté automatiquement
// par la macro DeclarativeModel pour éviter les conflits d'implémentation

// ============================================================================
// Schema Persistence
// ============================================================================

/// Directory name for storing schema specifications
const SCHEMA_DIR: &str = ".schema";

/// Save a ModelSpec to disk as JSON
///
/// The spec is saved to `{base_path}/.schema/{model_name}.json`
///
/// # Example
/// ```rust,ignore
/// let spec = Product::extract_schema_spec();
/// save_schema_spec(&spec, Path::new("./data"))?;
/// ```
pub fn save_schema_spec(spec: &ModelSpec, base_path: &Path) -> std::io::Result<()> {
    let schema_dir = base_path.join(SCHEMA_DIR);
    std::fs::create_dir_all(&schema_dir)?;

    let file_path = schema_dir.join(format!("{}.json", spec.model_name));
    let json = serde_json::to_string_pretty(spec)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    std::fs::write(&file_path, json)?;
    log::debug!("Schema spec saved: {}", file_path.display());
    Ok(())
}

/// Load a ModelSpec from disk
///
/// Looks for `{base_path}/.schema/{model_name}.json`
///
/// Returns `Ok(None)` if no stored spec exists (first run)
///
/// # Example
/// ```rust,ignore
/// if let Some(stored) = load_schema_spec("Product", Path::new("./data"))? {
///     let current = Product::extract_schema_spec();
///     let changes = SchemaChangeDetector::detect_changes(&stored, &current);
/// }
/// ```
pub fn load_schema_spec(model_name: &str, base_path: &Path) -> std::io::Result<Option<ModelSpec>> {
    let file_path = base_path.join(SCHEMA_DIR).join(format!("{}.json", model_name));

    if !file_path.exists() {
        log::debug!("No stored schema for {}, first run", model_name);
        return Ok(None);
    }

    let json = std::fs::read_to_string(&file_path)?;
    let spec: ModelSpec = serde_json::from_str(&json)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    log::debug!("Schema spec loaded: {} v{}", spec.model_name, spec.version);
    Ok(Some(spec))
}

/// List all stored schema specs in the given directory
pub fn list_stored_schemas(base_path: &Path) -> std::io::Result<Vec<String>> {
    let schema_dir = base_path.join(SCHEMA_DIR);

    if !schema_dir.exists() {
        return Ok(Vec::new());
    }

    let mut schemas = Vec::new();
    for entry in std::fs::read_dir(&schema_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map(|e| e == "json").unwrap_or(false) {
            if let Some(stem) = path.file_stem() {
                schemas.push(stem.to_string_lossy().to_string());
            }
        }
    }

    Ok(schemas)
}

/// Delete a stored schema spec
pub fn delete_schema_spec(model_name: &str, base_path: &Path) -> std::io::Result<()> {
    let file_path = base_path.join(SCHEMA_DIR).join(format!("{}.json", model_name));

    if file_path.exists() {
        std::fs::remove_file(&file_path)?;
        log::debug!("Schema spec deleted: {}", model_name);
    }

    Ok(())
}

// =============================================================================
// SCHEMA HISTORY PERSISTENCE
// =============================================================================

const HISTORY_FILE: &str = "schema_history.json";
const LOCK_FILE: &str = "schema_lock.json";

/// Schema history data structure for persistence
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SchemaHistoryData {
    /// Applied changes history
    pub changes: Vec<AppliedSchemaChange>,
    /// Lock status
    pub lock_status: SchemaLockStatus,
}

/// Save schema history to disk
///
/// Saves to `{base_path}/.schema/schema_history.json`
pub fn save_schema_history(history: &SchemaHistoryData, base_path: &Path) -> std::io::Result<()> {
    let schema_dir = base_path.join(SCHEMA_DIR);
    std::fs::create_dir_all(&schema_dir)?;

    let file_path = schema_dir.join(HISTORY_FILE);
    let json = serde_json::to_string_pretty(history)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    std::fs::write(&file_path, json)?;
    log::debug!("Schema history saved: {}", file_path.display());
    Ok(())
}

/// Load schema history from disk
///
/// Returns default (empty) history if file doesn't exist
pub fn load_schema_history(base_path: &Path) -> std::io::Result<SchemaHistoryData> {
    let file_path = base_path.join(SCHEMA_DIR).join(HISTORY_FILE);

    if !file_path.exists() {
        log::debug!("No schema history file found, starting fresh");
        return Ok(SchemaHistoryData::default());
    }

    let json = std::fs::read_to_string(&file_path)?;
    let history: SchemaHistoryData = serde_json::from_str(&json)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    log::debug!("Schema history loaded: {} changes", history.changes.len());
    Ok(history)
}

/// Append a single change to history (atomic operation)
///
/// Loads existing history, appends change, saves back
pub fn append_schema_history(change: &AppliedSchemaChange, base_path: &Path) -> std::io::Result<()> {
    let mut history = load_schema_history(base_path)?;
    history.changes.push(change.clone());
    save_schema_history(&history, base_path)
}

/// Save lock status to disk (separate file for quick access)
pub fn save_lock_status(lock: &SchemaLockStatus, base_path: &Path) -> std::io::Result<()> {
    let schema_dir = base_path.join(SCHEMA_DIR);
    std::fs::create_dir_all(&schema_dir)?;

    let file_path = schema_dir.join(LOCK_FILE);
    let json = serde_json::to_string_pretty(lock)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    std::fs::write(&file_path, json)?;
    log::debug!("Schema lock status saved");
    Ok(())
}

/// Load lock status from disk
pub fn load_lock_status(base_path: &Path) -> std::io::Result<SchemaLockStatus> {
    let file_path = base_path.join(SCHEMA_DIR).join(LOCK_FILE);

    if !file_path.exists() {
        return Ok(SchemaLockStatus::default());
    }

    let json = std::fs::read_to_string(&file_path)?;
    let lock: SchemaLockStatus = serde_json::from_str(&json)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    Ok(lock)
}

#[cfg(test)]
mod persistence_tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_spec() -> ModelSpec {
        let mut fields = HashMap::new();
        fields.insert(
            "id".to_string(),
            FieldConstraints {
                primary_key: true,
                unique: true,
                indexed: true,
                foreign_key: None,
                nullable: false,
                immutable: true,
                audited: false,
                versioned: 0,
                retention: 0,
                snapshot_only: false,
                validation_rules: vec![],
                permissions: FieldPermissions {
                    read_permission: Some("Public".to_string()),
                    write_permission: None,
                    owner_field: false,
                },
                default_value: None,
            },
        );
        fields.insert(
            "name".to_string(),
            FieldConstraints {
                primary_key: false,
                unique: false,
                indexed: true,
                foreign_key: None,
                nullable: false,
                immutable: false,
                audited: false,
                versioned: 0,
                retention: 0,
                snapshot_only: false,
                validation_rules: vec!["min_length:1".to_string()],
                permissions: FieldPermissions {
                    read_permission: Some("Public".to_string()),
                    write_permission: Some("Admin".to_string()),
                    owner_field: false,
                },
                default_value: None,
            },
        );

        ModelSpec {
            model_name: "Product".to_string(),
            version: 1,
            fields,
            indexes: vec![IndexSpec {
                name: "idx_name".to_string(),
                fields: vec!["name".to_string()],
                unique: false,
            }],
            foreign_keys: vec![],
        }
    }

    #[test]
    fn test_save_and_load_schema_spec() {
        let temp_dir = TempDir::new().unwrap();
        let spec = sample_spec();

        // Save
        save_schema_spec(&spec, temp_dir.path()).unwrap();

        // Verify file exists
        let file_path = temp_dir.path().join(".schema/Product.json");
        assert!(file_path.exists());

        // Load
        let loaded = load_schema_spec("Product", temp_dir.path()).unwrap();
        assert!(loaded.is_some());

        let loaded = loaded.unwrap();
        assert_eq!(loaded.model_name, "Product");
        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.fields.len(), 2);
        assert!(loaded.fields.contains_key("id"));
        assert!(loaded.fields.contains_key("name"));
    }

    #[test]
    fn test_load_nonexistent_returns_none() {
        let temp_dir = TempDir::new().unwrap();

        let result = load_schema_spec("NonExistent", temp_dir.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_list_stored_schemas() {
        let temp_dir = TempDir::new().unwrap();

        // Initially empty
        let schemas = list_stored_schemas(temp_dir.path()).unwrap();
        assert!(schemas.is_empty());

        // Save two specs
        let mut spec1 = sample_spec();
        spec1.model_name = "Product".to_string();
        save_schema_spec(&spec1, temp_dir.path()).unwrap();

        let mut spec2 = sample_spec();
        spec2.model_name = "Category".to_string();
        save_schema_spec(&spec2, temp_dir.path()).unwrap();

        // List
        let schemas = list_stored_schemas(temp_dir.path()).unwrap();
        assert_eq!(schemas.len(), 2);
        assert!(schemas.contains(&"Product".to_string()));
        assert!(schemas.contains(&"Category".to_string()));
    }

    #[test]
    fn test_delete_schema_spec() {
        let temp_dir = TempDir::new().unwrap();
        let spec = sample_spec();

        // Save
        save_schema_spec(&spec, temp_dir.path()).unwrap();
        assert!(load_schema_spec("Product", temp_dir.path()).unwrap().is_some());

        // Delete
        delete_schema_spec("Product", temp_dir.path()).unwrap();
        assert!(load_schema_spec("Product", temp_dir.path()).unwrap().is_none());
    }

    #[test]
    fn test_schema_roundtrip_preserves_data() {
        let temp_dir = TempDir::new().unwrap();
        let original = sample_spec();

        save_schema_spec(&original, temp_dir.path()).unwrap();
        let loaded = load_schema_spec("Product", temp_dir.path()).unwrap().unwrap();

        // Compare all fields
        assert_eq!(original.model_name, loaded.model_name);
        assert_eq!(original.version, loaded.version);
        assert_eq!(original.indexes.len(), loaded.indexes.len());
        assert_eq!(original.foreign_keys.len(), loaded.foreign_keys.len());

        // Compare field constraints
        for (name, constraints) in &original.fields {
            let loaded_constraints = loaded.fields.get(name).unwrap();
            assert_eq!(constraints, loaded_constraints);
        }
    }
}
