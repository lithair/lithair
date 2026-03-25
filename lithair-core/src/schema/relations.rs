//! Relation specifications and foreign key support for Lithair
//!
//! This module provides the schema definitions and metadata structures
//! needed to support declarative relations between models.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Supported relation types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelationType {
    /// Relation One-to-One (foreign key unique)
    OneToOne,
    /// Relation Many-to-One (foreign key standard)
    ManyToOne,
    /// Relation One-to-Many (reverse of ManyToOne)
    OneToMany,
    /// Relation Many-to-Many (via junction table)
    ManyToMany,
}

/// Cascade deletion strategies
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CascadeStrategy {
    /// Do nothing (leave the ID orphaned)
    None,
    /// Cascade delete
    Delete,
    /// Set to NULL/None
    SetNull,
    /// Forbid deletion if references exist
    Restrict,
}

/// Foreign key specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForeignKeySpec {
    /// Field name containing the foreign key
    pub field_name: String,
    /// Referenced model type (struct name)
    pub referenced_model: String,
    /// Referenced field in the target model (typically "id")
    pub referenced_field: String,
    /// Relation type
    pub relation_type: RelationType,
    /// Cascade strategy
    pub cascade: CascadeStrategy,
    /// Whether the foreign key is nullable
    pub nullable: bool,
    /// Automatic index on the foreign key
    pub indexed: bool,
}

/// Relation specification (inverse of a foreign key)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationSpec {
    /// Relation name (name of the generated method)
    pub relation_name: String,
    /// Model type that contains the foreign key
    pub source_model: String,
    /// Foreign key field in the source model
    pub source_field: String,
    /// Relation type
    pub relation_type: RelationType,
    /// Whether to use lazy loading or eager loading by default
    pub lazy: bool,
}

/// Complete relation metadata for a model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRelationSpec {
    /// Model name
    pub model_name: String,
    /// Foreign keys declared on this model
    pub foreign_keys: HashMap<String, ForeignKeySpec>,
    /// Inverse relations available on this model
    pub relations: HashMap<String, RelationSpec>,
}

/// Trait to extract relation specifications from a model
pub trait RelationSpecExtractor {
    /// Extract the model's relation metadata
    fn relation_spec() -> ModelRelationSpec;

    /// Check if a field is a foreign key
    fn is_foreign_key(field_name: &str) -> bool {
        Self::relation_spec().foreign_keys.contains_key(field_name)
    }

    /// Get the foreign key spec
    fn get_foreign_key_spec(field_name: &str) -> Option<ForeignKeySpec> {
        Self::relation_spec().foreign_keys.get(field_name).cloned()
    }

    /// Get all available relations
    fn get_relations() -> HashMap<String, RelationSpec> {
        Self::relation_spec().relations
    }
}

/// Global registry of relations between models
#[derive(Debug, Clone, Default)]
pub struct RelationRegistry {
    /// Mapping model -> relation specifications
    models: HashMap<String, ModelRelationSpec>,
}

impl RelationRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self { models: HashMap::new() }
    }

    /// Register a model's relations
    pub fn register_model(&mut self, spec: ModelRelationSpec) {
        self.models.insert(spec.model_name.clone(), spec);
    }

    /// Get a model's relations
    pub fn get_model_relations(&self, model_name: &str) -> Option<&ModelRelationSpec> {
        self.models.get(model_name)
    }

    /// Validate relation integrity (all FKs point to existing models)
    pub fn validate_integrity(&self) -> Result<(), String> {
        for (model_name, spec) in &self.models {
            for (field_name, fk_spec) in &spec.foreign_keys {
                if !self.models.contains_key(&fk_spec.referenced_model) {
                    return Err(format!(
                        "Foreign key {}.{} references unknown model {}",
                        model_name, field_name, fk_spec.referenced_model
                    ));
                }
            }
        }
        Ok(())
    }

    /// Build the dependency graph between models
    pub fn dependency_graph(&self) -> HashMap<String, Vec<String>> {
        let mut graph = HashMap::new();

        for (model_name, spec) in &self.models {
            let mut dependencies = Vec::new();
            for fk_spec in spec.foreign_keys.values() {
                dependencies.push(fk_spec.referenced_model.clone());
            }
            graph.insert(model_name.clone(), dependencies);
        }

        graph
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_foreign_key_spec_creation() {
        let fk_spec = ForeignKeySpec {
            field_name: "user_id".to_string(),
            referenced_model: "User".to_string(),
            referenced_field: "id".to_string(),
            relation_type: RelationType::ManyToOne,
            cascade: CascadeStrategy::SetNull,
            nullable: true,
            indexed: true,
        };

        assert_eq!(fk_spec.field_name, "user_id");
        assert_eq!(fk_spec.referenced_model, "User");
        assert!(fk_spec.nullable);
    }

    #[test]
    fn test_relation_registry() {
        let mut registry = RelationRegistry::new();

        let user_spec = ModelRelationSpec {
            model_name: "User".to_string(),
            foreign_keys: HashMap::new(),
            relations: HashMap::new(),
        };

        registry.register_model(user_spec);
        assert!(registry.get_model_relations("User").is_some());
        assert!(registry.get_model_relations("NonExistent").is_none());
    }
}
