use crate::engine::events::EventStore;
use crate::engine::{AsyncWriter, Event, WriteEvent};
use crate::model::ModelSpec;
use crate::model_inspect::Inspectable;
use scc::HashMap as SccHashMap;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Versioned entry for Optimistic Concurrency Control (OCC)
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
    pub force_immediate_persistence: bool,
}

/// High-Performance Lock-Free State Engine using SCC (Scalable Concurrent Containers)
pub struct Scc2Engine<S> {
    state_map: Arc<SccHashMap<String, VersionedEntry<S>>>,
    indexes: Arc<SccHashMap<String, SecondaryIndex>>,
    event_store: Arc<std::sync::RwLock<EventStore>>,
    async_writer: Arc<AsyncWriter>,
    config: Scc2EngineConfig,
    stats: Arc<Scc2EngineStats>,
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
        })
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
        // Use try_entry for read access as get/peek seem unavailable or changed
        if let Some(scc::hash_map::Entry::Occupied(o)) =
            (*self.state_map).try_entry(key.to_string())
        {
            Some(f(&o.get().data))
        } else {
            None
        }
    }

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
        // Use try_entry for update. Assuming Option return based on error message.
        match (*self.state_map).try_entry(key.to_string()) {
            Some(entry) => {
                // If Option
                match entry {
                    scc::hash_map::Entry::Occupied(mut o) => {
                        let v: &mut VersionedEntry<S> = o.get_mut();

                        let old_values =
                            if self.has_indexes() { Some(v.data.clone()) } else { None };

                        let result = f(&mut v.data);
                        v.version += 1;
                        v.last_updated = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();

                        if let Some(old) = old_values {
                            self.update_indexes(key, &old, &v.data);
                        }

                        self.stats.writes.fetch_add(1, Ordering::Relaxed);
                        Some(result)
                    }
                    scc::hash_map::Entry::Vacant(v) => {
                        let mut state = S::default();
                        let result = f(&mut state);
                        let entry = VersionedEntry {
                            version: 1,
                            last_updated: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                            data: state.clone(),
                        };

                        if self.has_indexes() {
                            self.add_to_index(key, &state);
                        }

                        v.insert_entry(entry);
                        self.stats.writes.fetch_add(1, Ordering::Relaxed);
                        Some(result)
                    }
                }
            }
            None => None, // If Option
        }
    }

    fn has_indexes(&self) -> bool {
        !self.indexes.is_empty()
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
        self.check_uniqueness(&event, &key)?;

        // Use try_entry synchronously
        match (*self.state_map).try_entry(key.clone()) {
            Some(entry) => match entry {
                scc::hash_map::Entry::Occupied(mut o) => {
                    let v: &mut VersionedEntry<S> = o.get_mut();
                    let old_values = v.data.clone();
                    event.apply(&mut v.data);
                    v.version += 1;
                    v.last_updated = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    self.update_indexes(&key, &old_values, &v.data);
                }
                scc::hash_map::Entry::Vacant(v) => {
                    let mut state = S::default();
                    event.apply(&mut state);
                    let entry = VersionedEntry {
                        version: 1,
                        last_updated: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                        data: state.clone(),
                    };
                    self.add_to_index(&key, &state);
                    v.insert_entry(entry);
                }
            },
            None => return Err(crate::Error::EngineError("Failed to acquire entry lock".into())),
        }

        self.stats.writes.fetch_add(1, Ordering::Relaxed);

        if persist && self.config.auto_persist_writes {
            let event_json = event.to_json();
            let write_event = WriteEvent::Event(event_json);
            self.async_writer.sender().send(write_event).map_err(|e| {
                crate::Error::EngineError(format!("Failed to send event to async writer: {}", e))
            })?;
        }

        Ok(())
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

    fn check_uniqueness<E>(&self, event: &E, key: &str) -> Result<(), crate::Error>
    where
        E: Event<State = S>,
    {
        let mut state_clone = self.read(key, |s| s.clone()).unwrap_or_default();

        event.apply(&mut state_clone);

        for field_name in state_clone.get_all_fields() {
            if let Some(policy) = state_clone.get_policy(&field_name) {
                if policy.unique {
                    if let Some(value) = state_clone.get_field_value(&field_name) {
                        let value_str = match value {
                            serde_json::Value::String(s) => s,
                            _ => value.to_string(),
                        };
                        let ids = self.get_indexed_values(&field_name, &value_str);
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
        if !(*self.indexes).contains_sync(field_name) {
            let _ = (*self.indexes)
                .insert_sync(field_name.to_string(), SecondaryIndex::new(field_name.to_string()));
        }

        // Use try_entry for read access
        if let Some(scc::hash_map::Entry::Occupied(idx_entry)) =
            (*self.indexes).try_entry(field_name.to_string())
        {
            let idx = idx_entry.get();
            // Use try_entry for inner map modification
            match idx.index_map.try_entry(value.to_string()) {
                Some(scc::hash_map::Entry::Occupied(mut o)) => {
                    let list = o.get_mut();
                    if !list.contains(&key.to_string()) {
                        list.push(key.to_string());
                    }
                }
                Some(scc::hash_map::Entry::Vacant(v)) => {
                    v.insert_entry(vec![key.to_string()]);
                }
                None => {}
            }
        }
    }

    fn remove_from_index_value(&self, field_name: &str, value: &str, key: &str) {
        if let Some(scc::hash_map::Entry::Occupied(idx_entry)) =
            (*self.indexes).try_entry(field_name.to_string())
        {
            let idx = idx_entry.get();
            if let Some(scc::hash_map::Entry::Occupied(mut o)) =
                idx.index_map.try_entry(value.to_string())
            {
                let list = o.get_mut();
                if let Some(pos) = list.iter().position(|x| x == key) {
                    list.remove(pos);
                }
                // Optional: remove entry if empty?
                // if list.is_empty() { o.remove(); } // remove() on OccupiedEntry might not be straightforward or exist
            }
        }
    }

    pub fn create_index(&self, field_name: &str) {
        let _ = (*self.indexes)
            .insert_sync(field_name.to_string(), SecondaryIndex::new(field_name.to_string()));
    }

    pub fn get_indexed_values(&self, field_name: &str, value: &str) -> Vec<String> {
        if let Some(scc::hash_map::Entry::Occupied(idx_entry)) =
            (*self.indexes).try_entry(field_name.to_string())
        {
            let idx = idx_entry.get();
            if let Some(scc::hash_map::Entry::Occupied(v_entry)) =
                idx.index_map.try_entry(value.to_string())
            {
                return v_entry.get().clone();
            }
        }
        Vec::new()
    }

    /// Helper for tests: Insert/Update value
    pub fn insert_sync(&self, key: String, value: S) {
        self.update_entry_volatile(&key, |state| *state = value);
    }

    pub async fn insert(&self, key: String, value: S) {
        self.insert_sync(key, value);
    }

    /// Helper for tests: Remove value
    pub fn remove_sync(&self, key: &str) {
        if let Some(scc::hash_map::Entry::Occupied(o)) =
            (*self.state_map).try_entry(key.to_string())
        {
            let _ = o.remove();
        }
    }

    pub async fn remove(&self, key: &str) {
        self.remove_sync(key);
    }

    /// Helper for tests: Clear all values
    pub fn clear_sync(&self) {
        (*self.state_map).retain_sync(|_, _| false);
        (*self.indexes).retain_sync(|_, _| false);
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
