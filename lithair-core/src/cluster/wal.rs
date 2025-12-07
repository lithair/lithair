//! Write-Ahead Log (WAL) for durable consensus operations
//!
//! This module provides a persistent log that ensures durability of operations
//! before they are replicated. Uses rkyv for zero-copy serialization.
//!
//! ## Group Commit
//!
//! The WAL supports group commit for high throughput. Instead of fsync per operation,
//! entries are buffered and flushed together, amortizing the fsync cost.
//!
//! - `append_buffered()` - Add to buffer, returns immediately
//! - `flush()` - Force flush buffer to disk with single fsync
//! - Background task flushes every `group_commit_interval_ms` or when buffer is full

use rkyv::{Archive, Deserialize, Serialize, rancor::Error as RkyvError};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, Notify};

use super::consensus_log::{CrudOperation, LogEntry, LogId};

/// WAL entry header - fixed size for easy seeking
const WAL_HEADER_SIZE: usize = 16; // 8 bytes length + 8 bytes checksum

/// WAL file magic number
const WAL_MAGIC: [u8; 4] = [b'L', b'W', b'A', b'L']; // Lithair WAL

/// WAL version for compatibility
const WAL_VERSION: u32 = 1;

/// Serializable WAL entry using rkyv
#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[rkyv(compare(PartialEq), derive(Debug))]
pub struct WalEntry {
    pub term: u64,
    pub index: u64,
    pub timestamp_ms: u64,
    pub operation: WalOperation,
}

/// WAL operation types (mirrors CrudOperation but with rkyv derives)
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[rkyv(compare(PartialEq), derive(Debug))]
pub enum WalOperation {
    Create {
        model_path: String,
        data: String, // JSON string for simplicity with rkyv
    },
    Update {
        model_path: String,
        id: String,
        data: String,
    },
    Delete {
        model_path: String,
        id: String,
    },
    /// Migration operations stored as JSON for flexibility with rkyv
    Migration {
        migration_type: String, // "begin", "step", "commit", "rollback"
        payload: String,        // JSON-serialized migration data
    },
}

impl From<&CrudOperation> for WalOperation {
    fn from(op: &CrudOperation) -> Self {
        match op {
            CrudOperation::Create { model_path, data } => WalOperation::Create {
                model_path: model_path.clone(),
                data: data.to_string(),
            },
            CrudOperation::Update { model_path, id, data } => WalOperation::Update {
                model_path: model_path.clone(),
                id: id.clone(),
                data: data.to_string(),
            },
            CrudOperation::Delete { model_path, id } => WalOperation::Delete {
                model_path: model_path.clone(),
                id: id.clone(),
            },
            CrudOperation::MigrationBegin {
                from_version,
                to_version,
                migration_id,
            } => WalOperation::Migration {
                migration_type: "begin".to_string(),
                payload: serde_json::json!({
                    "from_version": from_version,
                    "to_version": to_version,
                    "migration_id": migration_id.to_string(),
                })
                .to_string(),
            },
            CrudOperation::MigrationStep {
                migration_id,
                step_index,
                operation,
            } => WalOperation::Migration {
                migration_type: "step".to_string(),
                payload: serde_json::json!({
                    "migration_id": migration_id.to_string(),
                    "step_index": step_index,
                    "operation": operation,
                })
                .to_string(),
            },
            CrudOperation::MigrationCommit {
                migration_id,
                checksum,
            } => WalOperation::Migration {
                migration_type: "commit".to_string(),
                payload: serde_json::json!({
                    "migration_id": migration_id.to_string(),
                    "checksum": checksum,
                })
                .to_string(),
            },
            CrudOperation::MigrationRollback {
                migration_id,
                failed_step,
                reason,
            } => WalOperation::Migration {
                migration_type: "rollback".to_string(),
                payload: serde_json::json!({
                    "migration_id": migration_id.to_string(),
                    "failed_step": failed_step,
                    "reason": reason,
                })
                .to_string(),
            },
        }
    }
}

impl WalOperation {
    pub fn to_crud_operation(&self) -> CrudOperation {
        match self {
            WalOperation::Create { model_path, data } => CrudOperation::Create {
                model_path: model_path.clone(),
                data: serde_json::from_str(data).unwrap_or(serde_json::Value::Null),
            },
            WalOperation::Update { model_path, id, data } => CrudOperation::Update {
                model_path: model_path.clone(),
                id: id.clone(),
                data: serde_json::from_str(data).unwrap_or(serde_json::Value::Null),
            },
            WalOperation::Delete { model_path, id } => CrudOperation::Delete {
                model_path: model_path.clone(),
                id: id.clone(),
            },
            WalOperation::Migration {
                migration_type,
                payload,
            } => {
                let json: serde_json::Value =
                    serde_json::from_str(payload).unwrap_or(serde_json::Value::Null);

                match migration_type.as_str() {
                    "begin" => CrudOperation::MigrationBegin {
                        from_version: serde_json::from_value(json["from_version"].clone())
                            .unwrap_or_default(),
                        to_version: serde_json::from_value(json["to_version"].clone())
                            .unwrap_or_default(),
                        migration_id: json["migration_id"]
                            .as_str()
                            .and_then(|s| uuid::Uuid::parse_str(s).ok())
                            .unwrap_or_else(uuid::Uuid::nil),
                    },
                    "step" => CrudOperation::MigrationStep {
                        migration_id: json["migration_id"]
                            .as_str()
                            .and_then(|s| uuid::Uuid::parse_str(s).ok())
                            .unwrap_or_else(uuid::Uuid::nil),
                        step_index: json["step_index"].as_u64().unwrap_or(0) as u32,
                        operation: serde_json::from_value(json["operation"].clone()).unwrap_or(
                            super::upgrade::SchemaChange::Custom {
                                description: "parse_error".to_string(),
                                forward: String::new(),
                                backward: String::new(),
                            },
                        ),
                    },
                    "commit" => CrudOperation::MigrationCommit {
                        migration_id: json["migration_id"]
                            .as_str()
                            .and_then(|s| uuid::Uuid::parse_str(s).ok())
                            .unwrap_or_else(uuid::Uuid::nil),
                        checksum: json["checksum"].as_str().unwrap_or("").to_string(),
                    },
                    "rollback" => CrudOperation::MigrationRollback {
                        migration_id: json["migration_id"]
                            .as_str()
                            .and_then(|s| uuid::Uuid::parse_str(s).ok())
                            .unwrap_or_else(uuid::Uuid::nil),
                        failed_step: json["failed_step"].as_u64().unwrap_or(0) as u32,
                        reason: json["reason"].as_str().unwrap_or("").to_string(),
                    },
                    _ => CrudOperation::MigrationRollback {
                        migration_id: uuid::Uuid::nil(),
                        failed_step: 0,
                        reason: format!("Unknown migration type: {}", migration_type),
                    },
                }
            }
        }
    }
}

impl WalEntry {
    pub fn from_log_entry(entry: &LogEntry) -> Self {
        Self {
            term: entry.log_id.term,
            index: entry.log_id.index,
            timestamp_ms: entry.timestamp_ms,
            operation: WalOperation::from(&entry.operation),
        }
    }

    pub fn to_log_entry(&self) -> LogEntry {
        LogEntry {
            log_id: LogId::new(self.term, self.index),
            operation: self.operation.to_crud_operation(),
            timestamp_ms: self.timestamp_ms,
        }
    }
}

/// Group commit configuration
#[derive(Debug, Clone)]
pub struct GroupCommitConfig {
    /// Maximum time to wait before flushing (default: 5ms)
    pub flush_interval_ms: u64,
    /// Maximum entries to buffer before forcing flush (default: 100)
    pub max_buffer_size: usize,
    /// Enable group commit (default: true)
    pub enabled: bool,
}

impl Default for GroupCommitConfig {
    fn default() -> Self {
        Self {
            flush_interval_ms: 5,
            max_buffer_size: 100,
            enabled: true,
        }
    }
}

/// Pending WAL entry waiting for flush
struct PendingEntry {
    entry: LogEntry,
    /// Notification to signal when flushed
    notify: std::sync::Arc<Notify>,
}

/// Write-Ahead Log manager with group commit support
pub struct WriteAheadLog {
    /// Path to the WAL file
    path: PathBuf,
    /// Current file position for appending
    write_position: AtomicU64,
    /// Last synced index
    last_synced_index: AtomicU64,
    /// Last buffered (not yet synced) index
    last_buffered_index: AtomicU64,
    /// Write lock for exclusive append access
    write_lock: RwLock<()>,
    /// Pending entries buffer for group commit
    pending_buffer: RwLock<Vec<PendingEntry>>,
    /// Group commit configuration
    group_commit_config: GroupCommitConfig,
    /// Last flush time
    last_flush_time: RwLock<Instant>,
    /// Shutdown flag
    shutdown: AtomicBool,
    /// Notify for flush requests
    flush_notify: Notify,
}

impl WriteAheadLog {
    /// Create or open a WAL at the given path
    pub fn new<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        Self::with_config(path, GroupCommitConfig::default())
    }

    /// Create WAL with custom group commit configuration
    pub fn with_config<P: AsRef<Path>>(path: P, config: GroupCommitConfig) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();

        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let (write_position, last_synced_index) = if path.exists() {
            // Open existing WAL and find the end
            let file = File::open(&path)?;
            let metadata = file.metadata()?;
            let size = metadata.len();

            // Find the last valid entry
            let last_index = Self::find_last_index(&path)?;
            (size, last_index)
        } else {
            // Create new WAL with header
            let mut file = File::create(&path)?;
            file.write_all(&WAL_MAGIC)?;
            file.write_all(&WAL_VERSION.to_le_bytes())?;
            file.sync_all()?;
            (8, 0) // Magic + version = 8 bytes
        };

        Ok(Self {
            path,
            write_position: AtomicU64::new(write_position),
            last_synced_index: AtomicU64::new(last_synced_index),
            last_buffered_index: AtomicU64::new(last_synced_index),
            write_lock: RwLock::new(()),
            pending_buffer: RwLock::new(Vec::new()),
            group_commit_config: config,
            last_flush_time: RwLock::new(Instant::now()),
            shutdown: AtomicBool::new(false),
            flush_notify: Notify::new(),
        })
    }

    /// Append entry with group commit (buffered, waits for flush)
    ///
    /// This is the recommended method for high-throughput scenarios.
    /// The entry is buffered and will be flushed either:
    /// - When the buffer reaches `max_buffer_size`
    /// - After `flush_interval_ms` milliseconds
    /// - When `flush()` is called explicitly
    ///
    /// Returns when the entry has been durably written to disk.
    pub async fn append_buffered(&self, entry: &LogEntry) -> std::io::Result<()> {
        // If group commit is disabled, fall back to immediate append
        if !self.group_commit_config.enabled {
            return self.append(entry).await;
        }

        let notify = std::sync::Arc::new(Notify::new());
        let should_flush;

        {
            let mut buffer = self.pending_buffer.write().await;
            buffer.push(PendingEntry {
                entry: entry.clone(),
                notify: notify.clone(),
            });

            // Update last buffered index
            self.last_buffered_index.store(entry.log_id.index, Ordering::SeqCst);

            // Check if we should trigger immediate flush
            should_flush = buffer.len() >= self.group_commit_config.max_buffer_size;
        }

        if should_flush {
            // Trigger flush immediately
            self.flush_notify.notify_one();
        }

        // Wait for our entry to be flushed
        notify.notified().await;

        Ok(())
    }

    /// Flush all buffered entries to disk with a single fsync
    ///
    /// This is called automatically by the background flush task,
    /// but can also be called manually for immediate durability.
    pub async fn flush(&self) -> std::io::Result<usize> {
        let entries_to_flush: Vec<PendingEntry>;

        {
            let mut buffer = self.pending_buffer.write().await;
            if buffer.is_empty() {
                return Ok(0);
            }
            entries_to_flush = std::mem::take(&mut *buffer);
        }

        let count = entries_to_flush.len();

        // Extract just the entries for batch write
        let entries: Vec<LogEntry> = entries_to_flush.iter().map(|p| p.entry.clone()).collect();

        // Perform batch write with single fsync
        self.append_batch(&entries).await?;

        // Update flush time
        *self.last_flush_time.write().await = Instant::now();

        // Notify all waiters that their entries are now durable
        for pending in entries_to_flush {
            pending.notify.notify_one();
        }

        Ok(count)
    }

    /// Check if flush is needed based on time or buffer size
    pub async fn should_flush(&self) -> bool {
        let buffer = self.pending_buffer.read().await;
        if buffer.is_empty() {
            return false;
        }

        // Flush if buffer is full
        if buffer.len() >= self.group_commit_config.max_buffer_size {
            return true;
        }

        // Flush if interval has passed
        let last_flush = self.last_flush_time.read().await;
        last_flush.elapsed() >= Duration::from_millis(self.group_commit_config.flush_interval_ms)
    }

    /// Get pending buffer size
    pub async fn pending_count(&self) -> usize {
        self.pending_buffer.read().await.len()
    }

    /// Spawn background flush task
    ///
    /// Returns a handle that can be used to stop the task.
    /// The task will flush pending entries every `flush_interval_ms`.
    pub fn spawn_flush_task(self: &std::sync::Arc<Self>) -> tokio::task::JoinHandle<()> {
        let wal = std::sync::Arc::clone(self);
        let interval = Duration::from_millis(wal.group_commit_config.flush_interval_ms);

        tokio::spawn(async move {
            loop {
                // Wait for either:
                // 1. Flush interval timeout
                // 2. Explicit flush request (buffer full)
                // 3. Shutdown signal
                tokio::select! {
                    _ = tokio::time::sleep(interval) => {}
                    _ = wal.flush_notify.notified() => {}
                }

                if wal.shutdown.load(Ordering::SeqCst) {
                    // Final flush before shutdown
                    let _ = wal.flush().await;
                    break;
                }

                if let Err(e) = wal.flush().await {
                    log::error!("WAL flush error: {}", e);
                }
            }
        })
    }

    /// Signal shutdown and flush remaining entries
    pub async fn shutdown(&self) -> std::io::Result<()> {
        self.shutdown.store(true, Ordering::SeqCst);
        self.flush_notify.notify_one();
        // Final flush
        self.flush().await?;
        Ok(())
    }

    /// Append an entry to the WAL (with fsync for durability)
    pub async fn append(&self, entry: &LogEntry) -> std::io::Result<()> {
        let _guard = self.write_lock.write().await;

        let wal_entry = WalEntry::from_log_entry(entry);

        // Serialize with rkyv
        let bytes = rkyv::to_bytes::<RkyvError>(&wal_entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        // Calculate checksum (simple FNV-1a for speed)
        let checksum = Self::fnv1a_hash(&bytes);

        // Open file for append
        let mut file = OpenOptions::new()
            .write(true)
            .append(true)
            .open(&self.path)?;

        // Write header: length (8 bytes) + checksum (8 bytes)
        file.write_all(&(bytes.len() as u64).to_le_bytes())?;
        file.write_all(&checksum.to_le_bytes())?;

        // Write data
        file.write_all(&bytes)?;

        // Fsync for durability
        file.sync_all()?;

        // Update position
        let new_pos = self.write_position.load(Ordering::SeqCst)
            + WAL_HEADER_SIZE as u64
            + bytes.len() as u64;
        self.write_position.store(new_pos, Ordering::SeqCst);
        self.last_synced_index.store(entry.log_id.index, Ordering::SeqCst);

        Ok(())
    }

    /// Append multiple entries in a single fsync (batch write)
    pub async fn append_batch(&self, entries: &[LogEntry]) -> std::io::Result<()> {
        if entries.is_empty() {
            return Ok(());
        }

        let _guard = self.write_lock.write().await;

        let mut file = OpenOptions::new()
            .write(true)
            .append(true)
            .open(&self.path)?;

        let mut total_written = 0u64;
        let mut last_index = 0u64;

        for entry in entries {
            let wal_entry = WalEntry::from_log_entry(entry);
            let bytes = rkyv::to_bytes::<RkyvError>(&wal_entry)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

            let checksum = Self::fnv1a_hash(&bytes);

            file.write_all(&(bytes.len() as u64).to_le_bytes())?;
            file.write_all(&checksum.to_le_bytes())?;
            file.write_all(&bytes)?;

            total_written += WAL_HEADER_SIZE as u64 + bytes.len() as u64;
            last_index = entry.log_id.index;
        }

        // Single fsync for entire batch
        file.sync_all()?;

        let new_pos = self.write_position.load(Ordering::SeqCst) + total_written;
        self.write_position.store(new_pos, Ordering::SeqCst);
        self.last_synced_index.store(last_index, Ordering::SeqCst);

        Ok(())
    }

    /// Read all entries from the WAL (for recovery)
    pub fn read_all(&self) -> std::io::Result<Vec<LogEntry>> {
        let file = File::open(&self.path)?;
        let mut reader = BufReader::new(file);

        // Skip header
        let mut header = [0u8; 8];
        reader.read_exact(&mut header)?;

        // Verify magic
        if header[0..4] != WAL_MAGIC {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid WAL magic number",
            ));
        }

        let mut entries = Vec::new();

        loop {
            // Try to read entry header
            let mut len_buf = [0u8; 8];
            match reader.read_exact(&mut len_buf) {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e),
            }

            let len = u64::from_le_bytes(len_buf) as usize;

            let mut checksum_buf = [0u8; 8];
            reader.read_exact(&mut checksum_buf)?;
            let stored_checksum = u64::from_le_bytes(checksum_buf);

            // Read data
            let mut data = vec![0u8; len];
            reader.read_exact(&mut data)?;

            // Verify checksum
            let computed_checksum = Self::fnv1a_hash(&data);
            if stored_checksum != computed_checksum {
                // Corrupted entry, stop here
                break;
            }

            // Deserialize with rkyv
            match rkyv::from_bytes::<WalEntry, RkyvError>(&data) {
                Ok(wal_entry) => {
                    entries.push(wal_entry.to_log_entry());
                }
                Err(_) => {
                    // Corrupted entry, stop here
                    break;
                }
            }
        }

        Ok(entries)
    }

    /// Read entries from a specific index
    pub fn read_from(&self, from_index: u64) -> std::io::Result<Vec<LogEntry>> {
        let all_entries = self.read_all()?;
        Ok(all_entries
            .into_iter()
            .filter(|e| e.log_id.index >= from_index)
            .collect())
    }

    /// Truncate WAL after a specific index (for rollback)
    pub async fn truncate_after(&self, index: u64) -> std::io::Result<()> {
        let _guard = self.write_lock.write().await;

        // Read all entries
        let entries = self.read_all()?;

        // Filter entries to keep
        let entries_to_keep: Vec<_> = entries
            .into_iter()
            .filter(|e| e.log_id.index <= index)
            .collect();

        // Rewrite WAL
        let mut file = File::create(&self.path)?;
        file.write_all(&WAL_MAGIC)?;
        file.write_all(&WAL_VERSION.to_le_bytes())?;

        let mut position = 8u64;

        for entry in &entries_to_keep {
            let wal_entry = WalEntry::from_log_entry(entry);
            let bytes = rkyv::to_bytes::<RkyvError>(&wal_entry)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

            let checksum = Self::fnv1a_hash(&bytes);

            file.write_all(&(bytes.len() as u64).to_le_bytes())?;
            file.write_all(&checksum.to_le_bytes())?;
            file.write_all(&bytes)?;

            position += WAL_HEADER_SIZE as u64 + bytes.len() as u64;
        }

        file.sync_all()?;

        self.write_position.store(position, Ordering::SeqCst);
        self.last_synced_index.store(
            entries_to_keep.last().map(|e| e.log_id.index).unwrap_or(0),
            Ordering::SeqCst,
        );

        Ok(())
    }

    /// Get the last synced index
    pub fn last_index(&self) -> u64 {
        self.last_synced_index.load(Ordering::SeqCst)
    }

    /// FNV-1a hash for checksum (fast and good distribution)
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

    /// Find the last valid index in an existing WAL
    fn find_last_index(path: &Path) -> std::io::Result<u64> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);

        // Skip header
        reader.seek(SeekFrom::Start(8))?;

        let mut last_index = 0u64;

        loop {
            let mut len_buf = [0u8; 8];
            match reader.read_exact(&mut len_buf) {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e),
            }

            let len = u64::from_le_bytes(len_buf) as usize;

            // Skip checksum
            reader.seek(SeekFrom::Current(8))?;

            // Read data
            let mut data = vec![0u8; len];
            match reader.read_exact(&mut data) {
                Ok(_) => {}
                Err(_) => break,
            }

            // Try to deserialize to get index
            if let Ok(wal_entry) = rkyv::from_bytes::<WalEntry, RkyvError>(&data) {
                last_index = wal_entry.index;
            }
        }

        Ok(last_index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_wal_append_and_read() {
        let dir = tempdir().unwrap();
        let wal_path = dir.path().join("test.wal");

        let wal = WriteAheadLog::new(&wal_path).unwrap();

        let entry = LogEntry {
            log_id: LogId::new(1, 1),
            operation: CrudOperation::Create {
                model_path: "/api/products".to_string(),
                data: serde_json::json!({"name": "Test"}),
            },
            timestamp_ms: 12345,
        };

        wal.append(&entry).await.unwrap();

        let entries = wal.read_all().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].log_id.index, 1);
    }

    #[tokio::test]
    async fn test_wal_batch_append() {
        let dir = tempdir().unwrap();
        let wal_path = dir.path().join("test_batch.wal");

        let wal = WriteAheadLog::new(&wal_path).unwrap();

        let entries: Vec<LogEntry> = (1..=10)
            .map(|i| LogEntry {
                log_id: LogId::new(1, i),
                operation: CrudOperation::Create {
                    model_path: "/api/products".to_string(),
                    data: serde_json::json!({"id": i}),
                },
                timestamp_ms: i * 1000,
            })
            .collect();

        wal.append_batch(&entries).await.unwrap();

        let read_entries = wal.read_all().unwrap();
        assert_eq!(read_entries.len(), 10);
    }

    #[tokio::test]
    async fn test_wal_recovery() {
        let dir = tempdir().unwrap();
        let wal_path = dir.path().join("test_recovery.wal");

        // Write some entries
        {
            let wal = WriteAheadLog::new(&wal_path).unwrap();
            for i in 1..=5 {
                let entry = LogEntry {
                    log_id: LogId::new(1, i),
                    operation: CrudOperation::Create {
                        model_path: "/api/test".to_string(),
                        data: serde_json::json!({"id": i}),
                    },
                    timestamp_ms: i * 100,
                };
                wal.append(&entry).await.unwrap();
            }
        }

        // Reopen and verify
        {
            let wal = WriteAheadLog::new(&wal_path).unwrap();
            let entries = wal.read_all().unwrap();
            assert_eq!(entries.len(), 5);
            assert_eq!(wal.last_index(), 5);
        }
    }

    #[tokio::test]
    async fn test_wal_truncate() {
        let dir = tempdir().unwrap();
        let wal_path = dir.path().join("test_truncate.wal");

        let wal = WriteAheadLog::new(&wal_path).unwrap();

        // Write 10 entries
        let entries: Vec<LogEntry> = (1..=10)
            .map(|i| LogEntry {
                log_id: LogId::new(1, i),
                operation: CrudOperation::Create {
                    model_path: "/api/test".to_string(),
                    data: serde_json::json!({"id": i}),
                },
                timestamp_ms: i * 100,
            })
            .collect();

        wal.append_batch(&entries).await.unwrap();

        // Truncate after index 5
        wal.truncate_after(5).await.unwrap();

        let read_entries = wal.read_all().unwrap();
        assert_eq!(read_entries.len(), 5);
        assert_eq!(read_entries.last().unwrap().log_id.index, 5);
    }

    #[tokio::test]
    async fn test_group_commit() {
        let dir = tempdir().unwrap();
        let wal_path = dir.path().join("test_group_commit.wal");

        let config = GroupCommitConfig {
            flush_interval_ms: 100, // 100ms for testing
            max_buffer_size: 5,
            enabled: true,
        };

        let wal = std::sync::Arc::new(WriteAheadLog::with_config(&wal_path, config).unwrap());

        // Spawn background flush task
        let _flush_handle = wal.spawn_flush_task();

        // Spawn multiple concurrent writes
        let mut handles = Vec::new();
        for i in 1..=10u64 {
            let wal = std::sync::Arc::clone(&wal);
            handles.push(tokio::spawn(async move {
                let entry = LogEntry {
                    log_id: LogId::new(1, i),
                    operation: CrudOperation::Create {
                        model_path: "/api/test".to_string(),
                        data: serde_json::json!({"id": i}),
                    },
                    timestamp_ms: i * 100,
                };
                wal.append_buffered(&entry).await.unwrap();
            }));
        }

        // Wait for all writes to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Shutdown WAL to ensure final flush
        wal.shutdown().await.unwrap();

        // Verify all entries were written
        let entries = wal.read_all().unwrap();
        assert_eq!(entries.len(), 10);
    }

    #[tokio::test]
    async fn test_group_commit_immediate_flush_on_full_buffer() {
        let dir = tempdir().unwrap();
        let wal_path = dir.path().join("test_group_commit_full.wal");

        let config = GroupCommitConfig {
            flush_interval_ms: 10000, // Very long to ensure buffer-full triggers flush
            max_buffer_size: 3,
            enabled: true,
        };

        let wal = std::sync::Arc::new(WriteAheadLog::with_config(&wal_path, config).unwrap());
        let _flush_handle = wal.spawn_flush_task();

        // Write exactly max_buffer_size entries
        let mut handles = Vec::new();
        for i in 1..=3u64 {
            let wal = std::sync::Arc::clone(&wal);
            handles.push(tokio::spawn(async move {
                let entry = LogEntry {
                    log_id: LogId::new(1, i),
                    operation: CrudOperation::Create {
                        model_path: "/api/test".to_string(),
                        data: serde_json::json!({"id": i}),
                    },
                    timestamp_ms: i * 100,
                };
                wal.append_buffered(&entry).await.unwrap();
            }));
        }

        // Wait for writes - should complete quickly due to buffer full trigger
        let start = std::time::Instant::now();
        for handle in handles {
            handle.await.unwrap();
        }
        let elapsed = start.elapsed();

        // Should complete much faster than flush_interval_ms (10s)
        assert!(elapsed.as_millis() < 1000, "Flush took too long: {:?}", elapsed);

        wal.shutdown().await.unwrap();

        let entries = wal.read_all().unwrap();
        assert_eq!(entries.len(), 3);
    }
}
