// Lithair Lifecycle Management - Core Engine Integration
// Migrated from product_app patterns to provide declarative data lifecycle as a framework feature

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Field-level lifecycle policy configuration - the cornerstone of Lithair's declarative model
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct FieldPolicy {
    /// Retention limit in days (0 = unlimited)
    pub retention_limit: u32,
    /// Field must be unique across all instances
    pub unique: bool,
    /// Field should be indexed for fast queries
    pub indexed: bool,
    /// Field is only stored in snapshots, not events (for performance)
    pub snapshot_only: bool,
    /// Field is a foreign key reference
    pub fk: bool,
    /// Field never changes after creation
    pub immutable: bool,
    /// Field changes are audited (full history preserved)
    pub audited: bool,
    /// Maximum number of versions to keep (0 = unlimited)
    pub version_limit: u32,
    /// Field is computed from other fields (not stored in events)
    pub computed: bool,
}

/// Lifecycle strategies for common patterns
impl FieldPolicy {
    /// Immutable field - never changes after creation
    pub fn immutable() -> Self {
        Self {
            retention_limit: 0, // no history needed
            unique: false,
            indexed: false,
            snapshot_only: true, // immutable fields only need snapshots
            fk: false,
            immutable: true,
            audited: false,
            version_limit: 0,
            computed: false,
        }
    }

    /// Audited field - full history preserved
    pub fn audited() -> Self {
        Self {
            retention_limit: u32::MAX, // unlimited retention for full history
            unique: false,
            indexed: true, // audited fields should be indexed for queries
            snapshot_only: false,
            fk: false,
            immutable: false,
            audited: true,
            version_limit: 0,
            computed: false,
        }
    }

    /// Versioned field - keep limited history
    pub fn versioned() -> Self {
        Self {
            retention_limit: 0,
            unique: false,
            indexed: false,
            snapshot_only: false,
            fk: false,
            immutable: false,
            audited: false,
            version_limit: 5,
            computed: false,
        }
    }

    /// Foreign key field
    pub fn foreign_key() -> Self {
        Self {
            retention_limit: 0,
            unique: false,
            indexed: true, // FKs should be indexed
            snapshot_only: false,
            fk: true,
            immutable: false,
            audited: false,
            version_limit: 0,
            computed: false,
        }
    }

    /// Unique field
    pub fn unique() -> Self {
        Self {
            retention_limit: 0,
            unique: true,
            indexed: true, // Unique fields should be indexed
            snapshot_only: false,
            fk: false,
            immutable: false,
            audited: false,
            version_limit: 0,
            computed: false,
        }
    }

    /// Snapshot-only field
    pub fn snapshot_only() -> Self {
        Self {
            retention_limit: 0,
            unique: false,
            indexed: false,
            snapshot_only: true,
            fk: false,
            immutable: false,
            audited: false,
            version_limit: 0,
            computed: false,
        }
    }

    /// Unique field with versioning
    pub fn unique_versioned(retention: u32) -> Self {
        Self {
            retention_limit: retention,
            unique: true,
            indexed: true,
            snapshot_only: false,
            fk: false,
            immutable: false,
            audited: false,
            version_limit: 0,
            computed: false,
        }
    }

    /// Computed field - no storage needed
    pub fn computed() -> Self {
        Self {
            retention_limit: 0,
            unique: false,
            indexed: false,
            snapshot_only: false,
            fk: false,
            immutable: false,
            audited: false,
            version_limit: 0,
            computed: true,
        }
    }
}

/// Trait for lifecycle-aware data models - the cornerstone of Lithair's declarative approach
pub trait LifecycleAware {
    /// Get the lifecycle policy for a specific field
    fn lifecycle_policy_for_field(&self, field_name: &str) -> Option<FieldPolicy>;

    /// Get all field names that have lifecycle policies
    fn all_field_names(&self) -> Vec<&'static str>;

    /// Get the model name for this lifecycle-aware type
    fn model_name(&self) -> &'static str;

    /// Check if a field is immutable (never changes after creation)
    fn is_field_immutable(&self, field_name: &str) -> bool {
        self.lifecycle_policy_for_field(field_name)
            .map(|policy| policy.immutable)
            .unwrap_or(false)
    }

    /// Check if a field should be audited (full history preserved)
    fn is_field_audited(&self, field_name: &str) -> bool {
        self.lifecycle_policy_for_field(field_name)
            .map(|policy| policy.audited)
            .unwrap_or(false)
    }

    /// Get version limit for a field (0 = unlimited)
    fn field_version_limit(&self, field_name: &str) -> u32 {
        self.lifecycle_policy_for_field(field_name)
            .map(|policy| policy.version_limit)
            .unwrap_or(0)
    }

    /// Check if field is computed (derived from other fields)
    fn is_field_computed(&self, field_name: &str) -> bool {
        self.lifecycle_policy_for_field(field_name)
            .map(|policy| policy.computed)
            .unwrap_or(false)
    }
}

/// Lifecycle engine for managing field-level retention and optimization
#[derive(Debug)]
pub struct LifecycleEngine {
    /// Field policies by entity type and field name
    policies: HashMap<String, HashMap<String, FieldPolicy>>,
}

impl LifecycleEngine {
    pub fn new() -> Self {
        Self { policies: HashMap::new() }
    }

    /// Register lifecycle policies for an entity type
    pub fn register_entity_policies<T: LifecycleAware>(&mut self, entity_type: &str, entity: &T) {
        let mut field_policies = HashMap::new();

        // Get all field names and their policies
        for field_name in entity.all_field_names() {
            if let Some(policy) = entity.lifecycle_policy_for_field(field_name) {
                field_policies.insert(field_name.to_string(), policy);
            }
        }

        self.policies.insert(entity_type.to_string(), field_policies);
    }

    /// Get retention limit for a specific entity type and field variant
    pub fn retention_for_variant(&self, entity_type: &str, variant: &str) -> u32 {
        self.policies
            .get(entity_type)
            .and_then(|entity_policies| entity_policies.get(variant))
            .map(|policy| policy.retention_limit)
            .unwrap_or(0) // default to unlimited if not found
    }

    /// Check if a field should be indexed
    pub fn should_index(&self, entity_type: &str, field: &str) -> bool {
        self.policies
            .get(entity_type)
            .and_then(|entity_policies| entity_policies.get(field))
            .map(|policy| policy.indexed)
            .unwrap_or(false)
    }

    /// Check if a field has uniqueness constraint
    pub fn is_unique(&self, entity_type: &str, field: &str) -> bool {
        self.policies
            .get(entity_type)
            .and_then(|entity_policies| entity_policies.get(field))
            .map(|policy| policy.unique)
            .unwrap_or(false)
    }

    /// Check if a field should only be stored in snapshots
    pub fn is_snapshot_only(&self, entity_type: &str, field: &str) -> bool {
        self.policies
            .get(entity_type)
            .and_then(|entity_policies| entity_policies.get(field))
            .map(|policy| policy.snapshot_only)
            .unwrap_or(false)
    }

    /// Check if a field is a foreign key
    pub fn is_foreign_key(&self, entity_type: &str, field: &str) -> bool {
        self.policies
            .get(entity_type)
            .and_then(|entity_policies| entity_policies.get(field))
            .map(|policy| policy.fk)
            .unwrap_or(false)
    }
}

impl Default for LifecycleEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_field_policy_presets() {
        let immutable = FieldPolicy::immutable();
        assert_eq!(immutable.retention_limit, 0);
        assert!(immutable.snapshot_only);

        let audited = FieldPolicy::audited();
        assert_eq!(audited.retention_limit, u32::MAX);
        assert!(audited.indexed);

        let versioned = FieldPolicy::unique_versioned(3);
        assert_eq!(versioned.retention_limit, 3);
        assert!(versioned.unique);
        assert!(versioned.indexed);
    }

    #[test]
    fn test_lifecycle_engine() {
        let mut engine = LifecycleEngine::new();

        // Simulate registering policies (would be done via macros in real usage)
        let mut product_policies = HashMap::new();
        product_policies.insert("name".to_string(), FieldPolicy::unique_versioned(3));
        product_policies.insert("price".to_string(), FieldPolicy::audited());
        product_policies.insert("created_at".to_string(), FieldPolicy::immutable());

        engine.policies.insert("Product".to_string(), product_policies);

        assert_eq!(engine.retention_for_variant("Product", "name"), 3);
        assert_eq!(engine.retention_for_variant("Product", "price"), u32::MAX);
        assert_eq!(engine.retention_for_variant("Product", "created_at"), 0);

        assert!(engine.is_unique("Product", "name"));
        assert!(!engine.is_unique("Product", "price"));

        assert!(engine.should_index("Product", "price"));
        assert!(engine.is_snapshot_only("Product", "created_at"));
    }
}
