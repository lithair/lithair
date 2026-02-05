use serde::{Deserialize, Serialize};

/// Définition de la politique de gestion d'un champ (colonne)
///
/// Cette structure permet de définir déclarativement le comportement
/// du moteur Lithair pour chaque attribut de vos entités.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FieldPolicy {
    /// Nombre d'événements historiques à conserver pour ce champ
    /// 0 = Pas d'historique (juste l'état actuel snapshoté)
    /// N = Garder les N dernières modifications
    pub retention_limit: usize,

    /// Si vrai, le moteur garantira l'unicité de la valeur sur l'ensemble des agrégats
    /// (Nécessite un index global coûteux, à utiliser avec parcimonie)
    pub unique: bool,

    /// Si vrai, ce champ sera indexé pour des recherches rapides (Lookups)
    pub indexed: bool,

    /// Si vrai, ce champ n'est stocké que dans le snapshot (état courant)
    /// et ne génère pas d'événements persistés individuellement (optimisation)
    pub snapshot_only: bool,

    /// Si vrai, ce champ est une clé étrangère (référence un autre agrégat)
    /// Le moteur pourra vérifier l'intégrité référentielle
    pub fk: bool,

    /// Nom de la collection/table ciblée par la clé étrangère
    /// Ex: "products", "users", "categories"
    pub fk_collection: Option<String>,
}


/// Trait que doivent implémenter les spécifications de modèle
pub trait ModelSpec: Send + Sync {
    /// Retourne la politique pour un champ donné (par nom)
    fn get_policy(&self, field_name: &str) -> Option<FieldPolicy>;

    /// Retourne la liste de tous les champs gérés
    /// Utile pour l'itération lors de l'indexation automatique
    fn get_all_fields(&self) -> Vec<String> {
        Vec::new() // Default to empty to maintain backward compat where possible
    }
}
