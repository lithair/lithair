//! Static Asset Management for Lithair Frontend

use crate::engine::Event;
use crate::model::{FieldPolicy, ModelSpec};
use crate::model_inspect::Inspectable;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// StaticAsset - Revolutionary memory-first asset serving
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticAsset {
    #[serde(default = "generate_uuid")]
    pub id: Uuid,
    pub path: String,
    pub content: Vec<u8>,
    pub mime_type: String,
    pub version: String,
    pub size_bytes: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deployment_source: Option<String>,
    pub compression_enabled: bool,
    pub cache_ttl_seconds: u32,
    pub metadata: HashMap<String, String>,
}

impl Inspectable for StaticAsset {
    fn get_field_value(&self, field_name: &str) -> Option<serde_json::Value> {
        match field_name {
            "id" => serde_json::to_value(self.id).ok(),
            "path" => serde_json::to_value(&self.path).ok(),
            "mime_type" => serde_json::to_value(&self.mime_type).ok(),
            "version" => serde_json::to_value(&self.version).ok(),
            "size_bytes" => serde_json::to_value(self.size_bytes).ok(),
            "created_at" => serde_json::to_value(self.created_at).ok(),
            "updated_at" => serde_json::to_value(self.updated_at).ok(),
            "deployment_source" => serde_json::to_value(&self.deployment_source).ok(),
            "compression_enabled" => serde_json::to_value(self.compression_enabled).ok(),
            "cache_ttl_seconds" => serde_json::to_value(self.cache_ttl_seconds).ok(),
            // Note: 'content' and 'metadata' excluded from standard inspection for performance
            _ => None,
        }
    }
}

impl ModelSpec for StaticAsset {
    fn get_policy(&self, _field_name: &str) -> Option<FieldPolicy> {
        // StaticAsset uses default policies (no unique checks etc. except path maybe?)
        // Actually, path should be unique per virtual host?
        // For now, return None to disable engine-level checks.
        // FrontendEngine manages its own uniqueness via HashMap keys (path).
        None
    }

    fn get_all_fields(&self) -> Vec<String> {
        vec![
            "id".to_string(),
            "path".to_string(),
            "mime_type".to_string(),
            "version".to_string(),
            "size_bytes".to_string(),
            "created_at".to_string(),
            "updated_at".to_string(),
            "deployment_source".to_string(),
            "compression_enabled".to_string(),
            "cache_ttl_seconds".to_string(),
        ]
    }
}

// Make StaticAsset an Event so it can be persisted directly
impl Event for StaticAsset {
    type State = StaticAsset;

    fn apply(&self, state: &mut Self::State) {
        *state = self.clone();
    }

    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| String::from("{}"))
    }

    fn aggregate_id(&self) -> Option<String> {
        Some(format!("asset:{}", self.id))
    }
}

impl StaticAsset {
    pub fn new(path: String, content: Vec<u8>) -> Self {
        let mime_type = detect_mime_type(&path);
        let size_bytes = content.len() as u64;

        Self {
            id: generate_uuid(),
            path,
            content,
            compression_enabled: should_compress(&mime_type),
            cache_ttl_seconds: default_cache_ttl(&mime_type),
            mime_type,
            version: "v1.0.0".to_string(),
            size_bytes,
            created_at: Utc::now(),
            updated_at: None,
            deployment_source: None,
            metadata: HashMap::new(),
        }
    }

    /// Override the extension-derived MIME type with one the caller knows.
    ///
    /// `StaticAsset::new` derives `mime_type` from the path extension, so an
    /// extensionless clean URL (`/posts/hello`) lands on
    /// `application/octet-stream` and browsers download it instead of
    /// rendering (issue #193). A caller that rendered the content knows its
    /// real type; this setter records it and recomputes the mime-derived
    /// defaults (compression, cache TTL) so they stay consistent with what
    /// `new` would have produced for that type.
    pub fn set_mime_type(&mut self, mime_type: &str) {
        self.compression_enabled = should_compress(mime_type);
        self.cache_ttl_seconds = default_cache_ttl(mime_type);
        self.mime_type = mime_type.to_string();
    }

    pub fn http_headers(&self) -> Vec<(String, String)> {
        vec![
            ("Content-Type".to_string(), self.mime_type.clone()),
            ("Content-Length".to_string(), self.size_bytes.to_string()),
            (
                "Cache-Control".to_string(),
                format!("public, max-age={}", self.cache_ttl_seconds),
            ),
            ("X-Served-From".to_string(), "Lithair-Memory".to_string()),
            ("X-Asset-Version".to_string(), self.version.clone()),
        ]
    }
}

impl Default for StaticAsset {
    fn default() -> Self {
        Self {
            id: generate_uuid(),
            path: "/".to_string(),
            content: Vec::new(),
            mime_type: "text/html".to_string(),
            version: "v1.0.0".to_string(),
            size_bytes: 0,
            created_at: Utc::now(),
            updated_at: None,
            deployment_source: None,
            compression_enabled: false,
            cache_ttl_seconds: 3600,
            metadata: HashMap::new(),
        }
    }
}

fn generate_uuid() -> Uuid {
    Uuid::new_v4()
}

fn detect_mime_type(path: &str) -> String {
    let extension = path.rsplit('.').next().unwrap_or("");

    // The catch-all returns `application/octet-stream`, which means
    // every uncovered extension serves bytes correctly but advertises
    // the wrong type — bad for SEO, RSS readers, and feed validators.
    // Issue #56's acceptance criteria explicitly call out `/rss.xml`
    // (must be `application/xml`), so XML and the most common Astro /
    // Lithair-served extensions are mapped explicitly. Everything
    // else still falls through to `application/octet-stream`, which
    // remains the safe default for unknown binary content.
    match extension.to_lowercase().as_str() {
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "application/javascript",
        "json" => "application/json",
        "xml" | "rss" | "atom" => "application/xml",
        "txt" | "md" => "text/plain; charset=utf-8",
        "ico" => "image/x-icon",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "webmanifest" | "manifest" => "application/manifest+json",
        "pdf" => "application/pdf",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
    .to_string()
}

fn should_compress(mime_type: &str) -> bool {
    // Strip any MIME parameters (e.g. `; charset=utf-8`) before
    // matching — `detect_mime_type` now emits charset-qualified
    // values for text/* types, and the previous exact-string match
    // would drop those from the compressible set silently.
    // (Reported by CodeRabbit on PR #57.)
    let base = mime_type.split(';').next().unwrap_or(mime_type).trim();
    matches!(
        base,
        "text/html"
            | "text/css"
            | "text/plain"
            | "application/javascript"
            | "application/json"
            | "application/xml"
            | "application/manifest+json"
            | "image/svg+xml"
    )
}

fn default_cache_ttl(mime_type: &str) -> u32 {
    match mime_type {
        "text/html" => 300,
        "text/css" | "application/javascript" => 3600,
        mime if mime.starts_with("image/") => 86400,
        _ => 3600,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_mime_type_maps_common_static_extensions() {
        // Spot-check the extensions issue #56 specifically called out
        // (XML and the broader Astro-generated set). The previous
        // table only covered html/css/js/json/png/jpg/gif/svg, which
        // meant rss.xml and .webmanifest were served as
        // `application/octet-stream` — wrong for feed readers and
        // PWAs.
        assert_eq!(detect_mime_type("/rss.xml"), "application/xml");
        assert_eq!(detect_mime_type("/sitemap.xml"), "application/xml");
        assert_eq!(detect_mime_type("/index.html"), "text/html");
        assert_eq!(detect_mime_type("/site.webmanifest"), "application/manifest+json");
        assert_eq!(detect_mime_type("/font.woff2"), "font/woff2");
        assert_eq!(detect_mime_type("/favicon.ico"), "image/x-icon");
        // Unknown extension still falls through to the safe default.
        assert_eq!(detect_mime_type("/blob.unknown"), "application/octet-stream");
    }

    #[test]
    fn detect_mime_type_is_case_insensitive() {
        // Astro sometimes generates uppercase extensions for assets
        // imported from external sources. The lookup must normalize.
        assert_eq!(detect_mime_type("/MAP.XML"), "application/xml");
        assert_eq!(detect_mime_type("/INDEX.HTML"), "text/html");
    }

    #[test]
    fn set_mime_type_overrides_extension_detection_and_recomputes_defaults() {
        // Issue #193: an extensionless clean URL gets octet-stream from
        // extension detection, which also disables compression and picks the
        // generic TTL. The override must fix all three, matching what `new`
        // would produce for the declared type.
        let mut asset = StaticAsset::new("/posts/hello".to_string(), b"<h1>hi</h1>".to_vec());
        assert_eq!(asset.mime_type, "application/octet-stream");
        assert!(!asset.compression_enabled);

        asset.set_mime_type("text/html");
        assert_eq!(asset.mime_type, "text/html");
        assert!(asset.compression_enabled);
        assert_eq!(asset.cache_ttl_seconds, 300);
    }

    #[test]
    fn should_compress_ignores_mime_parameters_and_covers_new_types() {
        // Regression guard: `text/plain` now comes back with a
        // charset suffix from `detect_mime_type`, and the previous
        // exact-string match silently dropped it from compression.
        // Both forms must compress.
        assert!(should_compress("text/plain"));
        assert!(should_compress("text/plain; charset=utf-8"));
        assert!(should_compress("text/html"));
        // New MIME types added in this fix should be compressible.
        assert!(should_compress("application/xml"));
        assert!(should_compress("application/manifest+json"));
        assert!(should_compress("image/svg+xml"));
        // Non-text/non-structured types still skip compression.
        assert!(!should_compress("image/png"));
        assert!(!should_compress("font/woff2"));
        assert!(!should_compress("application/octet-stream"));
    }
}
