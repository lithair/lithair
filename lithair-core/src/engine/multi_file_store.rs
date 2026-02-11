//! Multi-file event store dispatcher
//!
//! Routes events to separate files based on aggregate_id.
//!
//! Architecture:
//! ```text
//! MultiFileEventStore
//! ├── aggregate_id: "block_target_hosts" → data/block_target_hosts/events.raftlog
//! ├── aggregate_id: "block_source_ips"   → data/block_source_ips/events.raftlog
//! └── aggregate_id: null                 → data/global/events.raftlog
//! ```

use super::snapshot::{RecoveryContext, Snapshot, SnapshotStore, DEFAULT_SNAPSHOT_THRESHOLD};
use super::{EngineError, EngineResult, EventStore};
use crate::engine::events::EventEnvelope;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Multi-file event store that dispatches to different files based on aggregate_id
pub struct MultiFileEventStore {
    /// Base directory for all event stores (e.g., "./data/rustgate")
    base_dir: PathBuf,

    /// Map of aggregate_id → EventStore instance
    /// Each aggregate gets its own EventStore and file
    stores: HashMap<String, EventStore>,

    /// Default store for events without aggregate_id
    global_store: EventStore,

    /// Snapshot store for managing snapshots
    snapshot_store: SnapshotStore,

    /// Event counts per aggregate (for snapshot threshold tracking)
    event_counts: HashMap<String, usize>,

    /// Global event count
    global_event_count: usize,

    /// Snapshot threshold (create snapshot after N events)
    snapshot_threshold: usize,

    /// Verbose logging flag
    log_verbose: bool,
}

impl MultiFileEventStore {
    /// Create a new multi-file event store
    ///
    /// - `base_dir`: Base directory (e.g., "./data/rustgate")
    /// - All aggregate-specific stores will be in subdirectories
    pub fn new(base_dir: impl AsRef<Path>) -> EngineResult<Self> {
        let base_dir = base_dir.as_ref().to_path_buf();

        // Ensure base directory exists
        std::fs::create_dir_all(&base_dir).map_err(|e| {
            EngineError::PersistenceError(format!("Failed to create base dir: {}", e))
        })?;

        // Create global store for events without aggregate_id
        let global_dir = base_dir.join("global");
        std::fs::create_dir_all(&global_dir).map_err(|e| {
            EngineError::PersistenceError(format!("Failed to create global dir: {}", e))
        })?;

        // IMPORTANT: EventStore::new expects a *directory* as base_path when in single-file mode.
        // It will internally create `events.raftlog` inside this directory. Passing a path
        // ending with "events.raftlog" would cause FileStorage to create a nested
        // `events.raftlog/` directory and then `events.raftlog` file inside it.
        let global_store = EventStore::new(global_dir.to_str().ok_or_else(|| {
            EngineError::PersistenceError("global dir path contains invalid UTF-8".to_string())
        })?)?;

        // Create snapshot store
        let snapshot_store = SnapshotStore::new(base_dir.to_str().ok_or_else(|| {
            EngineError::PersistenceError("base dir path contains invalid UTF-8".to_string())
        })?)?;

        Ok(Self {
            base_dir,
            stores: HashMap::new(),
            global_store,
            snapshot_store,
            event_counts: HashMap::new(),
            global_event_count: 0,
            snapshot_threshold: DEFAULT_SNAPSHOT_THRESHOLD,
            log_verbose: false,
        })
    }

    /// Create with custom snapshot threshold
    pub fn with_snapshot_threshold(
        base_dir: impl AsRef<Path>,
        threshold: usize,
    ) -> EngineResult<Self> {
        let mut store = Self::new(base_dir)?;
        store.snapshot_threshold = threshold;
        store.snapshot_store.set_threshold(threshold);
        Ok(store)
    }

    /// Get or create an EventStore for the given aggregate_id
    ///
    /// Creates a subdirectory and EventStore on-demand if not exists.
    fn get_store_mut(&mut self, aggregate_id: &str) -> EngineResult<&mut EventStore> {
        // Check if store exists
        if !self.stores.contains_key(aggregate_id) {
            // Create new store
            self.create_store(aggregate_id)?;
        }

        Ok(self.stores.get_mut(aggregate_id).unwrap())
    }

    /// Helper to create a new store
    fn create_store(&mut self, aggregate_id: &str) -> EngineResult<()> {
        // Double-check
        if self.stores.contains_key(aggregate_id) {
            return Ok(());
        }

        // Create aggregate directory
        let aggregate_dir = self.base_dir.join(aggregate_id);
        std::fs::create_dir_all(&aggregate_dir).map_err(|e| {
            EngineError::PersistenceError(format!(
                "Failed to create aggregate dir {}: {}",
                aggregate_id, e
            ))
        })?;

        // Create EventStore for this aggregate.
        // As with the global store, we pass the aggregate directory as base_path;
        // FileStorage will then create `<aggregate_dir>/events.raftlog` internally.
        let store = EventStore::new(aggregate_dir.to_str().ok_or_else(|| {
            EngineError::PersistenceError(format!(
                "aggregate dir path for '{}' contains invalid UTF-8",
                aggregate_id
            ))
        })?)?;

        if self.log_verbose {
            log::debug!(
                "Created EventStore for aggregate '{}' at {}",
                aggregate_id,
                aggregate_dir.display()
            );
        }

        self.stores.insert(aggregate_id.to_string(), store);
        Ok(())
    }

    /// Append an event envelope to the appropriate store
    ///
    /// Routes based on aggregate_id:
    /// - Some(id) → data/{id}/events.raftlog
    /// - None     → data/global/events.raftlog
    pub fn append_envelope(&mut self, envelope: &EventEnvelope) -> EngineResult<()> {
        match &envelope.aggregate_id {
            Some(aggregate_id) => {
                let store = self.get_store_mut(aggregate_id)?;
                store.append_envelope(envelope)?;
                self.increment_event_count(Some(aggregate_id));
            }
            None => {
                self.global_store.append_envelope(envelope)?;
                self.increment_event_count(None);
            }
        }
        Ok(())
    }

    /// Helper to parse JSON strings to EventEnvelopes
    fn parse_envelopes(json_events: Vec<String>) -> EngineResult<Vec<EventEnvelope>> {
        json_events
            .into_iter()
            .map(|json| {
                serde_json::from_str(&json)
                    .map_err(|e| EngineError::SerializationError(e.to_string()))
            })
            .collect()
    }

    /// Read all envelopes from all stores
    ///
    /// Returns events from all aggregate-specific stores + global store.
    pub fn read_all_envelopes(&self) -> EngineResult<Vec<EventEnvelope>> {
        let mut all_envelopes = Vec::new();

        // Read from global store
        let global_json = self.global_store.get_all_events()?;
        all_envelopes.extend(Self::parse_envelopes(global_json)?);

        // Read from all aggregate stores
        for store in self.stores.values() {
            let store_json = store.get_all_events()?;
            all_envelopes.extend(Self::parse_envelopes(store_json)?);
        }

        Ok(all_envelopes)
    }

    /// Read envelopes for a specific aggregate_id
    ///
    /// Returns only events from that aggregate's store.
    pub fn read_aggregate_envelopes(&self, aggregate_id: &str) -> EngineResult<Vec<EventEnvelope>> {
        if let Some(store) = self.stores.get(aggregate_id) {
            let json_events = store.get_all_events()?;
            Self::parse_envelopes(json_events)
        } else {
            // No events for this aggregate yet
            Ok(Vec::new())
        }
    }

    /// Flush all stores to disk
    pub fn flush_all(&mut self) -> EngineResult<()> {
        // Flush global store
        self.global_store.flush_events()?;

        // Flush all aggregate stores
        for store in self.stores.values_mut() {
            store.flush_events()?;
        }

        Ok(())
    }

    /// Enable verbose logging
    pub fn set_verbose(&mut self, verbose: bool) {
        self.log_verbose = verbose;
        self.global_store.log_verbose = verbose;

        for store in self.stores.values_mut() {
            store.log_verbose = verbose;
        }
    }

    /// Get list of all aggregate_ids with event stores
    pub fn list_aggregates(&self) -> Vec<String> {
        self.stores.keys().cloned().collect()
    }

    /// Get base directory path
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Append a deduplication event_id to global store
    ///
    /// Deduplication is handled at the global level, not per-aggregate.
    /// This method delegates to the global store's dedup mechanism.
    pub fn append_dedup_id(&self, event_id: &str) -> EngineResult<()> {
        self.global_store.save_dedup_id(event_id)
    }

    /// Load all persisted deduplication event ids from global store
    pub fn load_dedup_ids(&self) -> EngineResult<Vec<String>> {
        self.global_store.load_dedup_ids()
    }

    // ==================== SNAPSHOT METHODS ====================

    /// Set the snapshot threshold
    pub fn set_snapshot_threshold(&mut self, threshold: usize) {
        self.snapshot_threshold = threshold;
        self.snapshot_store.set_threshold(threshold);
    }

    /// Get the current snapshot threshold
    pub fn snapshot_threshold(&self) -> usize {
        self.snapshot_threshold
    }

    /// Save a snapshot for an aggregate
    ///
    /// # Arguments
    /// - `aggregate_id`: The aggregate to snapshot (None for global)
    /// - `state`: The serialized state to save
    /// - `last_event_id`: Optional ID of the last event included
    pub fn save_snapshot(
        &mut self,
        aggregate_id: Option<&str>,
        state: String,
        last_event_id: Option<String>,
    ) -> EngineResult<()> {
        let event_count = match aggregate_id {
            Some(id) => *self.event_counts.get(id).unwrap_or(&0),
            None => self.global_event_count,
        };

        let snapshot =
            Snapshot::new(aggregate_id.map(|s| s.to_string()), event_count, last_event_id, state);

        self.snapshot_store.save_snapshot(&snapshot)?;

        if self.log_verbose {
            log::debug!("Snapshot created for {:?}: {} events", aggregate_id, event_count);
        }

        Ok(())
    }

    /// Load a snapshot for an aggregate
    pub fn load_snapshot(&self, aggregate_id: Option<&str>) -> EngineResult<Option<Snapshot>> {
        self.snapshot_store.load_snapshot(aggregate_id)
    }

    /// Get event count for an aggregate
    pub fn get_event_count(&self, aggregate_id: Option<&str>) -> usize {
        match aggregate_id {
            Some(id) => *self.event_counts.get(id).unwrap_or(&0),
            None => self.global_event_count,
        }
    }

    /// Check if a snapshot should be created for an aggregate
    pub fn should_create_snapshot(&self, aggregate_id: Option<&str>) -> EngineResult<bool> {
        let current_count = self.get_event_count(aggregate_id);
        let snapshot_count = self
            .snapshot_store
            .load_snapshot(aggregate_id)?
            .map(|s| s.metadata.event_count)
            .unwrap_or(0);

        Ok(self.snapshot_store.should_create_snapshot(current_count, snapshot_count))
    }

    /// Create a recovery context for loading state
    ///
    /// Returns the snapshot (if any) and the number of events to skip
    pub fn recovery_context(&self, aggregate_id: Option<&str>) -> EngineResult<RecoveryContext> {
        RecoveryContext::new(&self.snapshot_store, aggregate_id)
    }

    /// Read events after snapshot for recovery
    ///
    /// Returns only events that are not included in the snapshot
    pub fn read_events_after_snapshot(
        &self,
        aggregate_id: Option<&str>,
    ) -> EngineResult<Vec<EventEnvelope>> {
        let ctx = self.recovery_context(aggregate_id)?;
        let all_events = match aggregate_id {
            Some(id) => self.read_aggregate_envelopes(id)?,
            None => {
                let global_json = self.global_store.get_all_events()?;
                Self::parse_envelopes(global_json)?
            }
        };

        // Skip events already in snapshot
        Ok(all_events.into_iter().skip(ctx.events_to_skip).collect())
    }

    /// Get snapshot statistics for an aggregate
    pub fn get_snapshot_stats(
        &self,
        aggregate_id: Option<&str>,
    ) -> EngineResult<Option<super::snapshot::SnapshotStats>> {
        self.snapshot_store.get_stats(aggregate_id)
    }

    /// List all aggregates with snapshots
    pub fn list_snapshots(&self) -> EngineResult<Vec<Option<String>>> {
        self.snapshot_store.list_snapshots()
    }

    /// Delete a snapshot
    pub fn delete_snapshot(&self, aggregate_id: Option<&str>) -> EngineResult<()> {
        self.snapshot_store.delete_snapshot(aggregate_id)
    }

    /// Increment event count for an aggregate (called after append)
    fn increment_event_count(&mut self, aggregate_id: Option<&str>) {
        match aggregate_id {
            Some(id) => {
                *self.event_counts.entry(id.to_string()).or_insert(0) += 1;
            }
            None => {
                self.global_event_count += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_multi_file_store_creation() {
        let temp_dir = tempdir().unwrap();
        let store = MultiFileEventStore::new(temp_dir.path()).unwrap();

        assert_eq!(store.base_dir(), temp_dir.path());
        assert_eq!(store.list_aggregates().len(), 0);
    }

    #[test]
    fn test_aggregate_routing() {
        let temp_dir = tempdir().unwrap();
        let mut store = MultiFileEventStore::new(temp_dir.path()).unwrap();

        // Create events for different aggregates
        let envelope1 = EventEnvelope {
            event_type: "TestEvent".to_string(),
            event_id: "test1".to_string(),
            timestamp: 1234567890,
            payload: "{}".to_string(),
            aggregate_id: Some("category_a".to_string()),
            event_hash: None,
            previous_hash: None,
        };

        let envelope2 = EventEnvelope {
            event_type: "TestEvent".to_string(),
            event_id: "test2".to_string(),
            timestamp: 1234567891,
            payload: "{}".to_string(),
            aggregate_id: Some("category_b".to_string()),
            event_hash: None,
            previous_hash: None,
        };

        let envelope3 = EventEnvelope {
            event_type: "TestEvent".to_string(),
            event_id: "test3".to_string(),
            timestamp: 1234567892,
            payload: "{}".to_string(),
            aggregate_id: None, // Global
            event_hash: None,
            previous_hash: None,
        };

        // Append events
        store.append_envelope(&envelope1).unwrap();
        store.append_envelope(&envelope2).unwrap();
        store.append_envelope(&envelope3).unwrap();

        store.flush_all().unwrap();

        // Verify separate files created
        assert!(temp_dir.path().join("category_a/events.raftlog").exists());
        assert!(temp_dir.path().join("category_b/events.raftlog").exists());
        assert!(temp_dir.path().join("global/events.raftlog").exists());

        // Verify aggregate list
        let aggregates = store.list_aggregates();
        assert_eq!(aggregates.len(), 2);
        assert!(aggregates.contains(&"category_a".to_string()));
        assert!(aggregates.contains(&"category_b".to_string()));
    }

    #[test]
    fn test_read_aggregate_specific() {
        let temp_dir = tempdir().unwrap();
        let mut store = MultiFileEventStore::new(temp_dir.path()).unwrap();

        // Add events to category_a
        for i in 0..5 {
            let envelope = EventEnvelope {
                event_type: "TestEvent".to_string(),
                event_id: format!("test_{}", i),
                timestamp: 1234567890 + i,
                payload: "{}".to_string(),
                aggregate_id: Some("category_a".to_string()),
                event_hash: None,
                previous_hash: None,
            };
            store.append_envelope(&envelope).unwrap();
        }

        store.flush_all().unwrap();

        // Read only category_a events
        let events = store.read_aggregate_envelopes("category_a").unwrap();
        assert_eq!(events.len(), 5);

        // Reading non-existent aggregate returns empty
        let events = store.read_aggregate_envelopes("category_c").unwrap();
        assert_eq!(events.len(), 0);
    }
}
