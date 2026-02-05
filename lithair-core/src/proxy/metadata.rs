//! Filter entry metadata
//!
//! Provides structures for tracking the origin and context of filter entries.
//! Essential for providing detailed block messages and audit trails.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(feature = "openapi")]
use utoipa::ToSchema;

/// Information about where a filter entry came from
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct EntryMetadata {
    /// Source that provided this entry (e.g., "FireHOL Level 1", "Manual Entry")
    #[cfg_attr(feature = "openapi", schema(example = "FireHOL Level 1"))]
    pub source_name: String,

    /// Category or type of threat (e.g., "malware", "spam", "phishing")
    #[cfg_attr(feature = "openapi", schema(example = "malware"))]
    pub category: Option<String>,

    /// Source URL where this entry was fetched from
    #[cfg_attr(
        feature = "openapi",
        schema(example = "https://reputation.alienvault.com/reputation.txt")
    )]
    pub source_url: String,

    /// Timestamp when this entry was last updated (Unix timestamp)
    #[cfg_attr(feature = "openapi", schema(example = 1678886400))]
    pub last_updated: u64,
}

impl EntryMetadata {
    /// Create metadata for a manual entry
    pub fn manual(category: Option<String>) -> Self {
        Self {
            source_name: "Manual Entry".to_string(),
            category,
            source_url: "API".to_string(),
            last_updated: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }

    /// Create metadata for an external source
    pub fn from_source(source_name: String, category: Option<String>, source_url: String) -> Self {
        Self {
            source_name,
            category,
            source_url,
            last_updated: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }

    /// Update the timestamp to now
    pub fn touch(&mut self) {
        self.last_updated =
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    }
}

/// A filter entry with its metadata
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct FilterEntry {
    /// The actual value to match against (IP, domain, fingerprint, etc.)
    #[cfg_attr(feature = "openapi", schema(example = "192.168.1.100"))]
    pub value: String,

    /// Metadata about where this entry came from
    pub metadata: EntryMetadata,
}

impl FilterEntry {
    /// Create a new filter entry
    pub fn new(value: String, metadata: EntryMetadata) -> Self {
        Self { value, metadata }
    }

    /// Create a manual filter entry
    pub fn manual(value: String, category: Option<String>) -> Self {
        Self { value, metadata: EntryMetadata::manual(category) }
    }

    /// Create a filter entry from an external source
    pub fn from_source(
        value: String,
        source_name: String,
        category: Option<String>,
        source_url: String,
    ) -> Self {
        Self { value, metadata: EntryMetadata::from_source(source_name, category, source_url) }
    }
}

/// Result of a block check with detailed information
#[derive(Debug, Clone)]
pub struct BlockResult {
    /// Whether the item should be blocked
    pub blocked: bool,

    /// Details about why it was blocked (if blocked)
    pub block_info: Option<BlockInfo>,
}

impl BlockResult {
    /// Create a non-blocked result
    pub fn allowed() -> Self {
        Self { blocked: false, block_info: None }
    }

    /// Create a blocked result with info
    pub fn blocked(entry: FilterEntry, category: String) -> Self {
        Self { blocked: true, block_info: Some(BlockInfo { entry, category }) }
    }
}

/// Information about why something was blocked
#[derive(Debug, Clone)]
pub struct BlockInfo {
    /// The entry that caused the block
    pub entry: FilterEntry,

    /// The filter category that matched
    pub category: String,
}

impl BlockInfo {
    /// Create a detailed error message for blocked requests
    pub fn create_message(&self) -> String {
        let category_display = match self.category.as_str() {
            "block_source_ips" => "Source IP",
            "block_target_hosts" => "Target Host",
            "block_server_cert_fingerprints_sha256" => "Server Certificate (SHA256)",
            "block_server_cert_fingerprints_sha1" => "Server Certificate (SHA1)",
            _ => "Content",
        };

        let threat_category = self
            .entry
            .metadata
            .category
            .as_ref()
            .map(|c| format!(" ({})", c))
            .unwrap_or_default();

        format!(
            "Access denied: {} blocked by {}{}. Source: {}",
            category_display,
            self.entry.metadata.source_name,
            threat_category,
            self.entry.metadata.source_url
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manual_metadata() {
        let metadata = EntryMetadata::manual(Some("test".to_string()));
        assert_eq!(metadata.source_name, "Manual Entry");
        assert_eq!(metadata.category, Some("test".to_string()));
        assert_eq!(metadata.source_url, "API");
        assert!(metadata.last_updated > 0);
    }

    #[test]
    fn test_filter_entry_from_source() {
        let entry = FilterEntry::from_source(
            "192.168.1.1".to_string(),
            "FireHOL".to_string(),
            Some("malware".to_string()),
            "https://example.com".to_string(),
        );

        assert_eq!(entry.value, "192.168.1.1");
        assert_eq!(entry.metadata.source_name, "FireHOL");
        assert_eq!(entry.metadata.category, Some("malware".to_string()));
    }

    #[test]
    fn test_block_result() {
        let allowed = BlockResult::allowed();
        assert!(!allowed.blocked);
        assert!(allowed.block_info.is_none());

        let entry = FilterEntry::manual("test".to_string(), None);
        let blocked = BlockResult::blocked(entry.clone(), "block_source_ips".to_string());
        assert!(blocked.blocked);
        assert!(blocked.block_info.is_some());
    }

    #[test]
    fn test_block_info_message() {
        let entry = FilterEntry::from_source(
            "192.168.1.1".to_string(),
            "FireHOL Level 1".to_string(),
            Some("malware".to_string()),
            "https://firehol.org".to_string(),
        );

        let info = BlockInfo { entry, category: "block_source_ips".to_string() };

        let message = info.create_message();
        assert!(message.contains("Source IP"));
        assert!(message.contains("FireHOL Level 1"));
        assert!(message.contains("malware"));
        assert!(message.contains("firehol.org"));
    }
}
