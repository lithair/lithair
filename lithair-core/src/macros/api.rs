//! API generation helpers

/// Helper for generating API-related code
#[allow(dead_code)]
pub struct ApiGenerator;

#[allow(dead_code)]
impl ApiGenerator {
    /// Generate HTTP routes for an API impl
    ///
    /// Returns generated route registration code as a string.
    /// This is used by the lithair-macros proc macro crate.
    pub fn generate_routes(impl_name: &str) -> String {
        format!("// Generated routes for {}\n", impl_name)
    }
}
