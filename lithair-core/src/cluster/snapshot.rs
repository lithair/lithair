//! Snapshot and Resync Protocol
//!
//! This module provides snapshot creation and transfer for resyncing
//! desynced followers. When a follower is too far behind (>1000 ops or >5s),
//! it's more efficient to send a full snapshot than replay all missing ops.

use rkyv::{rancor::Error as RkyvError, Archive, Deserialize, Serialize};
use serde::{Deserialize as SerdeDeserialize, Serialize as SerdeSerialize};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

/// Snapshot metadata
#[derive(Debug, Clone, SerdeSerialize, SerdeDeserialize)]
pub struct SnapshotMeta {
    /// Term at time of snapshot
    pub term: u64,
    /// Last included log index
    pub last_included_index: u64,
    /// Timestamp when snapshot was created
    pub created_at_ms: u64,
    /// Size of snapshot data in bytes
    pub size_bytes: u64,
    /// Checksum of snapshot data
    pub checksum: u64,
}

/// Snapshot data using rkyv for efficient serialization
#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[rkyv(derive(Debug))]
pub struct SnapshotData {
    /// Model data as JSON strings keyed by model_path
    /// e.g., "/api/products" -> "[{\"id\":1,...}, {\"id\":2,...}]"
    pub models: Vec<(String, String)>, // Use Vec of tuples instead of HashMap for rkyv compatibility
}

impl SnapshotData {
    pub fn new() -> Self {
        Self { models: Vec::new() }
    }

    /// Add model data to snapshot
    pub fn add_model(&mut self, model_path: &str, data: &[serde_json::Value]) {
        let json = serde_json::to_string(data).unwrap_or_else(|_| "[]".to_string());
        // Remove existing entry if present
        self.models.retain(|(path, _)| path != model_path);
        self.models.push((model_path.to_string(), json));
    }

    /// Get model data from snapshot
    pub fn get_model(&self, model_path: &str) -> Vec<serde_json::Value> {
        self.models
            .iter()
            .find(|(path, _)| path == model_path)
            .and_then(|(_, json)| serde_json::from_str(json).ok())
            .unwrap_or_default()
    }
}

impl Default for SnapshotData {
    fn default() -> Self {
        Self::new()
    }
}

/// Snapshot manager handles creation, storage, and transfer of snapshots
pub struct SnapshotManager {
    /// Directory for storing snapshots
    snapshot_dir: PathBuf,
    /// Current snapshot metadata
    current_meta: Option<SnapshotMeta>,
}

impl SnapshotManager {
    /// Create a new snapshot manager
    pub fn new<P: AsRef<Path>>(snapshot_dir: P) -> std::io::Result<Self> {
        let snapshot_dir = snapshot_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&snapshot_dir)?;

        // Try to load existing snapshot metadata
        let current_meta = Self::load_latest_meta(&snapshot_dir)?;

        Ok(Self { snapshot_dir, current_meta })
    }

    /// Create a new snapshot from current state
    pub fn create_snapshot(
        &mut self,
        term: u64,
        last_index: u64,
        data: SnapshotData,
    ) -> std::io::Result<SnapshotMeta> {
        // Serialize with rkyv
        let bytes = rkyv::to_bytes::<RkyvError>(&data)
            .map_err(|e| std::io::Error::other(e.to_string()))?;

        let checksum = Self::fnv1a_hash(&bytes);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let meta = SnapshotMeta {
            term,
            last_included_index: last_index,
            created_at_ms: timestamp,
            size_bytes: bytes.len() as u64,
            checksum,
        };

        // Write snapshot data
        let data_path = self.snapshot_dir.join(format!("snapshot_{}.data", last_index));
        let mut file = BufWriter::new(File::create(&data_path)?);
        file.write_all(&bytes)?;
        file.flush()?;

        // Write metadata
        let meta_path = self.snapshot_dir.join(format!("snapshot_{}.meta", last_index));
        let meta_json = serde_json::to_string_pretty(&meta)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        std::fs::write(&meta_path, meta_json)?;

        // Update current pointer
        let current_path = self.snapshot_dir.join("current");
        std::fs::write(&current_path, last_index.to_string())?;

        // Clean up old snapshots (keep last 3)
        self.cleanup_old_snapshots(3)?;

        self.current_meta = Some(meta.clone());
        Ok(meta)
    }

    /// Load snapshot data
    pub fn load_snapshot(&self, index: u64) -> std::io::Result<SnapshotData> {
        let data_path = self.snapshot_dir.join(format!("snapshot_{}.data", index));
        let file = File::open(&data_path)?;
        let mut reader = BufReader::new(file);
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;

        rkyv::from_bytes::<SnapshotData, RkyvError>(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))
    }

    /// Load current snapshot data
    pub fn load_current(&self) -> std::io::Result<Option<(SnapshotMeta, SnapshotData)>> {
        match &self.current_meta {
            Some(meta) => {
                let data = self.load_snapshot(meta.last_included_index)?;
                Ok(Some((meta.clone(), data)))
            }
            None => Ok(None),
        }
    }

    /// Get current snapshot metadata
    pub fn current_meta(&self) -> Option<&SnapshotMeta> {
        self.current_meta.as_ref()
    }

    /// Get raw snapshot bytes for transfer
    pub fn get_snapshot_bytes(&self, index: u64) -> std::io::Result<Vec<u8>> {
        let data_path = self.snapshot_dir.join(format!("snapshot_{}.data", index));
        std::fs::read(data_path)
    }

    /// Install snapshot from received bytes
    pub fn install_snapshot(
        &mut self,
        meta: SnapshotMeta,
        bytes: &[u8],
    ) -> std::io::Result<SnapshotData> {
        // Verify checksum
        let checksum = Self::fnv1a_hash(bytes);
        if checksum != meta.checksum {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Snapshot checksum mismatch: expected {}, got {}", meta.checksum, checksum),
            ));
        }

        // Parse data with rkyv zero-copy deserialization
        let data = rkyv::from_bytes::<SnapshotData, RkyvError>(bytes).map_err(|e| {
            log::error!("Snapshot deserialization failed: {:?}, bytes_len={}", e, bytes.len());
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("rkyv deserialization failed: {:?}", e),
            )
        })?;

        // Write to disk
        let data_path =
            self.snapshot_dir.join(format!("snapshot_{}.data", meta.last_included_index));
        std::fs::write(&data_path, bytes)?;

        let meta_path =
            self.snapshot_dir.join(format!("snapshot_{}.meta", meta.last_included_index));
        let meta_json = serde_json::to_string_pretty(&meta)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        std::fs::write(&meta_path, meta_json)?;

        // Update current pointer
        let current_path = self.snapshot_dir.join("current");
        std::fs::write(&current_path, meta.last_included_index.to_string())?;

        self.current_meta = Some(meta);
        Ok(data)
    }

    /// Load latest metadata from disk
    fn load_latest_meta(snapshot_dir: &Path) -> std::io::Result<Option<SnapshotMeta>> {
        let current_path = snapshot_dir.join("current");
        if !current_path.exists() {
            return Ok(None);
        }

        let index_str = std::fs::read_to_string(&current_path)?;
        let index: u64 = index_str
            .trim()
            .parse()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        let meta_path = snapshot_dir.join(format!("snapshot_{}.meta", index));
        if !meta_path.exists() {
            return Ok(None);
        }

        let meta_json = std::fs::read_to_string(&meta_path)?;
        let meta: SnapshotMeta = serde_json::from_str(&meta_json)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;

        Ok(Some(meta))
    }

    /// Clean up old snapshots, keeping the last N
    fn cleanup_old_snapshots(&self, keep: usize) -> std::io::Result<()> {
        let mut snapshots: Vec<u64> = std::fs::read_dir(&self.snapshot_dir)?
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("snapshot_") && name.ends_with(".data") {
                    name.strip_prefix("snapshot_")?.strip_suffix(".data")?.parse().ok()
                } else {
                    None
                }
            })
            .collect();

        snapshots.sort_by(|a, b| b.cmp(a)); // Sort descending

        // Remove old snapshots
        for index in snapshots.into_iter().skip(keep) {
            let data_path = self.snapshot_dir.join(format!("snapshot_{}.data", index));
            let meta_path = self.snapshot_dir.join(format!("snapshot_{}.meta", index));
            let _ = std::fs::remove_file(data_path);
            let _ = std::fs::remove_file(meta_path);
        }

        Ok(())
    }

    /// FNV-1a hash
    fn fnv1a_hash(data: &[u8]) -> u64 {
        const FNV_OFFSET: u64 = 0xcbf29ce484222325;
        const FNV_PRIME: u64 = 0x100000001b3;

        let mut hash = FNV_OFFSET;
        for byte in data {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        hash
    }
}

/// Request to install a snapshot (sent from leader to desynced follower)
#[derive(Debug, Clone, SerdeSerialize, SerdeDeserialize)]
pub struct InstallSnapshotRequest {
    /// Leader's term
    pub term: u64,
    /// Leader's node ID
    pub leader_id: u64,
    /// Snapshot metadata
    pub meta: SnapshotMeta,
    /// Offset in bytes for chunked transfer (0 for single transfer)
    pub offset: u64,
    /// Snapshot data (may be chunked for large snapshots)
    pub data: Vec<u8>,
    /// True if this is the last chunk
    pub done: bool,
}

/// Response to install snapshot
#[derive(Debug, Clone, SerdeSerialize, SerdeDeserialize)]
pub struct InstallSnapshotResponse {
    /// Current term
    pub term: u64,
    /// True if snapshot was installed successfully
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_snapshot_create_and_load() {
        let dir = tempdir().unwrap();
        let mut manager = SnapshotManager::new(dir.path()).unwrap();

        let mut data = SnapshotData::new();
        data.add_model(
            "/api/products",
            &[
                serde_json::json!({"id": "1", "name": "Product 1"}),
                serde_json::json!({"id": "2", "name": "Product 2"}),
            ],
        );

        let meta = manager.create_snapshot(1, 100, data).unwrap();

        assert_eq!(meta.term, 1);
        assert_eq!(meta.last_included_index, 100);

        // Load it back
        let (loaded_meta, loaded_data) = manager.load_current().unwrap().unwrap();
        assert_eq!(loaded_meta.last_included_index, 100);

        let products = loaded_data.get_model("/api/products");
        assert_eq!(products.len(), 2);
    }

    #[test]
    fn test_snapshot_transfer() {
        let dir1 = tempdir().unwrap();
        let dir2 = tempdir().unwrap();

        // Create snapshot on "leader"
        let mut leader_manager = SnapshotManager::new(dir1.path()).unwrap();
        let mut data = SnapshotData::new();
        data.add_model("/api/items", &[serde_json::json!({"id": "test"})]);
        let meta = leader_manager.create_snapshot(1, 50, data).unwrap();

        // Get bytes for transfer
        let bytes = leader_manager.get_snapshot_bytes(50).unwrap();

        // Install on "follower"
        let mut follower_manager = SnapshotManager::new(dir2.path()).unwrap();
        let installed_data = follower_manager.install_snapshot(meta.clone(), &bytes).unwrap();

        let items = installed_data.get_model("/api/items");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"], "test");
    }

    #[test]
    fn test_snapshot_cleanup() {
        let dir = tempdir().unwrap();
        let mut manager = SnapshotManager::new(dir.path()).unwrap();

        // Create multiple snapshots
        for i in 1..=5 {
            let data = SnapshotData::new();
            manager.create_snapshot(1, i * 100, data).unwrap();
        }

        // Should only have 3 snapshots (keep last 3)
        let data_files: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".data"))
            .collect();

        assert_eq!(data_files.len(), 3);
    }
}
