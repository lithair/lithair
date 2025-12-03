//! Caching module for Lithair
//!
//! Provides high-performance caching strategies including LRU (Least Recently Used).
//! Useful for proxy response caching, DNS caching, session caching, etc.

pub mod lru;
pub mod traits;

pub use lru::LruCache;
pub use traits::{Cache, CacheEntry};
