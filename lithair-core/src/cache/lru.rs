//! LRU (Least Recently Used) cache implementation
//!
//! Provides an efficient LRU cache with O(1) get/insert operations.
//! Uses a HashMap for fast lookups and a doubly-linked list for LRU tracking.

use super::traits::{Cache, CacheEntry};
use std::collections::HashMap;
use std::hash::Hash;

/// Node in the LRU linked list
struct LruNode<K, V> {
    key: K,
    entry: CacheEntry<V>,
    prev: Option<usize>,
    next: Option<usize>,
}

/// LRU Cache with configurable capacity
pub struct LruCache<K, V>
where
    K: Hash + Eq + Clone,
{
    capacity: usize,
    map: HashMap<K, usize>,
    nodes: Vec<Option<LruNode<K, V>>>,
    head: Option<usize>,
    tail: Option<usize>,
    free_list: Vec<usize>,
}

impl<K, V> LruCache<K, V>
where
    K: Hash + Eq + Clone,
{
    /// Create a new LRU cache with the given capacity
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "Cache capacity must be greater than 0");

        Self {
            capacity,
            map: HashMap::with_capacity(capacity),
            nodes: Vec::with_capacity(capacity),
            head: None,
            tail: None,
            free_list: Vec::new(),
        }
    }

    /// Move a node to the front of the list (most recently used)
    fn move_to_front(&mut self, idx: usize) {
        if self.head == Some(idx) {
            return; // Already at front
        }

        // Get the node's prev and next indices before making mutable borrows
        let (prev_idx, next_idx) = if let Some(node) = &self.nodes[idx] {
            (node.prev, node.next)
        } else {
            return;
        };

        // Remove from current position
        if let Some(prev_idx) = prev_idx {
            if let Some(prev_node) = &mut self.nodes[prev_idx] {
                prev_node.next = next_idx;
            }
        }

        if let Some(next_idx) = next_idx {
            if let Some(next_node) = &mut self.nodes[next_idx] {
                next_node.prev = prev_idx;
            }
        }

        if self.tail == Some(idx) {
            self.tail = prev_idx;
        }

        // Insert at front
        let old_head = self.head;
        if let Some(node) = &mut self.nodes[idx] {
            node.prev = None;
            node.next = old_head;
        }

        if let Some(old_head_idx) = old_head {
            if let Some(old_head_node) = &mut self.nodes[old_head_idx] {
                old_head_node.prev = Some(idx);
            }
        }

        self.head = Some(idx);

        if self.tail.is_none() {
            self.tail = Some(idx);
        }
    }

    /// Remove the least recently used item (tail)
    fn evict_lru(&mut self) -> Option<(K, V)> {
        let tail_idx = self.tail?;

        let node = self.nodes[tail_idx].take()?;
        self.map.remove(&node.key);
        self.free_list.push(tail_idx);

        if let Some(prev_idx) = node.prev {
            if let Some(prev_node) = &mut self.nodes[prev_idx] {
                prev_node.next = None;
            }
            self.tail = Some(prev_idx);
        } else {
            self.head = None;
            self.tail = None;
        }

        Some((node.key, node.entry.value))
    }

    /// Get a node index, either from free list or by allocating new
    fn get_node_index(&mut self) -> usize {
        if let Some(idx) = self.free_list.pop() {
            idx
        } else {
            let idx = self.nodes.len();
            self.nodes.push(None);
            idx
        }
    }
}

impl<K, V> Cache<K, V> for LruCache<K, V>
where
    K: Hash + Eq + Clone,
{
    fn get(&mut self, key: &K) -> Option<&V> {
        let idx = *self.map.get(key)?;
        self.move_to_front(idx);

        if let Some(node) = &mut self.nodes[idx] {
            node.entry.touch();
            Some(&node.entry.value)
        } else {
            None
        }
    }

    fn insert(&mut self, key: K, value: V) -> Option<V> {
        // Check if key already exists
        if let Some(&idx) = self.map.get(&key) {
            let old_value = if let Some(node) = &mut self.nodes[idx] {
                let old = std::mem::replace(&mut node.entry.value, value);
                node.entry.touch();
                Some(old)
            } else {
                None
            };

            self.move_to_front(idx);
            return old_value;
        }

        // Evict if at capacity
        if self.map.len() >= self.capacity {
            self.evict_lru();
        }

        // Insert new node
        let idx = self.get_node_index();
        let entry = CacheEntry::new(value);

        self.nodes[idx] = Some(LruNode {
            key: key.clone(),
            entry,
            prev: None,
            next: self.head,
        });

        if let Some(old_head) = self.head {
            if let Some(old_head_node) = &mut self.nodes[old_head] {
                old_head_node.prev = Some(idx);
            }
        }

        self.head = Some(idx);

        if self.tail.is_none() {
            self.tail = Some(idx);
        }

        self.map.insert(key, idx);
        None
    }

    fn remove(&mut self, key: &K) -> Option<V> {
        let idx = self.map.remove(key)?;
        let node = self.nodes[idx].take()?;

        if let Some(prev_idx) = node.prev {
            if let Some(prev_node) = &mut self.nodes[prev_idx] {
                prev_node.next = node.next;
            }
        } else {
            self.head = node.next;
        }

        if let Some(next_idx) = node.next {
            if let Some(next_node) = &mut self.nodes[next_idx] {
                next_node.prev = node.prev;
            }
        } else {
            self.tail = node.prev;
        }

        self.free_list.push(idx);
        Some(node.entry.value)
    }

    fn clear(&mut self) {
        self.map.clear();
        self.nodes.clear();
        self.free_list.clear();
        self.head = None;
        self.tail = None;
    }

    fn len(&self) -> usize {
        self.map.len()
    }

    fn capacity(&self) -> usize {
        self.capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lru_basic_operations() {
        let mut cache = LruCache::new(2);

        assert_eq!(cache.insert("a", 1), None);
        assert_eq!(cache.insert("b", 2), None);

        assert_eq!(cache.get(&"a"), Some(&1));
        assert_eq!(cache.get(&"b"), Some(&2));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn test_lru_eviction() {
        let mut cache = LruCache::new(2);

        cache.insert("a", 1);
        cache.insert("b", 2);
        cache.insert("c", 3); // Should evict "a"

        assert_eq!(cache.get(&"a"), None);
        assert_eq!(cache.get(&"b"), Some(&2));
        assert_eq!(cache.get(&"c"), Some(&3));
    }

    #[test]
    fn test_lru_update_existing() {
        let mut cache = LruCache::new(2);

        cache.insert("a", 1);
        cache.insert("b", 2);

        // Update "a" (should move to front)
        assert_eq!(cache.insert("a", 10), Some(1));

        cache.insert("c", 3); // Should evict "b", not "a"

        assert_eq!(cache.get(&"a"), Some(&10));
        assert_eq!(cache.get(&"b"), None);
        assert_eq!(cache.get(&"c"), Some(&3));
    }

    #[test]
    fn test_lru_remove() {
        let mut cache = LruCache::new(3);

        cache.insert("a", 1);
        cache.insert("b", 2);
        cache.insert("c", 3);

        assert_eq!(cache.remove(&"b"), Some(2));
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get(&"b"), None);
    }

    #[test]
    fn test_lru_clear() {
        let mut cache = LruCache::new(3);

        cache.insert("a", 1);
        cache.insert("b", 2);

        cache.clear();

        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
        assert_eq!(cache.get(&"a"), None);
    }

    #[test]
    fn test_lru_access_order() {
        let mut cache = LruCache::new(2);

        cache.insert("a", 1);
        cache.insert("b", 2);

        // Access "a" to make it most recent
        cache.get(&"a");

        // Insert "c", should evict "b" (least recent)
        cache.insert("c", 3);

        assert_eq!(cache.get(&"a"), Some(&1));
        assert_eq!(cache.get(&"b"), None);
        assert_eq!(cache.get(&"c"), Some(&3));
    }
}
