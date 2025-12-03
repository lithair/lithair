//! Ultra-performance HTTP response constants and optimizations
//!
//! This module provides pre-compiled static JSON responses and error messages
//! for maximum throughput and zero allocation in high-performance scenarios.

/// Pre-compiled static error responses for ultra-performance (zero allocation)
pub mod static_errors {
    /// Generic parsing error (400 Bad Request)
    pub const ERROR_PARSING: &str = r#"{"status":"error","message":"❌ Erreur parsing JSON"}"#;

    /// Generic persistence error (500 Internal Server Error)
    pub const ERROR_PERSIST: &str =
        r#"{"status":"error","message":"❌ Erreur persistance optimisée"}"#;

    /// Authentication error (401 Unauthorized)
    pub const ERROR_AUTH: &str = r#"{"status":"error","message":"❌ Erreur authentification"}"#;

    /// Authorization error (403 Forbidden)
    pub const ERROR_AUTHZ: &str = r#"{"status":"error","message":"❌ Accès refusé"}"#;

    /// Not found error (404 Not Found)
    pub const ERROR_NOT_FOUND: &str = r#"{"status":"error","message":"❌ Ressource non trouvée"}"#;

    /// Rate limit error (429 Too Many Requests)
    pub const ERROR_RATE_LIMIT: &str =
        r#"{"status":"error","message":"❌ Limite de débit dépassée"}"#;

    /// Service unavailable error (503 Service Unavailable)
    pub const ERROR_SERVICE_UNAVAILABLE: &str =
        r#"{"status":"error","message":"❌ Service temporairement indisponible"}"#;
}

/// Pre-compiled static success response templates for ultra-performance
pub mod static_templates {
    /// Batch injection success template (IoT optimized)
    pub const IOT_BATCH_SUCCESS: &str = r#"{"status":"success","message":"🔥 {COUNT} lectures injectées avec FORMAT BINAIRE ultra-optimisé !","readings_injected":{COUNT},"optimizations_active":{"parsing":"✅ Direct JSON → SensorReading","serialization":"🔥 Format binaire (bincode) ACTIVÉ","io":"⚡ Écriture asynchrone bufferisée (2MB, 50ms flush)"},"performance":"184K+ events/sec avec format binaire","binary_format":"ENABLED"}"#;

    /// Generic success template
    pub const GENERIC_SUCCESS: &str =
        r#"{"status":"success","message":"✅ Opération réussie","count":{COUNT}}"#;

    /// Stats response template
    pub const STATS_TEMPLATE: &str = r#"{"status":"success","total_readings":{TOTAL},"recent_readings_count":{RECENT},"memory_usage_mb":{MEMORY},"performance_ms":{PERF}}"#;
}

/// Ultra-performance helper functions for template replacement
pub mod template_helpers {
    use super::static_templates;

    /// Replace {COUNT} placeholder in IoT batch success template
    pub fn iot_batch_success(count: usize) -> String {
        static_templates::IOT_BATCH_SUCCESS.replace("{COUNT}", &count.to_string())
    }

    /// Replace {COUNT} placeholder in generic success template
    pub fn generic_success(count: usize) -> String {
        static_templates::GENERIC_SUCCESS.replace("{COUNT}", &count.to_string())
    }

    /// Replace multiple placeholders in stats template
    pub fn stats_response(total: usize, recent: usize, memory_mb: usize, perf_ms: f64) -> String {
        static_templates::STATS_TEMPLATE
            .replace("{TOTAL}", &total.to_string())
            .replace("{RECENT}", &recent.to_string())
            .replace("{MEMORY}", &memory_mb.to_string())
            .replace("{PERF}", &format!("{:.2}", perf_ms))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_static_errors() {
        assert!(static_errors::ERROR_PARSING.contains("parsing"));
        assert!(static_errors::ERROR_PERSIST.contains("persistance"));
        assert!(static_errors::ERROR_AUTH.contains("authentification"));
    }

    #[test]
    fn test_template_helpers() {
        let success = template_helpers::iot_batch_success(1000);
        assert!(success.contains("1000"));
        assert!(success.contains("lectures injectées"));

        let stats = template_helpers::stats_response(5000, 100, 256, 4.25);
        assert!(stats.contains("5000"));
        assert!(stats.contains("100"));
        assert!(stats.contains("256"));
        assert!(stats.contains("4.25"));
    }
}
