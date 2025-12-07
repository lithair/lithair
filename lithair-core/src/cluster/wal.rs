//! Write-Ahead Log (WAL) for durable consensus operations
//!
//! This module provides a persistent log that ensures durability of operations
//! before they are replicated. Uses rkyv for zero-copy serialization.

use rkyv::{Archive, Deserialize, Serialize, rancor::Error as RkyvError};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::RwLock;

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

/// Write-Ahead Log manager
pub struct WriteAheadLog {
    /// Path to the WAL file
    path: PathBuf,
    /// Current file position for appending
    write_position: AtomicU64,
    /// Last synced index
    last_synced_index: AtomicU64,
    /// Write lock for exclusive append access
    write_lock: RwLock<()>,
}

impl WriteAheadLog {
    /// Create or open a WAL at the given path
    pub fn new<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
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
            write_lock: RwLock::new(()),
        })
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
}
