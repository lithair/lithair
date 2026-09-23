use crate::engine::events::EventStore;
use crate::engine::retention::RetentionLayer;
use crate::engine::{AsyncWriter, Event, EventEnvelope};
use crate::lifecycle::RetentionConfig;
use crate::model::ModelSpec;
use crate::model_inspect::Inspectable;
use scc::HashMap as SccHashMap;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Version metadata for a resident record (not a compare-and-swap API).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionedEntry<S> {
    pub version: u64,
    pub last_updated: u64,
    pub data: S,
}

/// Secondary Index Structure
/// Maps index_key -> List of Record IDs (Primary Keys)
#[derive(Debug)]
pub struct SecondaryIndex {
    pub field_name: String,
    pub index_map: SccHashMap<String, Vec<String>>,
}

impl SecondaryIndex {
    pub fn new(field_name: String) -> Self {
        Self { field_name, index_map: SccHashMap::new() }
    }
}

/// Configuration for SCC2 Engine
#[derive(Debug, Clone)]
pub struct Scc2EngineConfig {
    pub verbose_logging: bool,
    pub enable_snapshots: bool,
    pub snapshot_interval: u64,
    pub enable_deduplication: bool,
    pub auto_persist_writes: bool,
    /// Await the journal flush before publishing the prepared state. Otherwise
    /// success acknowledges queue admission; call `flush` before relying on durability.
    pub force_immediate_persistence: bool,
}

/// Concurrent in-memory reads with one ordered mutation path per engine.
/// SCC protects buckets; the mutation gate coordinates state, indexes and journal.
#[derive(Clone)]
pub struct Scc2Engine<S> {
    state_map: Arc<SccHashMap<String, VersionedEntry<S>>>,
    indexes: Arc<SccHashMap<String, SecondaryIndex>>,
    event_store: Arc<std::sync::RwLock<EventStore>>,
    async_writer: Arc<AsyncWriter>,
    config: Scc2EngineConfig,
    stats: Arc<Scc2EngineStats>,
    retention: Option<Arc<RetentionLayer>>,
    mutations: Arc<Mutex<()>>,
}

#[derive(Debug, Default)]
pub struct Scc2EngineStats {
    pub reads: AtomicU64,
    pub writes: AtomicU64,
    pub conflicts: AtomicU64,
}

impl<S> Scc2Engine<S>
where
    S: Clone + Send + Sync + Default + Serialize + 'static + Inspectable + ModelSpec,
{
    pub fn new(
        event_store: Arc<std::sync::RwLock<EventStore>>,
        config: Scc2EngineConfig,
    ) -> Result<Self, crate::Error> {
        let async_writer = AsyncWriter::new(event_store.clone(), 1000);

        Ok(Self {
            state_map: Arc::new(SccHashMap::new()),
            indexes: Arc::new(SccHashMap::new()),
            event_store,
            async_writer: Arc::new(async_writer),
            config,
            stats: Arc::new(Scc2EngineStats::default()),
            retention: None,
            mutations: Arc::new(Mutex::new(())),
        })
    }

    /// Enable retention policy on this engine.
    /// Once enabled, the engine evicts oldest items beyond the memory limit,
    /// keeping only pinned fields in a warm map for fast listing.
    pub fn enable_retention(&mut self, config: RetentionConfig, pinned_field_names: Vec<String>) {
        // Gate on ANY configured dimension (count, duration or budget) — a
        // budget-only or duration-only config must activate retention too
        // (issue #121).
        if config.is_configured() {
            self.retention = Some(Arc::new(RetentionLayer::new(config, pinned_field_names)));
        }
    }

    /// Check if retention is active on this engine.
    pub fn has_retention(&self) -> bool {
        self.retention.as_ref().is_some_and(|r| r.is_active())
    }

    /// Get the retention layer (if active).
    pub fn retention_layer(&self) -> Option<&RetentionLayer> {
        self.retention.as_deref()
    }

    /// After a write to the hot map, check if we need to evict. May evict
    /// multiple keys in a single call (budget mode can drop several small
    /// old items to make room for a new large one).
    fn maybe_evict(&self, inserted_key: &str) {
        let retention = match &self.retention {
            Some(r) if r.is_active() => r,
            _ => return,
        };

        // Compute size + timestamp of the just-inserted item. Size = serialized
        // JSON byte count via a zero-alloc counting writer (no temporary Vec).
        struct ByteCounter(usize);
        impl std::io::Write for ByteCounter {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0 += buf.len();
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let (inserted_last_updated, inserted_size) = self
            .state_map
            .read_sync(inserted_key, |_, entry| {
                let mut counter = ByteCounter(0);
                let size = serde_json::to_writer(&mut counter, &entry.data)
                    .map(|_| counter.0)
                    .unwrap_or(0);
                (entry.last_updated, size)
            })
            .unwrap_or((0, 0));

        let to_evict = retention.track_insert(inserted_key, inserted_last_updated, inserted_size);

        for evict_key in to_evict {
            if let Some(o) = self.state_map.get_sync(&evict_key) {
                let entry = o.get();
                retention.evict_to_warm(&evict_key, &entry.data, entry.version, entry.last_updated);
                let _ = o.remove();

                if self.config.verbose_logging {
                    log::debug!("SCC2 retention: evicted '{}' to warm map", evict_key);
                }
            }
        }
    }

    /// Read a full item, checking warm map + event store replay if evicted.
    /// Returns None if the key doesn't exist anywhere.
    pub fn read_or_load<R, F, E>(&self, key: &str, f: F) -> Option<R>
    where
        F: Fn(&S) -> R,
        E: Event<State = S> + DeserializeOwned,
    {
        // Try hot map first
        if let Some(result) = self.read(key, &f) {
            return Some(result);
        }

        // If retention is active, check warm map and replay from event store
        let retention = self.retention.as_ref()?;
        if !retention.is_evicted(key) {
            return None;
        }

        // Replay events for this specific aggregate to reconstruct full state
        let events = {
            let store = self.event_store.read().expect("event store lock poisoned");
            store.get_all_events().ok()?
        };

        let mut state = S::default();
        let mut found = false;
        for event_json in events {
            if let Ok(envelope) = serde_json::from_str::<serde_json::Value>(&event_json) {
                let aggregate_id =
                    envelope.get("aggregate_id").and_then(|v| v.as_str()).unwrap_or("global");
                if aggregate_id != key {
                    continue;
                }
                let payload_str = if let Some(p) = envelope.get("payload").and_then(|v| v.as_str())
                {
                    p.to_string()
                } else {
                    event_json.clone()
                };
                if let Ok(event) = serde_json::from_str::<E>(&payload_str) {
                    event.apply(&mut state);
                    found = true;
                }
            }
        }

        if found {
            Some(f(&state))
        } else {
            None
        }
    }

    /// List all items: hot (full) + warm (pinned fields only).
    /// Returns JSON values — hot items serialized fully, warm items as pinned-only.
    pub fn list_all_with_warm(&self) -> Vec<(String, serde_json::Value)> {
        let mut result = Vec::new();

        // Hot items (full data)
        self.state_map.retain_sync(|key, entry| {
            if let Ok(val) = serde_json::to_value(&entry.data) {
                result.push((key.clone(), val));
            }
            true
        });

        // Warm items (pinned fields only)
        if let Some(retention) = &self.retention {
            for (key, warm_entry) in retention.all_warm_entries() {
                result.push((key, warm_entry.pinned_data));
            }
        }

        result
    }

    /// Total item count (hot + warm).
    pub fn total_count(&self) -> usize {
        let hot = self.state_map.len();
        let warm = self.retention.as_ref().map_or(0, |r| r.warm_count());
        hot + warm
    }

    pub fn replay_events<E>(&self) -> Result<(), crate::Error>
    where
        E: Event<State = S> + DeserializeOwned,
    {
        let start = std::time::Instant::now();
        let events = {
            let store = self.event_store.read().expect("event store lock poisoned");
            store.get_all_events().map_err(|e| crate::Error::EngineError(e.to_string()))?
        };

        let mut count = 0;
        for event_json in events {
            if let Ok(envelope) = serde_json::from_str::<serde_json::Value>(&event_json) {
                let aggregate_id =
                    envelope.get("aggregate_id").and_then(|v| v.as_str()).unwrap_or("global");

                let payload_str = if let Some(p) = envelope.get("payload").and_then(|v| v.as_str())
                {
                    p.to_string()
                } else {
                    event_json.clone()
                };

                if let Ok(event) = serde_json::from_str::<E>(&payload_str) {
                    self.update_entry_volatile(aggregate_id, |state| {
                        event.apply(state);
                    });
                    count += 1;
                }
            }
        }

        if self.config.verbose_logging {
            log::info!("SCC2: Replayed {} events in {:?}", count, start.elapsed());
        }

        Ok(())
    }

    pub fn read<R, F>(&self, key: &str, f: F) -> Option<R>
    where
        F: FnOnce(&S) -> R,
    {
        self.stats.reads.fetch_add(1, Ordering::Relaxed);
        self.state_map.read_sync(key, |_, entry| f(&entry.data))
    }

    /// Internal escape hatch: direct mutations bypass journal/index coordination.
    pub fn internal_map(&self) -> &SccHashMap<String, VersionedEntry<S>> {
        &self.state_map
    }

    pub fn event_store(&self) -> Arc<std::sync::RwLock<EventStore>> {
        self.event_store.clone()
    }

    pub fn async_writer(&self) -> Arc<AsyncWriter> {
        self.async_writer.clone()
    }

    pub fn update_entry_volatile<F, R>(&self, key: &str, f: F) -> Option<R>
    where
        F: FnOnce(&mut S) -> R,
    {
        let _mutation = self.mutations.lock().ok()?;
        let mut state =
            self.state_map.read_sync(key, |_, entry| entry.data.clone()).unwrap_or_default();
        let result = f(&mut state);
        self.publish(key, state);
        Some(result)
    }

    /// Caller holds the mutation gate. Never reacquire a bucket while its entry
    /// guard is live: retention may read or evict the very item just published.
    fn publish(&self, key: &str, state: S) {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        match self.state_map.entry_sync(key.to_string()) {
            scc::hash_map::Entry::Occupied(mut entry) => {
                self.update_indexes(key, &entry.get().data, &state);
                let version = entry.get().version + 1;
                *entry.get_mut() = VersionedEntry { version, last_updated: timestamp, data: state };
            }
            scc::hash_map::Entry::Vacant(entry) => {
                if self.retention.as_ref().is_some_and(|r| r.is_evicted(key)) {
                    self.remove_key_from_indexes(key);
                }
                self.add_to_index(key, &state);
                entry.insert_entry(VersionedEntry {
                    version: 1,
                    last_updated: timestamp,
                    data: state,
                });
            }
        }
        self.stats.writes.fetch_add(1, Ordering::Relaxed);
        if let Some(retention) = &self.retention {
            retention.promote_from_warm(key);
        }
        self.maybe_evict(key);
    }

    pub async fn apply_event<E>(
        &self,
        key: String,
        event: E,
        persist: bool,
    ) -> Result<(), crate::Error>
    where
        E: Event<State = S> + Serialize + 'static,
    {
        // Keep mutation waits off the HTTP executor. The journal has its own
        // worker; once admitted, cancellation does not undo a commit.
        let engine = self.clone();
        tokio::task::spawn_blocking(move || {
            let _mutation = engine
                .mutations
                .lock()
                .map_err(|e| crate::Error::EngineError(format!("mutation lock poisoned: {e}")))?;
            let mut next = engine
                .state_map
                .read_sync(&key, |_, entry| entry.data.clone())
                .unwrap_or_default();
            event.apply(&mut next);
            engine.check_uniqueness(&next, &key)?;
            if persist && engine.config.auto_persist_writes {
                let payload = serde_json::to_string(&event)
                    .map_err(|e| crate::Error::SerializationError(e.to_string()))?;
                let envelope = EventEnvelope::new(
                    std::any::type_name::<E>().to_string(),
                    event.idempotence_key().unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    payload,
                    Some(key.clone()),
                    None,
                );
                engine
                    .async_writer
                    .write_envelope(envelope)
                    .map_err(crate::Error::EngineError)?;
                if engine.config.force_immediate_persistence {
                    engine.async_writer.flush_blocking().map_err(crate::Error::EngineError)?;
                }
            }
            engine.publish(&key, next);
            Ok(())
        })
        .await
        .map_err(|e| crate::Error::EngineError(format!("mutation task failed: {e}")))?
    }

    pub async fn flush(&self) -> Result<(), crate::Error> {
        self.async_writer.flush().await.map_err(crate::Error::EngineError)
    }

    pub fn write<F>(&self, key: &str, f: F) -> Result<(), crate::Error>
    where
        F: FnOnce(&mut S),
    {
        if self.update_entry_volatile(key, f).is_some() {
            Ok(())
        } else {
            Err(crate::Error::EngineError("Failed to write state".to_string()))
        }
    }

    pub fn snapshot(&self) -> Result<(), crate::Error> {
        if !self.config.enable_snapshots {
            return Err(crate::Error::EngineError("Snapshots disabled".into()));
        }

        // Collect all state (snapshot)
        // We serialize the whole map: key -> VersionedEntry<S>
        let mut snapshot_map = std::collections::HashMap::new();
        (*self.state_map).retain_sync(|key, v| {
            snapshot_map.insert(key.clone(), v.clone());
            true
        });

        let json = serde_json::to_string(&snapshot_map)
            .map_err(|e| crate::Error::SerializationError(e.to_string()))?;

        // Save to EventStore
        let store = self.event_store.read().expect("event store lock poisoned");
        store.save_snapshot(&json).map_err(|e| crate::Error::EngineError(e.to_string()))
    }

    pub fn truncate_log(&self) -> Result<(), crate::Error> {
        let mut store = self.event_store.write().expect("event store lock poisoned");
        store.truncate_events().map_err(|e| crate::Error::EngineError(e.to_string()))
    }

    fn check_uniqueness(&self, state_clone: &S, key: &str) -> Result<(), crate::Error> {
        for field_name in state_clone.get_all_fields() {
            if let Some(policy) = state_clone.get_policy(&field_name) {
                if policy.unique {
                    if let Some(value) = state_clone.get_field_value(&field_name) {
                        let value_str = match value {
                            serde_json::Value::String(s) => s,
                            _ => value.to_string(),
                        };
                        let ids = self.indexed_values(&field_name, &value_str);
                        for id in ids {
                            if id != key {
                                return Err(crate::Error::EngineError(format!(
                                    "Unique constraint violation: Field '{}' with value '{}' already exists in record '{}'",
                                    field_name, value_str, id
                                )));
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn update_indexes(&self, key: &str, old_state: &S, new_state: &S) {
        for field_name in new_state.get_all_fields() {
            if let Some(policy) = new_state.get_policy(&field_name) {
                if policy.indexed || policy.unique {
                    let old_val = old_state.get_field_value(&field_name);
                    let new_val = new_state.get_field_value(&field_name);

                    if old_val != new_val {
                        if let Some(v) = old_val {
                            let v_str = match v {
                                serde_json::Value::String(s) => s,
                                _ => v.to_string(),
                            };
                            self.remove_from_index_value(&field_name, &v_str, key);
                        }
                        if let Some(v) = new_val {
                            let v_str = match v {
                                serde_json::Value::String(s) => s,
                                _ => v.to_string(),
                            };
                            self.add_to_index_value(&field_name, &v_str, key);
                        }
                    }
                }
            }
        }
    }

    fn add_to_index(&self, key: &str, state: &S) {
        for field_name in state.get_all_fields() {
            if let Some(policy) = state.get_policy(&field_name) {
                if policy.indexed || policy.unique {
                    if let Some(v) = state.get_field_value(&field_name) {
                        let v_str = match v {
                            serde_json::Value::String(s) => s,
                            _ => v.to_string(),
                        };
                        self.add_to_index_value(&field_name, &v_str, key);
                    }
                }
            }
        }
    }

    fn add_to_index_value(&self, field_name: &str, value: &str, key: &str) {
        let index = self
            .indexes
            .entry_sync(field_name.to_string())
            .or_insert_with(|| SecondaryIndex::new(field_name.to_string()));
        let mut ids = index.get().index_map.entry_sync(value.to_string()).or_default();
        if !ids.get().iter().any(|id| id == key) {
            ids.get_mut().push(key.to_string());
        }
    }

    fn remove_from_index_value(&self, field_name: &str, value: &str, key: &str) {
        self.indexes.read_sync(field_name, |_, index| {
            if let Some(mut ids) = index.index_map.get_sync(value) {
                ids.get_mut().retain(|id| id != key);
                if ids.get().is_empty() {
                    let _ = ids.remove();
                }
            }
        });
    }

    fn remove_key_from_indexes(&self, key: &str) {
        self.indexes.retain_sync(|_, index| {
            index.index_map.retain_sync(|_, ids| {
                ids.retain(|id| id != key);
                !ids.is_empty()
            });
            true
        });
    }

    pub fn create_index(&self, field_name: &str) {
        let _mutation = self.mutations.lock().expect("mutation lock poisoned");
        self.indexes
            .entry_sync(field_name.to_string())
            .or_insert_with(|| SecondaryIndex::new(field_name.to_string()));
    }

    pub fn get_indexed_values(&self, field_name: &str, value: &str) -> Vec<String> {
        let _mutation = self.mutations.lock().expect("mutation lock poisoned");
        self.indexed_values(field_name, value)
    }

    fn indexed_values(&self, field_name: &str, value: &str) -> Vec<String> {
        self.indexes
            .read_sync(field_name, |_, index| {
                index.index_map.read_sync(value, |_, ids| ids.clone()).unwrap_or_default()
            })
            .unwrap_or_default()
    }

    /// Helper for tests: Insert/Update value
    pub fn insert_sync(&self, key: String, value: S) {
        self.update_entry_volatile(&key, |state| *state = value)
            .expect("mutation lock poisoned");
    }

    pub async fn insert(&self, key: String, value: S) {
        self.insert_sync(key, value);
    }

    /// Helper for tests: Remove value
    pub fn remove_sync(&self, key: &str) {
        let _mutation = self.mutations.lock().expect("mutation lock poisoned");
        if let Some((_, entry)) = self.state_map.remove_sync(key) {
            for field in entry.data.get_all_fields() {
                if let Some(value) = entry.data.get_field_value(&field) {
                    let value = match value {
                        serde_json::Value::String(s) => s,
                        value => value.to_string(),
                    };
                    self.remove_from_index_value(&field, &value, key);
                }
            }
        } else if self.retention.as_ref().is_some_and(|r| r.is_evicted(key)) {
            // Full field values are no longer resident. Remove the key from
            // in-memory indexes without reading the journal on this path.
            self.remove_key_from_indexes(key);
        }
        if let Some(retention) = &self.retention {
            retention.remove(key);
        }
    }

    pub async fn remove(&self, key: &str) {
        self.remove_sync(key);
    }

    /// Helper for tests: Clear all values
    pub fn clear_sync(&self) {
        let _mutation = self.mutations.lock().expect("mutation lock poisoned");
        (*self.state_map).retain_sync(|_, _| false);
        (*self.indexes).retain_sync(|_, _| false);
        if let Some(retention) = &self.retention {
            retention.clear();
        }
    }

    pub async fn clear(&self) {
        self.clear_sync();
    }

    /// Helper for tests: Iterate all values synchronously
    pub fn iter_all_sync(&self) -> Vec<(String, S)> {
        let mut result = Vec::new();
        (*self.state_map).retain_sync(|key, v| {
            result.push((key.clone(), v.data.clone()));
            true // Keep all items
        });
        result
    }

    /// Helper for tests: Iterate all values
    pub async fn iter_all(&self) -> Vec<(String, S)> {
        self.iter_all_sync()
    }
}

// Implement DataSource for Scc2Engine to support Auto-Joiner
impl<S> crate::engine::DataSource for Scc2Engine<S>
where
    S: Clone + Send + Sync + Default + Serialize + 'static + Inspectable + ModelSpec,
{
    fn fetch_by_id(&self, id: &str) -> Option<serde_json::Value> {
        self.read(id, |state| serde_json::to_value(state).ok()).flatten()
    }
}
