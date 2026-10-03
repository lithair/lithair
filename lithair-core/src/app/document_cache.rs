//! L1 copies of records whose authority is an external store (RFC 304).
//!
//! A cache only ever holds copies: writes go to the authority, which then
//! invalidates. The **generation** closes the read/invalidate race: a reader
//! takes the generation before reading the authority and may only insert its
//! result if no invalidation happened since. A value read before a commit can
//! therefore never be cached after that commit's invalidation.
use std::{
    any::Any,
    collections::{BTreeMap, HashMap},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

/// "Not found" answers are cached for at most this long (or the TTL if shorter).
pub const NEGATIVE_TTL: Duration = Duration::from_secs(5);

/// Bounds of an L1 copy set, declared with `#[retention(...)]` on an external
/// storage model. At least one bound must be set for the cache to exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CachePolicy {
    /// `memory = N`: at most N records.
    pub max_items: Option<usize>,
    /// `max_mb = M`: at most M MiB of encoded documents.
    pub max_bytes: Option<usize>,
    /// `ttl = "5m"`: a copy older than this is never served.
    pub ttl: Option<Duration>,
}

impl CachePolicy {
    /// Apply the deploy-time overrides shared with native retention:
    /// `LT_<MODEL>_MEMORY_RETENTION`, `LT_<MODEL>_MEMORY_MAX_MB` and
    /// `LT_<MODEL>_CACHE_TTL`, where `<MODEL>` comes from `type_name`.
    pub fn with_env_overrides(mut self, type_name: &str) -> Self {
        let Some(prefix) = crate::lifecycle::model_env_prefix(type_name) else {
            return self;
        };
        let var = |suffix: &str| std::env::var(format!("{prefix}_{suffix}")).ok();
        if let Some(n) = var("MEMORY_RETENTION").and_then(|v| v.parse().ok()) {
            self.max_items = Some(n);
        }
        if let Some(mb) = var("MEMORY_MAX_MB").and_then(|v| v.parse::<usize>().ok()) {
            self.max_bytes = mb.checked_mul(1024 * 1024);
        }
        if let Some(secs) = var("CACHE_TTL").and_then(|v| crate::lifecycle::parse_duration(&v)) {
            self.ttl = Some(Duration::from_secs(secs));
        }
        self
    }

    pub fn is_bounded(&self) -> bool {
        self.max_items.is_some() || self.max_bytes.is_some() || self.ttl.is_some()
    }
}

/// Counters for one cache. They never expose documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub items: usize,
    pub bytes: usize,
}

type Value = Arc<dyn Any + Send + Sync>;

struct Entry {
    /// `None` caches "not found".
    value: Option<Value>,
    bytes: usize,
    stored: Instant,
    tick: u64,
}

#[derive(Default)]
struct State {
    entries: HashMap<String, Entry>,
    /// Least recently used first.
    order: BTreeMap<u64, String>,
    tick: u64,
    bytes: usize,
    stats: CacheStats,
}

/// A bounded LRU of type-erased documents for one model partition.
pub struct DocumentCache {
    policy: CachePolicy,
    state: Mutex<State>,
    generation: AtomicU64,
    enabled: AtomicBool,
}

/// The outcome of a cache lookup.
pub enum Lookup {
    /// A copy, or a cached "not found" (`None`).
    Hit(Option<Value>),
    /// Read the authority, then [`DocumentCache::insert`] with this generation.
    Miss { generation: u64 },
}

impl DocumentCache {
    pub fn new(policy: CachePolicy) -> Self {
        Self {
            policy,
            state: Mutex::new(State::default()),
            generation: AtomicU64::new(0),
            enabled: AtomicBool::new(true),
        }
    }

    pub fn policy(&self) -> CachePolicy {
        self.policy
    }

    pub fn lookup(&self, id: &str) -> Lookup {
        // Read the generation first: an invalidation racing with this lookup
        // makes the caller's later insert a no-op.
        let generation = self.generation.load(Ordering::Acquire);
        let mut state = self.lock();
        if self.enabled.load(Ordering::Acquire) {
            let live = state.entries.get(id).map(|entry| {
                let ttl = match entry.value {
                    Some(_) => self.policy.ttl,
                    None => Some(self.policy.ttl.map_or(NEGATIVE_TTL, |t| t.min(NEGATIVE_TTL))),
                };
                ttl.is_none_or(|ttl| entry.stored.elapsed() < ttl)
            });
            match live {
                Some(true) => {
                    state.tick += 1;
                    let tick = state.tick;
                    let entry = state.entries.get_mut(id).expect("present");
                    let previous = std::mem::replace(&mut entry.tick, tick);
                    let value = entry.value.clone();
                    state.order.remove(&previous);
                    state.order.insert(tick, id.to_owned());
                    state.stats.hits += 1;
                    return Lookup::Hit(value);
                }
                Some(false) => Self::remove(&mut state, id),
                None => {}
            }
        }
        state.stats.misses += 1;
        Lookup::Miss { generation }
    }

    /// Store what the authority returned for `id`, unless the cache was
    /// invalidated or disabled since the lookup that produced `generation`.
    pub fn insert(&self, id: &str, value: Option<Value>, bytes: usize, generation: u64) {
        let mut state = self.lock();
        if !self.enabled.load(Ordering::Acquire)
            || self.generation.load(Ordering::Acquire) != generation
            || self.policy.max_bytes.is_some_and(|max| bytes > max)
        {
            return;
        }
        Self::remove(&mut state, id);
        state.tick += 1;
        let tick = state.tick;
        state.order.insert(tick, id.to_owned());
        state.bytes += bytes;
        state
            .entries
            .insert(id.to_owned(), Entry { value, bytes, stored: Instant::now(), tick });
        while state.entries.len() > self.policy.max_items.unwrap_or(usize::MAX)
            || state.bytes > self.policy.max_bytes.unwrap_or(usize::MAX)
        {
            let Some((_, oldest)) = state.order.pop_first() else { break };
            if let Some(entry) = state.entries.remove(&oldest) {
                state.bytes -= entry.bytes;
                state.stats.evictions += 1;
            }
        }
    }

    /// Drop the copy of `id` (after a committed or possibly committed write).
    pub fn invalidate(&self, id: &str) {
        let mut state = self.lock();
        self.generation.fetch_add(1, Ordering::AcqRel);
        Self::remove(&mut state, id);
    }

    /// Drop every copy (a migration, or lost invalidations).
    pub fn clear(&self) {
        let mut state = self.lock();
        self.generation.fetch_add(1, Ordering::AcqRel);
        state.entries.clear();
        state.order.clear();
        state.bytes = 0;
    }

    /// A disabled cache is empty, misses on every lookup and refuses inserts:
    /// used while invalidations cannot be received.
    pub fn set_enabled(&self, enabled: bool) {
        self.clear();
        self.enabled.store(enabled, Ordering::Release);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    pub fn stats(&self) -> CacheStats {
        let state = self.lock();
        CacheStats { items: state.entries.len(), bytes: state.bytes, ..state.stats }
    }

    fn remove(state: &mut State, id: &str) {
        if let Some(entry) = state.entries.remove(id) {
            state.order.remove(&entry.tick);
            state.bytes -= entry.bytes;
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        // A panic while holding the lock leaves consistent counters; recover.
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(n: u32) -> Option<Value> {
        Some(Arc::new(n))
    }
    fn hit(cache: &DocumentCache, id: &str) -> Option<Option<u32>> {
        match cache.lookup(id) {
            Lookup::Hit(v) => Some(v.map(|v| *v.downcast::<u32>().unwrap())),
            Lookup::Miss { .. } => None,
        }
    }
    fn fill(cache: &DocumentCache, id: &str, n: u32, bytes: usize) {
        let Lookup::Miss { generation } = cache.lookup(id) else { panic!("expected miss") };
        cache.insert(id, value(n), bytes, generation);
    }

    #[test]
    fn a_read_racing_an_invalidation_is_never_cached() {
        let cache = DocumentCache::new(CachePolicy { max_items: Some(10), ..Default::default() });
        let Lookup::Miss { generation } = cache.lookup("a") else { panic!() };
        // A write commits and invalidates while the read was in flight.
        cache.invalidate("a");
        cache.insert("a", value(1), 1, generation);
        assert_eq!(hit(&cache, "a"), None, "the pre-commit value must not be cached");
        fill(&cache, "a", 2, 1);
        assert_eq!(hit(&cache, "a"), Some(Some(2)));
    }

    #[test]
    fn lru_count_and_byte_bounds_evict_the_least_recently_used() {
        let cache = DocumentCache::new(CachePolicy {
            max_items: Some(2),
            max_bytes: Some(10),
            ..Default::default()
        });
        fill(&cache, "a", 1, 4);
        fill(&cache, "b", 2, 4);
        assert!(hit(&cache, "a").is_some()); // a is now more recent than b
        fill(&cache, "c", 3, 4);
        assert_eq!(hit(&cache, "b"), None);
        assert!(hit(&cache, "a").is_some() && hit(&cache, "c").is_some());
        fill(&cache, "d", 4, 9); // over the byte budget: evicts until it fits
        assert_eq!(cache.stats().items, 1);
        assert_eq!(cache.stats().bytes, 9);
        let big = cache.lookup("e");
        let Lookup::Miss { generation } = big else { panic!() };
        cache.insert("e", value(5), 11, generation); // larger than the budget: skipped
        assert_eq!(hit(&cache, "e"), None);
    }

    #[test]
    fn ttl_negative_entries_and_disabling() {
        let cache = DocumentCache::new(CachePolicy {
            ttl: Some(Duration::from_millis(30)),
            ..Default::default()
        });
        fill(&cache, "a", 1, 1);
        let Lookup::Miss { generation } = cache.lookup("gone") else { panic!() };
        cache.insert("gone", None, 1, generation);
        assert_eq!(hit(&cache, "gone"), Some(None), "cached not-found");
        assert_eq!(hit(&cache, "a"), Some(Some(1)));
        std::thread::sleep(Duration::from_millis(40));
        assert_eq!(hit(&cache, "a"), None, "expired");
        fill(&cache, "a", 1, 1);
        cache.set_enabled(false);
        assert_eq!(hit(&cache, "a"), None);
        let Lookup::Miss { generation } = cache.lookup("a") else { panic!() };
        cache.insert("a", value(1), 1, generation);
        assert_eq!(hit(&cache, "a"), None, "a disabled cache refuses inserts");
        cache.set_enabled(true);
        fill(&cache, "a", 1, 1);
        assert_eq!(hit(&cache, "a"), Some(Some(1)));
        let stats = cache.stats();
        assert!(stats.hits >= 3 && stats.misses >= 4, "{stats:?}");
    }
}
