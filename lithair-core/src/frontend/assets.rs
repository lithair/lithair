//! Static Asset Management for Lithair Frontend

use crate::engine::Event;
use crate::model_inspect::Inspectable;
use crate::model::{FieldPolicy, ModelSpec};
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
            _ => None
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

    pub fn http_headers(&self) -> Vec<(String, String)> {
        vec![
            ("Content-Type".to_string(), self.mime_type.clone()),
            ("Content-Length".to_string(), self.size_bytes.to_string()),
            ("Cache-Control".to_string(), format!("public, max-age={}", self.cache_ttl_seconds)),
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

    match extension.to_lowercase().as_str() {
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "application/javascript",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }.to_string()
}

fn should_compress(mime_type: &str) -> bool {
    matches!(mime_type,
        "text/html" | "text/css" | "application/javascript" |
        "application/json" | "text/plain"
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
