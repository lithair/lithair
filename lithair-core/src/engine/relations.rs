use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::model::ModelSpec;

/// Trait for a data source that can be joined
/// Implement this for your Repositories or Engines
pub trait DataSource: Send + Sync {
    /// Fetch an item by ID and return it as a JSON Value
    fn fetch_by_id(&self, id: &str) -> Option<Value>;
}

/// Registry for all data sources in the application
/// Used to resolve "fk_collection" references
#[derive(Default, Clone)]
pub struct RelationRegistry {
    sources: Arc<RwLock<HashMap<String, Arc<dyn DataSource>>>>,
}

impl RelationRegistry {
    pub fn new() -> Self {
        Self { sources: Arc::new(RwLock::new(HashMap::new())) }
    }

    /// Register a data source for a specific collection name
    pub fn register(&self, collection_name: &str, source: Arc<dyn DataSource>) {
        self.sources.write().expect("relation sources lock poisoned").insert(collection_name.to_string(), source);
    }

    /// Get a data source by name
    pub fn get(&self, collection_name: &str) -> Option<Arc<dyn DataSource>> {
        self.sources.read().expect("relation sources lock poisoned").get(collection_name).cloned()
    }
}

/// The Auto-Joiner Logic
/// Takes an entity, inspects its ModelSpec, and expands foreign keys
pub struct AutoJoiner;

impl AutoJoiner {
    /// Expand relations for a single entity
    pub fn expand<T>(
        entity: &T,
        model_spec: &dyn ModelSpec,
        registry: &RelationRegistry,
    ) -> serde_json::Result<Value>
    where
        T: Serialize,
    {
        // 1. Serialize entity to JSON Value (Object)
        let mut json_val = serde_json::to_value(entity)?;

        if let Value::Object(ref mut map) = json_val {
            // 2. Iterate over fields in the JSON
            // Note: We need to collect keys first to avoid borrowing issues if we modify map
            let keys: Vec<String> = map.keys().cloned().collect();

            for key in keys {
                // 3. Check ModelSpec for this field
                if let Some(policy) = model_spec.get_policy(&key) {
                    // 4. If it's a Foreign Key with a Target Collection
                    if policy.fk {
                        if let Some(target_collection) = &policy.fk_collection {
                            // 5. Get the FK Value (ID)
                            if let Some(fk_value) = map.get(&key).and_then(|v| v.as_str()) {
                                // 6. Resolve Data Source
                                if let Some(source) = registry.get(target_collection) {
                                    // 7. Fetch Related Data
                                    if let Some(related_data) = source.fetch_by_id(fk_value) {
                                        // 8. Inject into JSON
                                        // Convention: field_id -> field (replace) OR field_expanded
                                        // Let's use "field_expanded" to be safe and keep original ID
                                        // Or maybe just "field_data"
                                        // Actually, convention usually is:
                                        // category_id -> 123
                                        // category -> { id: 123, name: "..." }

                                        // Infer target field name: remove "_id" suffix if present, else append "_data"
                                        let target_field_name = if key.ends_with("_id") {
                                            key[..key.len() - 3].to_string()
                                        } else {
                                            format!("{}_data", key)
                                        };

                                        map.insert(target_field_name, related_data);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(json_val)
    }

    /// Expand relations for a list of entities
    pub fn expand_list<T>(
        entities: &[T],
        model_spec: &dyn ModelSpec,
        registry: &RelationRegistry,
    ) -> serde_json::Result<Value>
    where
        T: Serialize,
    {
        let mut list = Vec::with_capacity(entities.len());
        for entity in entities {
            list.push(Self::expand(entity, model_spec, registry)?);
        }
        Ok(Value::Array(list))
    }
}

/* impl<S> DataSource for Scc2Engine<S> moved to scc2_engine.rs */

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::FieldPolicy;

    #[derive(Serialize, Clone)]
    struct TestUser {
        id: String,
        name: String,
        role_id: String,
    }

    struct MockDataSource {
        data: HashMap<String, Value>,
    }

    impl DataSource for MockDataSource {
        fn fetch_by_id(&self, id: &str) -> Option<Value> {
            self.data.get(id).cloned()
        }
    }

    struct TestModelSpec;
    impl ModelSpec for TestModelSpec {
        fn get_policy(&self, field_name: &str) -> Option<FieldPolicy> {
            if field_name == "role_id" {
                Some(FieldPolicy {
                    fk: true,
                    fk_collection: Some("roles".to_string()),
                    ..Default::default()
                })
            } else {
                None
            }
        }
    }

    #[test]
    fn test_auto_joiner_expand() {
        let registry = RelationRegistry::new();
        let mut role_data = HashMap::new();
        role_data.insert("r1".to_string(), serde_json::json!({ "id": "r1", "title": "Admin" }));
        registry.register("roles", Arc::new(MockDataSource { data: role_data }));

        let user =
            TestUser { id: "u1".to_string(), name: "Alice".to_string(), role_id: "r1".to_string() };

        let expanded = AutoJoiner::expand(&user, &TestModelSpec, &registry).unwrap();

        assert_eq!(expanded["id"], "u1");
        assert_eq!(expanded["name"], "Alice");
        assert_eq!(expanded["role_id"], "r1");

        // Check joined data
        assert!(expanded.get("role").is_some());
        assert_eq!(expanded["role"]["title"], "Admin");
    }
}
