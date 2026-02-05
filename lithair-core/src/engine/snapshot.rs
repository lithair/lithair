//! Snapshot management for fast recovery
//!
//! Snapshots capture the current state of an aggregate to avoid
//! replaying all events from the beginning on startup.
//!
//! # Architecture
//!
//! ```text
//! Without Snapshots:        With Snapshots:
//! ┌─────────────────┐       ┌─────────────────┐
//! │ Startup         │       │ Startup         │
//! │ ↓               │       │ ↓               │
//! │ Read 1M events  │       │ Load snapshot   │
//! │ Apply all       │       │ (10ms)          │
//! │ (30+ seconds)   │       │ ↓               │
//! └─────────────────┘       │ Read 1K events  │
//!                           │ since snapshot  │
//!                           │ (100ms)         │
//!                           └─────────────────┘
//! ```

use super::persistence::{calculate_crc32, format_event_with_crc32, parse_and_validate_event};
use super::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Snapshot format version for future compatibility
pub const SNAPSHOT_VERSION: u32 = 1;

/// Default number of events before creating a snapshot
pub const DEFAULT_SNAPSHOT_THRESHOLD: usize = 10_000;

/// Metadata about a snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMetadata {
    /// Format version for compatibility checks
    pub version: u32,
    /// Aggregate identifier (None for global)
    pub aggregate_id: Option<String>,
    /// Number of events included in this snapshot
    pub event_count: usize,
    /// ID of the last event included in snapshot
    pub last_event_id: Option<String>,
    /// Unix timestamp when snapshot was created
    pub timestamp: u64,
    /// CRC32 checksum of the state data
    pub state_crc32: String,
}

/// A complete snapshot with metadata and state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    /// Snapshot metadata
    pub metadata: SnapshotMetadata,
    /// Serialized state data (JSON string)
    pub state: String,
}

impl Snapshot {
    /// Create a new snapshot
    pub fn new(
        aggregate_id: Option<String>,
        event_count: usize,
        last_event_id: Option<String>,
        state: String,
    ) -> Self {
        let state_crc32 = format!("{:08x}", calculate_crc32(state.as_bytes()));

        Self {
            metadata: SnapshotMetadata {
                version: SNAPSHOT_VERSION,
                aggregate_id,
                event_count,
                last_event_id,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                state_crc32,
            },
            state,
        }
    }

    /// Validate the snapshot's integrity
    pub fn validate(&self) -> Result<(), String> {
        let actual_crc = format!("{:08x}", calculate_crc32(self.state.as_bytes()));
        if actual_crc != self.metadata.state_crc32 {
            return Err(format!(
                "Snapshot CRC32 mismatch: expected {}, got {} - CORRUPTED",
                self.metadata.state_crc32, actual_crc
            ));
        }
        Ok(())
    }

    /// Serialize snapshot to JSON with CRC32 line format
    pub fn to_json(&self) -> EngineResult<String> {
        let json = serde_json::to_string_pretty(self).map_err(|e| {
            EngineError::SerializationError(format!("Failed to serialize snapshot: {}", e))
        })?;
        Ok(json)
    }

    /// Deserialize snapshot from JSON
    pub fn from_json(json: &str) -> EngineResult<Self> {
        let snapshot: Snapshot = serde_json::from_str(json).map_err(|e| {
            EngineError::SerializationError(format!("Failed to deserialize snapshot: {}", e))
        })?;

        // Validate integrity
        snapshot.validate().map_err(|e| {
            EngineError::PersistenceError(format!("Snapshot validation failed: {}", e))
        })?;

        Ok(snapshot)
    }
}

/// Snapshot store for managing snapshots per aggregate
pub struct SnapshotStore {
    /// Base directory for snapshot files
    base_dir: String,
    /// Number of events before auto-snapshot
    snapshot_threshold: usize,
    /// Verbose logging
    log_verbose: bool,
}

impl SnapshotStore {
    /// Create a new snapshot store
    pub fn new(base_dir: &str) -> EngineResult<Self> {
        fs::create_dir_all(base_dir).map_err(|e| {
            EngineError::PersistenceError(format!("Failed to create snapshot dir: {}", e))
        })?;

        Ok(Self {
            base_dir: base_dir.to_string(),
            snapshot_threshold: DEFAULT_SNAPSHOT_THRESHOLD,
            log_verbose: false,
        })
    }

    /// Set the snapshot threshold (events before auto-snapshot)
    pub fn set_threshold(&mut self, threshold: usize) {
        self.snapshot_threshold = threshold;
    }

    /// Enable verbose logging
    pub fn set_verbose(&mut self, verbose: bool) {
        self.log_verbose = verbose;
    }

    /// Get snapshot file path for an aggregate
    fn snapshot_path(&self, aggregate_id: Option<&str>) -> String {
        match aggregate_id {
            Some(id) => format!("{}/{}/snapshot.raftsnap", self.base_dir, id),
            None => format!("{}/global/snapshot.raftsnap", self.base_dir),
        }
    }

    /// Save a snapshot for an aggregate
    pub fn save_snapshot(&self, snapshot: &Snapshot) -> EngineResult<()> {
        let path = self.snapshot_path(snapshot.metadata.aggregate_id.as_deref());

        // Ensure parent directory exists
        if let Some(parent) = Path::new(&path).parent() {
            fs::create_dir_all(parent).map_err(|e| {
                EngineError::PersistenceError(format!("Failed to create snapshot dir: {}", e))
            })?;
        }

        let json = snapshot.to_json()?;

        // Write with CRC32 protection
        let protected_content = format_event_with_crc32(&json);

        fs::write(&path, &protected_content).map_err(|e| {
            EngineError::PersistenceError(format!("Failed to write snapshot: {}", e))
        })?;

        if self.log_verbose {
            println!(
                "📸 Snapshot saved for {:?}: {} events, {} bytes",
                snapshot.metadata.aggregate_id,
                snapshot.metadata.event_count,
                json.len()
            );
        }

        Ok(())
    }

    /// Load a snapshot for an aggregate
    pub fn load_snapshot(&self, aggregate_id: Option<&str>) -> EngineResult<Option<Snapshot>> {
        let path = self.snapshot_path(aggregate_id);

        if !Path::new(&path).exists() {
            if self.log_verbose {
                println!("📂 No snapshot found for {:?}", aggregate_id);
            }
            return Ok(None);
        }

        let content = fs::read_to_string(&path).map_err(|e| {
            EngineError::PersistenceError(format!("Failed to read snapshot: {}", e))
        })?;

        // Validate CRC32 and extract JSON
        let json = parse_and_validate_event(&content).map_err(|e| {
            EngineError::PersistenceError(format!("Snapshot file corrupted: {}", e))
        })?;

        let snapshot = Snapshot::from_json(&json)?;

        if self.log_verbose {
            println!(
                "📂 Loaded snapshot for {:?}: {} events",
                aggregate_id, snapshot.metadata.event_count
            );
        }

        Ok(Some(snapshot))
    }

    /// Delete a snapshot
    pub fn delete_snapshot(&self, aggregate_id: Option<&str>) -> EngineResult<()> {
        let path = self.snapshot_path(aggregate_id);

        if Path::new(&path).exists() {
            fs::remove_file(&path).map_err(|e| {
                EngineError::PersistenceError(format!("Failed to delete snapshot: {}", e))
            })?;

            if self.log_verbose {
                println!("🗑️ Deleted snapshot for {:?}", aggregate_id);
            }
        }

        Ok(())
    }

    /// Check if a snapshot should be created based on event count
    pub fn should_create_snapshot(
        &self,
        current_event_count: usize,
        snapshot_event_count: usize,
    ) -> bool {
        let events_since_snapshot = current_event_count.saturating_sub(snapshot_event_count);
        events_since_snapshot >= self.snapshot_threshold
    }

    /// List all aggregates that have snapshots
    pub fn list_snapshots(&self) -> EngineResult<Vec<Option<String>>> {
        let mut snapshots = Vec::new();

        // Check global snapshot
        let global_path = self.snapshot_path(None);
        if Path::new(&global_path).exists() {
            snapshots.push(None);
        }

        // Check aggregate snapshots
        if let Ok(entries) = fs::read_dir(&self.base_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let dir_name = path.file_name().and_then(|n| n.to_str()).map(|s| s.to_string());

                    if let Some(name) = dir_name {
                        if name != "global" {
                            let snap_path = path.join("snapshot.raftsnap");
                            if snap_path.exists() {
                                snapshots.push(Some(name));
                            }
                        }
                    }
                }
            }
        }

        Ok(snapshots)
    }

    /// Get snapshot statistics
    pub fn get_stats(&self, aggregate_id: Option<&str>) -> EngineResult<Option<SnapshotStats>> {
        let snapshot = self.load_snapshot(aggregate_id)?;

        Ok(snapshot.map(|s| SnapshotStats {
            aggregate_id: s.metadata.aggregate_id,
            event_count: s.metadata.event_count,
            timestamp: s.metadata.timestamp,
            state_size: s.state.len(),
        }))
    }
}

/// Statistics about a snapshot
#[derive(Debug, Clone)]
pub struct SnapshotStats {
    pub aggregate_id: Option<String>,
    pub event_count: usize,
    pub timestamp: u64,
    pub state_size: usize,
}

// ==================== RECOVERY HELPER ====================

/// Recovery context for loading state from snapshot + events
pub struct RecoveryContext {
    /// Loaded snapshot (if any)
    pub snapshot: Option<Snapshot>,
    /// Number of events to skip (already in snapshot)
    pub events_to_skip: usize,
}

impl RecoveryContext {
    /// Create a recovery context by loading snapshot
    pub fn new(snapshot_store: &SnapshotStore, aggregate_id: Option<&str>) -> EngineResult<Self> {
        let snapshot = snapshot_store.load_snapshot(aggregate_id)?;
        let events_to_skip = snapshot.as_ref().map(|s| s.metadata.event_count).unwrap_or(0);

        Ok(Self { snapshot, events_to_skip })
    }

    /// Check if we have a snapshot to restore from
    pub fn has_snapshot(&self) -> bool {
        self.snapshot.is_some()
    }

    /// Get the initial state from snapshot (or None if no snapshot)
    pub fn get_initial_state(&self) -> Option<&str> {
        self.snapshot.as_ref().map(|s| s.state.as_str())
    }
}

// ==================== TESTS ====================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_snapshot_creation_and_validation() {
        let state = r#"{"counter": 100, "items": ["a", "b", "c"]}"#.to_string();
        let snapshot = Snapshot::new(
            Some("articles".to_string()),
            1000,
            Some("event-999".to_string()),
            state.clone(),
        );

        assert_eq!(snapshot.metadata.version, SNAPSHOT_VERSION);
        assert_eq!(snapshot.metadata.aggregate_id, Some("articles".to_string()));
        assert_eq!(snapshot.metadata.event_count, 1000);
        assert_eq!(snapshot.state, state);

        // Validation should pass
        assert!(snapshot.validate().is_ok());
    }

    #[test]
    fn test_snapshot_corruption_detection() {
        let mut snapshot = Snapshot::new(None, 500, None, r#"{"value": 42}"#.to_string());

        // Corrupt the state
        snapshot.state = r#"{"value": 99}"#.to_string();

        // Validation should fail
        let result = snapshot.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("CORRUPTED"));
    }

    #[test]
    fn test_snapshot_store_save_load() {
        let temp_dir = tempdir().unwrap();
        let store = SnapshotStore::new(temp_dir.path().to_str().unwrap()).unwrap();

        let snapshot = Snapshot::new(
            Some("users".to_string()),
            2500,
            Some("user-event-2499".to_string()),
            r#"{"users_count": 100}"#.to_string(),
        );

        // Save snapshot
        store.save_snapshot(&snapshot).unwrap();

        // Load snapshot
        let loaded = store.load_snapshot(Some("users")).unwrap();
        assert!(loaded.is_some());

        let loaded = loaded.unwrap();
        assert_eq!(loaded.metadata.event_count, 2500);
        assert_eq!(loaded.state, r#"{"users_count": 100}"#);
    }

    #[test]
    fn test_snapshot_store_global() {
        let temp_dir = tempdir().unwrap();
        let store = SnapshotStore::new(temp_dir.path().to_str().unwrap()).unwrap();

        let snapshot = Snapshot::new(
            None, // Global
            5000,
            None,
            r#"{"global_state": true}"#.to_string(),
        );

        store.save_snapshot(&snapshot).unwrap();

        let loaded = store.load_snapshot(None).unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().metadata.aggregate_id, None);
    }

    #[test]
    fn test_snapshot_threshold() {
        let temp_dir = tempdir().unwrap();
        let mut store = SnapshotStore::new(temp_dir.path().to_str().unwrap()).unwrap();
        store.set_threshold(1000);

        // 500 events since snapshot of 0 → no snapshot needed
        assert!(!store.should_create_snapshot(500, 0));

        // 1000 events since snapshot → snapshot needed
        assert!(store.should_create_snapshot(1000, 0));

        // 1500 total, snapshot at 1000 → 500 since → no snapshot
        assert!(!store.should_create_snapshot(1500, 1000));

        // 2500 total, snapshot at 1000 → 1500 since → snapshot needed
        assert!(store.should_create_snapshot(2500, 1000));
    }

    #[test]
    fn test_recovery_context() {
        let temp_dir = tempdir().unwrap();
        let store = SnapshotStore::new(temp_dir.path().to_str().unwrap()).unwrap();

        // No snapshot → skip 0 events
        let ctx = RecoveryContext::new(&store, Some("new_aggregate")).unwrap();
        assert!(!ctx.has_snapshot());
        assert_eq!(ctx.events_to_skip, 0);

        // Create snapshot
        let snapshot = Snapshot::new(
            Some("existing".to_string()),
            750,
            None,
            r#"{"data": "test"}"#.to_string(),
        );
        store.save_snapshot(&snapshot).unwrap();

        // With snapshot → skip 750 events
        let ctx = RecoveryContext::new(&store, Some("existing")).unwrap();
        assert!(ctx.has_snapshot());
        assert_eq!(ctx.events_to_skip, 750);
        assert_eq!(ctx.get_initial_state(), Some(r#"{"data": "test"}"#));
    }

    #[test]
    fn test_list_snapshots() {
        let temp_dir = tempdir().unwrap();
        let store = SnapshotStore::new(temp_dir.path().to_str().unwrap()).unwrap();

        // Create several snapshots
        for agg in &["articles", "users", "products"] {
            let snapshot = Snapshot::new(Some(agg.to_string()), 100, None, "{}".to_string());
            store.save_snapshot(&snapshot).unwrap();
        }

        // Create global snapshot
        let global = Snapshot::new(None, 50, None, "{}".to_string());
        store.save_snapshot(&global).unwrap();

        let list = store.list_snapshots().unwrap();
        assert_eq!(list.len(), 4);
        assert!(list.contains(&None)); // Global
        assert!(list.contains(&Some("articles".to_string())));
        assert!(list.contains(&Some("users".to_string())));
        assert!(list.contains(&Some("products".to_string())));
    }

    #[test]
    fn test_snapshot_json_roundtrip() {
        let snapshot = Snapshot::new(
            Some("test".to_string()),
            999,
            Some("last-event".to_string()),
            r#"{"complex": {"nested": [1, 2, 3]}}"#.to_string(),
        );

        let json = snapshot.to_json().unwrap();
        let restored = Snapshot::from_json(&json).unwrap();

        assert_eq!(restored.metadata.aggregate_id, snapshot.metadata.aggregate_id);
        assert_eq!(restored.metadata.event_count, snapshot.metadata.event_count);
        assert_eq!(restored.state, snapshot.state);
    }
}
