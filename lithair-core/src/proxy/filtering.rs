//! Advanced filtering logic with metadata support
//!
//! Provides high-level filtering capabilities for proxy requests,
//! including support for multiple filter categories and detailed metadata.

use super::metadata::{BlockResult, FilterEntry};
use std::collections::{HashMap, HashSet};
use tokio::sync::RwLock;

/// Filter list manager with metadata support
pub struct FilterListManager {
    /// Legacy filter lists (value-only, for compatibility)
    legacy_lists: RwLock<HashMap<String, HashSet<String>>>,

    /// Enhanced filter lists with metadata
    enhanced_lists: RwLock<HashMap<String, HashMap<String, FilterEntry>>>,
}

impl FilterListManager {
    /// Create a new filter list manager
    pub fn new() -> Self {
        Self {
            legacy_lists: RwLock::new(HashMap::new()),
            enhanced_lists: RwLock::new(HashMap::new()),
        }
    }

    /// Add entries to a legacy list (no metadata)
    pub async fn add_legacy_entries(
        &self,
        category: &str,
        values: impl IntoIterator<Item = String>,
    ) {
        let mut lists = self.legacy_lists.write().await;
        let list = lists.entry(category.to_string()).or_insert_with(HashSet::new);
        for value in values {
            list.insert(value);
        }
    }

    /// Add entries with metadata to an enhanced list
    pub async fn add_entries(
        &self,
        category: &str,
        entries: impl IntoIterator<Item = FilterEntry>,
    ) {
        let mut lists = self.enhanced_lists.write().await;
        let list = lists.entry(category.to_string()).or_insert_with(HashMap::new);
        for entry in entries {
            list.insert(entry.value.clone(), entry);
        }
    }

    /// Check if a value is blocked in a category
    ///
    /// Checks both enhanced lists (with metadata) and legacy lists (without metadata).
    /// Returns detailed block information if blocked.
    pub async fn check_block(&self, category: &str, value: &str) -> BlockResult {
        // First check enhanced filter lists with metadata
        let enhanced_lists = self.enhanced_lists.read().await;
        if let Some(category_entries) = enhanced_lists.get(category) {
            if let Some(filter_entry) = category_entries.get(value) {
                return BlockResult::blocked(filter_entry.clone(), category.to_string());
            }
        }
        drop(enhanced_lists);

        // Fallback to legacy filter lists (for compatibility)
        let legacy_lists = self.legacy_lists.read().await;
        if let Some(category_entries) = legacy_lists.get(category) {
            if category_entries.contains(value) {
                // Create a FilterEntry for legacy entries
                let entry = FilterEntry::manual(value.to_string(), Some("legacy".to_string()));
                return BlockResult::blocked(entry, category.to_string());
            }
        }

        BlockResult::allowed()
    }

    /// Get all categories
    pub async fn list_categories(&self) -> Vec<String> {
        let enhanced = self.enhanced_lists.read().await;
        let legacy = self.legacy_lists.read().await;

        let mut categories: HashSet<String> = HashSet::new();
        categories.extend(enhanced.keys().cloned());
        categories.extend(legacy.keys().cloned());

        let mut result: Vec<String> = categories.into_iter().collect();
        result.sort();
        result
    }

    /// Get entries in a category (enhanced only)
    pub async fn get_category_entries(&self, category: &str) -> Option<Vec<FilterEntry>> {
        let lists = self.enhanced_lists.read().await;
        lists.get(category).map(|entries| {
            let mut result: Vec<FilterEntry> = entries.values().cloned().collect();
            result.sort_by(|a, b| a.value.cmp(&b.value));
            result
        })
    }

    /// Remove a value from a category
    pub async fn remove_entry(&self, category: &str, value: &str) -> bool {
        let mut enhanced = self.enhanced_lists.write().await;
        let mut removed = false;

        if let Some(category_entries) = enhanced.get_mut(category) {
            removed = category_entries.remove(value).is_some();
        }
        drop(enhanced);

        if !removed {
            let mut legacy = self.legacy_lists.write().await;
            if let Some(category_entries) = legacy.get_mut(category) {
                removed = category_entries.remove(value);
            }
        }

        removed
    }

    /// Clear all entries in a category
    pub async fn clear_category(&self, category: &str) {
        let mut enhanced = self.enhanced_lists.write().await;
        enhanced.remove(category);
        drop(enhanced);

        let mut legacy = self.legacy_lists.write().await;
        legacy.remove(category);
    }

    /// Get statistics for all categories
    pub async fn get_stats(&self) -> HashMap<String, usize> {
        let enhanced = self.enhanced_lists.read().await;
        let legacy = self.legacy_lists.read().await;

        let mut stats = HashMap::new();

        for (category, entries) in enhanced.iter() {
            stats.insert(category.clone(), entries.len());
        }

        for (category, entries) in legacy.iter() {
            *stats.entry(category.clone()).or_insert(0) += entries.len();
        }

        stats
    }

    /// Get legacy entries for a category (for API compatibility)
    pub async fn get_category_legacy_entries(&self, category: &str) -> Option<HashSet<String>> {
        let lists = self.legacy_lists.read().await;
        lists.get(category).cloned()
    }

    /// Replace all entries in a legacy category
    pub async fn replace_category_legacy(&self, category: &str, entries: HashSet<String>) {
        let mut lists = self.legacy_lists.write().await;
        lists.insert(category.to_string(), entries);
    }

    /// Replace all entries in an enhanced category with metadata
    pub async fn replace_category_enhanced(&self, category: &str, entries: Vec<FilterEntry>) {
        let mut lists = self.enhanced_lists.write().await;
        let map: HashMap<String, FilterEntry> =
            entries.into_iter().map(|e| (e.value.clone(), e)).collect();
        lists.insert(category.to_string(), map);
    }
}

impl Default for FilterListManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_add_and_check_enhanced_entries() {
        let manager = FilterListManager::new();

        let entries = vec![FilterEntry::from_source(
            "192.168.1.1".to_string(),
            "TestSource".to_string(),
            Some("malware".to_string()),
            "https://test.com".to_string(),
        )];

        manager.add_entries("block_ips", entries).await;

        let result = manager.check_block("block_ips", "192.168.1.1").await;
        assert!(result.blocked);
        assert!(result.block_info.is_some());

        let not_blocked = manager.check_block("block_ips", "10.0.0.1").await;
        assert!(!not_blocked.blocked);
    }

    #[tokio::test]
    async fn test_legacy_entries() {
        let manager = FilterListManager::new();

        manager.add_legacy_entries("block_domains", vec!["evil.com".to_string()]).await;

        let result = manager.check_block("block_domains", "evil.com").await;
        assert!(result.blocked);
    }

    #[tokio::test]
    async fn test_remove_entry() {
        let manager = FilterListManager::new();

        let entry = FilterEntry::manual("test".to_string(), None);
        manager.add_entries("test_category", vec![entry]).await;

        assert!(manager.remove_entry("test_category", "test").await);
        assert!(!manager.remove_entry("test_category", "test").await);

        let result = manager.check_block("test_category", "test").await;
        assert!(!result.blocked);
    }

    #[tokio::test]
    async fn test_get_stats() {
        let manager = FilterListManager::new();

        manager.add_legacy_entries("cat1", vec!["a".to_string(), "b".to_string()]).await;
        manager
            .add_entries("cat2", vec![FilterEntry::manual("c".to_string(), None)])
            .await;

        let stats = manager.get_stats().await;
        assert_eq!(*stats.get("cat1").unwrap(), 2);
        assert_eq!(*stats.get("cat2").unwrap(), 1);
    }
}
