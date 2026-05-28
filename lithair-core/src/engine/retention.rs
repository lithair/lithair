use crate::lifecycle::RetentionConfig;
use scc::HashMap as SccHashMap;
use serde::Serialize;
use std::collections::{HashSet, VecDeque};
use std::sync::Mutex;

/// Tracks evicted items' pinned field data and insertion order for FIFO eviction.
///
/// The hot map (Scc2Engine.state_map) holds the last N fully-projected items.
/// The warm map here holds lightweight pinned-field JSON for evicted items,
/// enabling fast listing and filtering without loading full data from disk.
pub struct RetentionLayer {
    config: RetentionConfig,
    pinned_field_names: Vec<String>,
    warm_map: SccHashMap<String, WarmEntry>,
    /// FIFO queue of inserted keys + HashSet sidecar for O(1) membership checks.
    /// Both structures are kept in sync under the same mutex. The HashSet avoids
    /// O(N) linear scans on `track_insert`/`promote_from_warm` (issue #98).
    order_state: Mutex<OrderState>,
}

struct OrderState {
    queue: VecDeque<String>,
    set: HashSet<String>,
}

impl OrderState {
    fn new() -> Self {
        Self { queue: VecDeque::new(), set: HashSet::new() }
    }

    fn push_back(&mut self, key: String) {
        if self.set.insert(key.clone()) {
            self.queue.push_back(key);
        }
    }

    fn pop_front(&mut self) -> Option<String> {
        let key = self.queue.pop_front()?;
        self.set.remove(&key);
        Some(key)
    }

    fn push_front(&mut self, key: String) {
        if self.set.insert(key.clone()) {
            self.queue.push_front(key);
        }
    }

    fn remove(&mut self, key: &str) {
        if self.set.remove(key) {
            if let Some(pos) = self.queue.iter().position(|k| k == key) {
                self.queue.remove(pos);
            }
        }
    }

    fn clear(&mut self) {
        self.queue.clear();
        self.set.clear();
    }
}

/// Lightweight representation of an evicted item — only pinned fields + metadata.
#[derive(Debug, Clone)]
pub struct WarmEntry {
    pub pinned_data: serde_json::Value,
    pub version: u64,
    pub last_updated: u64,
}

/// Result of an eviction check: either no eviction needed, or one item to evict.
pub struct EvictionResult {
    pub evict_key: Option<String>,
}

impl RetentionLayer {
    pub fn new(config: RetentionConfig, pinned_field_names: Vec<String>) -> Self {
        Self {
            config,
            pinned_field_names,
            warm_map: SccHashMap::new(),
            order_state: Mutex::new(OrderState::new()),
        }
    }

    pub fn memory_limit(&self) -> Option<usize> {
        self.config.memory_count
    }

    pub fn is_active(&self) -> bool {
        self.config.memory_count.is_some()
    }

    /// Names of fields marked `#[pinned]` on the model. Used by uniqueness
    /// checks to know which fields can be safely scanned from warm entries.
    pub fn pinned_field_names(&self) -> &[String] {
        &self.pinned_field_names
    }

    /// Record that a key was inserted/updated in the hot map.
    /// Returns the key to evict if the hot map now exceeds capacity.
    pub fn track_insert(&self, key: &str, hot_count: usize) -> EvictionResult {
        let mut order = self.order_state.lock().expect("order_state lock poisoned");

        order.push_back(key.to_string());

        let limit = match self.config.memory_count {
            Some(n) => n,
            None => return EvictionResult { evict_key: None },
        };

        if hot_count > limit {
            // limit == 0: nothing should stay in hot; evict the just-inserted key.
            if limit == 0 {
                order.remove(key);
                return EvictionResult { evict_key: Some(key.to_string()) };
            }
            if let Some(oldest) = order.pop_front() {
                if oldest == key {
                    let next = order.pop_front();
                    order.push_front(oldest);
                    return EvictionResult { evict_key: next };
                }
                return EvictionResult { evict_key: Some(oldest) };
            }
        }

        EvictionResult { evict_key: None }
    }

    /// Clear all warm entries and reset the eviction queue.
    /// Used during full state replacement (e.g., follower reconcile from leader).
    pub fn clear(&self) {
        self.warm_map.retain_sync(|_, _| false);
        let mut order = self.order_state.lock().expect("order_state lock poisoned");
        order.clear();
    }

    /// Move an item from hot to warm: extract pinned fields and store as JSON.
    pub fn evict_to_warm<S: Serialize>(
        &self,
        key: &str,
        data: &S,
        version: u64,
        last_updated: u64,
    ) {
        let pinned_data = self.extract_pinned_fields(data);
        let entry = WarmEntry { pinned_data, version, last_updated };
        let _ = self.warm_map.insert_sync(key.to_string(), entry);
    }

    /// Promote an item back from warm to hot (e.g., on update of evicted item).
    pub fn promote_from_warm(&self, key: &str) {
        if let Some(scc::hash_map::Entry::Occupied(o)) = self.warm_map.try_entry(key.to_string()) {
            let _ = o.remove();
        }
        let mut order = self.order_state.lock().expect("order_state lock poisoned");
        order.push_back(key.to_string());
    }

    /// Remove a key entirely (deletion).
    pub fn remove(&self, key: &str) {
        if let Some(scc::hash_map::Entry::Occupied(o)) = self.warm_map.try_entry(key.to_string()) {
            let _ = o.remove();
        }
        let mut order = self.order_state.lock().expect("order_state lock poisoned");
        order.remove(key);
    }

    /// Read pinned data for an evicted item (for listing/filtering).
    pub fn read_warm(&self, key: &str) -> Option<WarmEntry> {
        if let Some(scc::hash_map::Entry::Occupied(o)) = self.warm_map.try_entry(key.to_string()) {
            Some(o.get().clone())
        } else {
            None
        }
    }

    /// Check if a key is in the warm map (evicted).
    pub fn is_evicted(&self, key: &str) -> bool {
        self.warm_map.contains_sync(key)
    }

    /// Get all warm (evicted) entries as (key, pinned_data) pairs.
    pub fn all_warm_entries(&self) -> Vec<(String, WarmEntry)> {
        let mut result = Vec::new();
        self.warm_map.retain_sync(|key, entry| {
            result.push((key.clone(), entry.clone()));
            true
        });
        result
    }

    /// Find warm entry keys whose pinned `field` equals `value`.
    /// Used by uniqueness validation to detect duplicates among evicted items.
    /// Only works for pinned fields; non-pinned fields aren't stored in warm entries.
    pub fn warm_keys_with_field_value(&self, field: &str, value: &str) -> Vec<String> {
        let mut result = Vec::new();
        self.warm_map.retain_sync(|key, entry| {
            if let Some(field_value) = entry.pinned_data.get(field) {
                let matches = match field_value {
                    serde_json::Value::String(s) => s == value,
                    other => other.to_string().as_str() == value,
                };
                if matches {
                    result.push(key.clone());
                }
            }
            true
        });
        result
    }

    /// Total number of items tracked (hot + warm).
    pub fn warm_count(&self) -> usize {
        self.warm_map.len()
    }

    fn extract_pinned_fields<S: Serialize>(&self, data: &S) -> serde_json::Value {
        let full_json = match serde_json::to_value(data) {
            Ok(v) => v,
            Err(_) => return serde_json::Value::Object(serde_json::Map::new()),
        };

        if self.pinned_field_names.is_empty() {
            return serde_json::Value::Object(serde_json::Map::new());
        }

        match full_json {
            serde_json::Value::Object(map) => {
                let mut pinned = serde_json::Map::new();
                for field_name in &self.pinned_field_names {
                    if let Some(value) = map.get(field_name) {
                        pinned.insert(field_name.clone(), value.clone());
                    }
                }
                serde_json::Value::Object(pinned)
            }
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Serialize, Clone, Default)]
    struct TestEmail {
        from: String,
        subject: String,
        body: String,
    }

    fn make_config(limit: usize) -> RetentionConfig {
        RetentionConfig { memory_count: Some(limit) }
    }

    #[test]
    fn no_eviction_below_limit() {
        let layer = RetentionLayer::new(make_config(5), vec!["from".into()]);
        let result = layer.track_insert("a", 3);
        assert!(result.evict_key.is_none());
    }

    #[test]
    fn eviction_when_over_limit() {
        let layer = RetentionLayer::new(make_config(2), vec!["from".into()]);
        layer.track_insert("a", 1);
        layer.track_insert("b", 2);
        let result = layer.track_insert("c", 3);
        assert_eq!(result.evict_key, Some("a".to_string()));
    }

    #[test]
    fn extract_pinned_fields_only() {
        let layer = RetentionLayer::new(make_config(1), vec!["from".into(), "subject".into()]);
        let email = TestEmail {
            from: "alice@test.com".into(),
            subject: "Hello".into(),
            body: "Very long body content...".into(),
        };
        let pinned = layer.extract_pinned_fields(&email);
        let obj = pinned.as_object().unwrap();
        assert_eq!(obj.len(), 2);
        assert_eq!(obj["from"], "alice@test.com");
        assert_eq!(obj["subject"], "Hello");
        assert!(!obj.contains_key("body"));
    }

    #[test]
    fn evict_and_read_warm() {
        let layer = RetentionLayer::new(make_config(1), vec!["from".into(), "subject".into()]);
        let email = TestEmail {
            from: "bob@test.com".into(),
            subject: "Test".into(),
            body: "Big content".into(),
        };
        layer.evict_to_warm("email-1", &email, 3, 1000);

        assert!(layer.is_evicted("email-1"));
        let warm = layer.read_warm("email-1").unwrap();
        assert_eq!(warm.version, 3);
        assert_eq!(warm.pinned_data["from"], "bob@test.com");
        assert_eq!(warm.pinned_data["subject"], "Test");
        assert!(warm.pinned_data.get("body").is_none());
    }

    #[test]
    fn promote_removes_from_warm() {
        let layer = RetentionLayer::new(make_config(1), vec!["from".into()]);
        let email = TestEmail { from: "x".into(), subject: "y".into(), body: "z".into() };
        layer.evict_to_warm("k1", &email, 1, 100);
        assert!(layer.is_evicted("k1"));

        layer.promote_from_warm("k1");
        assert!(!layer.is_evicted("k1"));
    }

    #[test]
    fn no_retention_means_inactive() {
        let layer = RetentionLayer::new(RetentionConfig::default(), vec![]);
        assert!(!layer.is_active());
        let result = layer.track_insert("anything", 999999);
        assert!(result.evict_key.is_none());
    }

    #[test]
    fn limit_zero_evicts_inserted_key() {
        let layer = RetentionLayer::new(make_config(0), vec!["from".into()]);
        let result = layer.track_insert("k1", 1);
        assert_eq!(result.evict_key, Some("k1".to_string()));
    }

    #[test]
    fn warm_keys_with_field_value_finds_match() {
        let layer = RetentionLayer::new(make_config(1), vec!["from".into(), "subject".into()]);
        let e1 = TestEmail { from: "a@x".into(), subject: "Hi".into(), body: "b1".into() };
        let e2 = TestEmail { from: "b@x".into(), subject: "Hi".into(), body: "b2".into() };
        let e3 = TestEmail { from: "a@x".into(), subject: "Bye".into(), body: "b3".into() };

        layer.evict_to_warm("k1", &e1, 1, 100);
        layer.evict_to_warm("k2", &e2, 2, 200);
        layer.evict_to_warm("k3", &e3, 3, 300);

        let matches = layer.warm_keys_with_field_value("from", "a@x");
        assert_eq!(matches.len(), 2);
        assert!(matches.contains(&"k1".to_string()));
        assert!(matches.contains(&"k3".to_string()));

        let no_match = layer.warm_keys_with_field_value("from", "nobody@x");
        assert!(no_match.is_empty());

        // Non-pinned field "body" isn't stored in warm — should return empty.
        let body_search = layer.warm_keys_with_field_value("body", "b1");
        assert!(body_search.is_empty());
    }

    #[test]
    fn clear_empties_warm_and_order() {
        let layer = RetentionLayer::new(make_config(1), vec!["from".into()]);
        let email = TestEmail { from: "x".into(), subject: "y".into(), body: "z".into() };
        layer.evict_to_warm("k1", &email, 1, 100);
        layer.evict_to_warm("k2", &email, 2, 200);
        layer.track_insert("k3", 1);
        assert!(layer.is_evicted("k1"));
        assert!(layer.is_evicted("k2"));

        layer.clear();

        assert!(!layer.is_evicted("k1"));
        assert!(!layer.is_evicted("k2"));
        assert_eq!(layer.warm_count(), 0);
    }
}
