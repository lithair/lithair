//! Relation specifications and foreign key support for Lithair
//!
//! This module provides the schema definitions and metadata structures
//! needed to support declarative relations between models.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Types de relations supportées
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelationType {
    /// Relation One-to-One (foreign key unique)
    OneToOne,
    /// Relation Many-to-One (foreign key standard)
    ManyToOne,
    /// Relation One-to-Many (reverse de ManyToOne)
    OneToMany,
    /// Relation Many-to-Many (via table de liaison)
    ManyToMany,
}

/// Stratégies de suppression en cascade
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CascadeStrategy {
    /// Ne rien faire (laisser l'ID orphelin)
    None,
    /// Supprimer en cascade
    Delete,
    /// Mettre à NULL/None
    SetNull,
    /// Interdire la suppression si des références existent
    Restrict,
}

/// Spécification d'une foreign key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForeignKeySpec {
    /// Nom du champ contenant la foreign key
    pub field_name: String,
    /// Type du modèle référencé (nom du struct)
    pub referenced_model: String,
    /// Champ référencé dans le modèle cible (généralement "id")
    pub referenced_field: String,
    /// Type de relation
    pub relation_type: RelationType,
    /// Stratégie de cascade
    pub cascade: CascadeStrategy,
    /// Si la foreign key est nullable
    pub nullable: bool,
    /// Index automatique sur la foreign key
    pub indexed: bool,
}

/// Spécification d'une relation (inverse d'une foreign key)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationSpec {
    /// Nom de la relation (nom de la méthode générée)
    pub relation_name: String,
    /// Type du modèle qui contient la foreign key
    pub source_model: String,
    /// Champ foreign key dans le modèle source
    pub source_field: String,
    /// Type de relation
    pub relation_type: RelationType,
    /// Si lazy loading ou eager loading par défaut
    pub lazy: bool,
}

/// Métadonnées complètes des relations d'un modèle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRelationSpec {
    /// Nom du modèle
    pub model_name: String,
    /// Foreign keys déclarées sur ce modèle
    pub foreign_keys: HashMap<String, ForeignKeySpec>,
    /// Relations inverses disponibles sur ce modèle
    pub relations: HashMap<String, RelationSpec>,
}

/// Trait pour extraire les spécifications de relations d'un modèle
pub trait RelationSpecExtractor {
    /// Extraire les métadonnées de relations du modèle
    fn relation_spec() -> ModelRelationSpec;

    /// Vérifier si un champ est une foreign key
    fn is_foreign_key(field_name: &str) -> bool {
        Self::relation_spec().foreign_keys.contains_key(field_name)
    }

    /// Obtenir la spec d'une foreign key
    fn get_foreign_key_spec(field_name: &str) -> Option<ForeignKeySpec> {
        Self::relation_spec().foreign_keys.get(field_name).cloned()
    }

    /// Obtenir toutes les relations disponibles
    fn get_relations() -> HashMap<String, RelationSpec> {
        Self::relation_spec().relations
    }
}

/// Gestionnaire global des relations entre modèles
#[derive(Debug, Clone, Default)]
pub struct RelationRegistry {
    /// Mapping modèle -> spécifications de relations
    models: HashMap<String, ModelRelationSpec>,
}

impl RelationRegistry {
    /// Créer un nouveau registre vide
    pub fn new() -> Self {
        Self { models: HashMap::new() }
    }

    /// Enregistrer les relations d'un modèle
    pub fn register_model(&mut self, spec: ModelRelationSpec) {
        self.models.insert(spec.model_name.clone(), spec);
    }

    /// Obtenir les relations d'un modèle
    pub fn get_model_relations(&self, model_name: &str) -> Option<&ModelRelationSpec> {
        self.models.get(model_name)
    }

    /// Vérifier l'intégrité des relations (toutes les FK pointent vers des modèles existants)
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

    /// Construire le graphe de dépendances entre modèles
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
