use serde_json::Value;

/// Trait for inspecting internal fields of a struct without full serialization
/// This is critical for high-performance indexing and uniqueness checks
pub trait Inspectable {
    /// Get the value of a specific field by name
    /// Returns None if field doesn't exist
    /// Returns Some(Value) if field exists
    ///
    /// Optimizations:
    /// - Should avoid cloning large strings if possible (future optimization with Cow)
    /// - Currently returns serde_json::Value for compatibility with Index storage
    fn get_field_value(&self, field_name: &str) -> Option<Value>;

    /// Get values for multiple fields at once (batch optimization)
    fn get_field_values(&self, field_names: &[&str]) -> Vec<(String, Option<Value>)> {
        field_names
            .iter()
            .map(|&name| (name.to_string(), self.get_field_value(name)))
            .collect()
    }
}
