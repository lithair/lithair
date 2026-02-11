//! Model generation helpers

/// Helper for generating model-related code
#[allow(dead_code)]
pub struct ModelGenerator;

#[allow(dead_code)]
impl ModelGenerator {
    /// Generate event types for a model
    ///
    /// Returns generated event type code as a string.
    /// This is used by the lithair-macros proc macro crate.
    pub fn generate_events(model_name: &str) -> String {
        format!("// Generated events for {}\n", model_name)
    }
}
