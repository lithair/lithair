use crate::lifecycle::RetentionConfig;
use scc::HashMap as SccHashMap;
use serde::Serialize;
use std::collections::{HashSet, VecDeque};
use std::sync::Mutex;

/// Tracks evicted items' pinned field data, hot-storage insertion order,
/// per-item timestamps + sizes for the count / duration / budget eviction
/// modes (issue #97).
///
/// The hot map (caller's `HashMap<String, T>` or `Scc2Engine.state_map`)
/// holds fully-projected items. The warm map here holds lightweight
/// pinned-field JSON for evicted items, enabling fast listing without
/// loading full data from disk.
pub struct RetentionLayer {
    config: RetentionConfig,
    pinned_field_names: Vec<String>,
    warm_map: SccHashMap<String, WarmEntry>,
    /// FIFO queue of `OrderEntry { key, last_updated, size_bytes }` +
    /// HashSet sidecar for O(1) membership checks + running `total_bytes`.
    /// All three are kept in sync under the same mutex.
    order_state: Mutex<OrderState>,
}

struct OrderEntry {
    key: String,
    /// UNIX seconds — used by duration-based eviction.
    last_updated: u64,
    /// Serialized hot-storage cost of this item — used by budget-based eviction.
    size_bytes: usize,
}

struct OrderState {
    queue: VecDeque<OrderEntry>,
    set: HashSet<String>,
    total_bytes: usize,
}

impl OrderState {
    fn new() -> Self {
        Self { queue: VecDeque::new(), set: HashSet::new(), total_bytes: 0 }
    }

    /// Move (or insert) `key` to the back of the queue with the given
    /// `last_updated` and `size_bytes`. If the key was already present,
    /// the old entry is removed first and its size subtracted from the
    /// running total — semantically "most recent insert wins".
    fn touch(&mut self, key: String, last_updated: u64, size_bytes: usize) {
        if self.set.contains(&key) {
            if let Some(pos) = self.queue.iter().position(|e| e.key == key) {
                let old = self.queue.remove(pos).expect("position just found");
                self.total_bytes = self.total_bytes.saturating_sub(old.size_bytes);
            }
        } else {
            self.set.insert(key.clone());
        }
        self.total_bytes = self.total_bytes.saturating_add(size_bytes);
        self.queue.push_back(OrderEntry { key, last_updated, size_bytes });
    }

    fn pop_front(&mut self) -> Option<OrderEntry> {
        let entry = self.queue.pop_front()?;
        self.set.remove(&entry.key);
        self.total_bytes = self.total_bytes.saturating_sub(entry.size_bytes);
        Some(entry)
    }

    fn peek_front(&self) -> Option<&OrderEntry> {
        self.queue.front()
    }

    fn remove(&mut self, key: &str) {
        if self.set.remove(key) {
            if let Some(pos) = self.queue.iter().position(|e| e.key == key) {
                let entry = self.queue.remove(pos).expect("position just found");
                self.total_bytes = self.total_bytes.saturating_sub(entry.size_bytes);
            }
        }
    }

    fn clear(&mut self) {
        self.queue.clear();
        self.set.clear();
        self.total_bytes = 0;
    }
}

/// Lightweight representation of an evicted item — only pinned fields + metadata.
#[derive(Debug, Clone)]
pub struct WarmEntry {
    pub pinned_data: serde_json::Value,
    pub version: u64,
    pub last_updated: u64,
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
        self.config.is_configured()
    }

    /// Names of fields marked `#[pinned]` on the model. Used by uniqueness
    /// checks to know which fields can be safely scanned from warm entries.
    pub fn pinned_field_names(&self) -> &[String] {
        &self.pinned_field_names
    }

    /// Record that a key was inserted/updated in the hot map. Returns the
    /// list of keys to evict — possibly empty, possibly several (when
    /// budget mode needs to drop many small old items at once).
    ///
    /// `last_updated` and `size_bytes` describe the JUST-inserted item.
    /// Duration eviction uses `last_updated` as the "now" reference clock
    /// (since the caller computes it from `SystemTime::now()` in production,
    /// and tests can pass a controlled value).
    pub fn track_insert(&self, key: &str, last_updated: u64, size_bytes: usize) -> Vec<String> {
        let mut order = self.order_state.lock().expect("order_state lock poisoned");

        order.touch(key.to_string(), last_updated, size_bytes);

        let mut to_evict = Vec::new();

        // Evict in a loop: budget mode may require dropping several small
        // old items before the budget is back under the cap.
        while self.needs_eviction(&order, last_updated) {
            match order.pop_front() {
                Some(entry) => to_evict.push(entry.key),
                None => break,
            }
        }
        to_evict
    }

    fn needs_eviction(&self, order: &OrderState, now: u64) -> bool {
        if let Some(limit) = self.config.memory_count {
            if order.queue.len() > limit {
                return true;
            }
        }
        if let Some(budget) = self.config.memory_budget_bytes {
            if order.total_bytes > budget {
                return true;
            }
        }
        if let Some(dur) = self.config.memory_duration_secs {
            if let Some(oldest) = order.peek_front() {
                if now.saturating_sub(oldest.last_updated) > dur {
                    return true;
                }
            }
        }
        false
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
    /// The caller is expected to immediately follow with `track_insert` so
    /// the order_state stays consistent — this method only clears the warm
    /// entry, it does not re-add to the order queue.
    pub fn promote_from_warm(&self, key: &str) {
        if let Some(scc::hash_map::Entry::Occupied(o)) = self.warm_map.try_entry(key.to_string()) {
            let _ = o.remove();
        }
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
    pub fn warm_keys_with_field_value(
        &self,
        field: &str,
        value: &serde_json::Value,
    ) -> Vec<String> {
        let mut result = Vec::new();
        self.warm_map.retain_sync(|key, entry| {
            if let Some(field_value) = entry.pinned_data.get(field) {
                if field_value == value {
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
        RetentionConfig {
            memory_count: Some(limit),
            memory_duration_secs: None,
            memory_budget_bytes: None,
        }
    }

    fn make_duration_config(duration_secs: u64) -> RetentionConfig {
        RetentionConfig {
            memory_count: None,
            memory_duration_secs: Some(duration_secs),
            memory_budget_bytes: None,
        }
    }

    fn make_budget_config(budget_bytes: usize) -> RetentionConfig {
        RetentionConfig {
            memory_count: None,
            memory_duration_secs: None,
            memory_budget_bytes: Some(budget_bytes),
        }
    }

    #[test]
    fn no_eviction_below_limit() {
        let layer = RetentionLayer::new(make_config(5), vec!["from".into()]);
        let result = layer.track_insert("a", 0, 10);
        assert!(result.is_empty());
    }

    #[test]
    fn eviction_when_over_limit() {
        let layer = RetentionLayer::new(make_config(2), vec!["from".into()]);
        layer.track_insert("a", 1, 10);
        layer.track_insert("b", 2, 10);
        let result = layer.track_insert("c", 3, 10);
        assert_eq!(result, vec!["a".to_string()]);
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
        let result = layer.track_insert("anything", 0, 10);
        assert!(result.is_empty());
    }

    #[test]
    fn limit_zero_evicts_inserted_key() {
        let layer = RetentionLayer::new(make_config(0), vec!["from".into()]);
        let result = layer.track_insert("k1", 1, 10);
        assert_eq!(result, vec!["k1".to_string()]);
    }

    #[test]
    fn duration_evicts_when_older_than_cutoff() {
        // 30s retention; first insert at t=100, second at t=200 — first should evict.
        let layer = RetentionLayer::new(make_duration_config(30), vec!["from".into()]);
        let result = layer.track_insert("old", 100, 10);
        assert!(result.is_empty(), "no eviction on initial insert");

        let result = layer.track_insert("new", 200, 10);
        assert_eq!(result, vec!["old".to_string()], "old item past duration cutoff");
    }

    #[test]
    fn duration_keeps_recent_items() {
        // 30s retention; both inserts within window — no eviction.
        let layer = RetentionLayer::new(make_duration_config(30), vec!["from".into()]);
        layer.track_insert("a", 100, 10);
        let result = layer.track_insert("b", 110, 10);
        assert!(result.is_empty(), "both inserts within duration window");
    }

    #[test]
    fn budget_evicts_oldest_until_under_cap() {
        // 100B budget; each insert is 40B. Third insert triggers eviction of first.
        let layer = RetentionLayer::new(make_budget_config(100), vec!["from".into()]);
        assert!(layer.track_insert("a", 1, 40).is_empty());
        assert!(layer.track_insert("b", 2, 40).is_empty()); // total 80, ok
        let result = layer.track_insert("c", 3, 40); // total would be 120 > 100
        assert_eq!(result, vec!["a".to_string()]);
    }

    #[test]
    fn budget_evicts_multiple_when_oversized_item_lands() {
        // 100B budget; small items 10B each, then one 95B item.
        // Total before big = 30B. After insert: 30 + 95 = 125 > 100.
        // Evict a → 115, evict b → 105, evict c → 95 ≤ 100 → stop.
        // Result: 3 small items evicted in one call.
        let layer = RetentionLayer::new(make_budget_config(100), vec!["from".into()]);
        layer.track_insert("a", 1, 10);
        layer.track_insert("b", 2, 10);
        layer.track_insert("c", 3, 10);
        let result = layer.track_insert("big", 4, 95);
        assert_eq!(result, vec!["a", "b", "c"], "multiple keys evicted in one call");
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

        let matches = layer.warm_keys_with_field_value("from", &serde_json::json!("a@x"));
        assert_eq!(matches.len(), 2);
        assert!(matches.contains(&"k1".to_string()));
        assert!(matches.contains(&"k3".to_string()));

        let no_match = layer.warm_keys_with_field_value("from", &serde_json::json!("nobody@x"));
        assert!(no_match.is_empty());

        // Non-pinned field "body" isn't stored in warm — should return empty.
        let body_search = layer.warm_keys_with_field_value("body", &serde_json::json!("b1"));
        assert!(body_search.is_empty());
    }

    #[test]
    fn clear_empties_warm_and_order() {
        let layer = RetentionLayer::new(make_config(1), vec!["from".into()]);
        let email = TestEmail { from: "x".into(), subject: "y".into(), body: "z".into() };
        layer.evict_to_warm("k1", &email, 1, 100);
        layer.evict_to_warm("k2", &email, 2, 200);
        layer.track_insert("k3", 1, 10);
        assert!(layer.is_evicted("k1"));
        assert!(layer.is_evicted("k2"));

        layer.clear();

        assert!(!layer.is_evicted("k1"));
        assert!(!layer.is_evicted("k2"));
        assert_eq!(layer.warm_count(), 0);
    }
}
