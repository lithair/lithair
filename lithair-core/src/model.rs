use serde::{Deserialize, Serialize};

/// Field management policy definition (column)
///
/// This structure allows declaratively defining the behavior
/// of the Lithair engine for each entity attribute.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FieldPolicy {
    /// Number of historical events to retain for this field
    /// 0 = No history (only the current snapshotted state)
    /// N = Keep the last N modifications
    pub retention_limit: usize,

    /// If true, the engine will enforce value uniqueness across all aggregates
    /// (Requires an expensive global index, use sparingly)
    pub unique: bool,

    /// If true, this field will be indexed for fast lookups
    pub indexed: bool,

    /// If true, this field is only stored in the snapshot (current state)
    /// and does not generate individually persisted events (optimization)
    pub snapshot_only: bool,

    /// If true, this field is a foreign key (references another aggregate)
    /// The engine can verify referential integrity
    pub fk: bool,

    /// Name of the collection/table targeted by the foreign key
    /// e.g. "products", "users", "categories"
    pub fk_collection: Option<String>,
}

/// Trait that model specifications must implement
pub trait ModelSpec: Send + Sync {
    /// Return the policy for a given field (by name)
    fn get_policy(&self, field_name: &str) -> Option<FieldPolicy>;

    /// Return the list of all managed fields
    /// Useful for iteration during automatic indexing
    fn get_all_fields(&self) -> Vec<String> {
        Vec::new() // Default to empty to maintain backward compat where possible
    }
}
