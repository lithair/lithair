use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Module pour les relations et foreign keys
pub mod relations;
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
        if constraints.primary_key || constraints.unique {
            MigrationStrategy::Breaking
        } else if constraints.nullable {
            MigrationStrategy::Additive
        } else {
            MigrationStrategy::Versioned
        }
    }

    fn requires_consensus_for_add(constraints: &FieldConstraints) -> bool {
        constraints.primary_key || constraints.unique || !constraints.nullable
    }

    fn generate_default_value(constraints: &FieldConstraints) -> Option<String> {
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
