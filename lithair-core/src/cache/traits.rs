//! Core traits for caching functionality

use std::hash::Hash;

/// A cache entry with metadata
#[derive(Debug, Clone)]
pub struct CacheEntry<V> {
    /// The cached value
    pub value: V,

    /// When this entry was created (Unix timestamp)
    pub created_at: u64,

    /// When this entry was last accessed (Unix timestamp)
    pub last_accessed: u64,

    /// Number of times this entry has been accessed
    pub access_count: u64,
}

impl<V> CacheEntry<V> {
    /// Create a new cache entry
    pub fn new(value: V) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            value,
            created_at: now,
            last_accessed: now,
            access_count: 1,
        }
    }

    /// Mark this entry as accessed
    pub fn touch(&mut self) {
        self.last_accessed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.access_count += 1;
    }

    /// Check if this entry is expired
    pub fn is_expired(&self, ttl_seconds: u64) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        now - self.created_at > ttl_seconds
    }
}

/// Core caching trait
pub trait Cache<K, V>
where
    K: Hash + Eq,
{
    /// Get a value from the cache
    fn get(&mut self, key: &K) -> Option<&V>;

    /// Insert a value into the cache
    fn insert(&mut self, key: K, value: V) -> Option<V>;

    /// Remove a value from the cache
    fn remove(&mut self, key: &K) -> Option<V>;

    /// Clear all entries from the cache
    fn clear(&mut self);

    /// Get the number of entries in the cache
    fn len(&self) -> usize;

    /// Check if the cache is empty
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get the capacity of the cache
    fn capacity(&self) -> usize;
}
