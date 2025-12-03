//! File-based persistence for event storage
//!
//! This module implements the Lithair database format for persistent event storage.
//!
//! # Database Format
//!
//! ```text
//! data/
//! ├── events.raftlog     # Append-only event log (JSON lines)
//! ├── state.raftsnap     # Latest state snapshot (JSON)
//! └── meta.raftmeta      # Metadata (version, checksums, etc.)
//! ```

use super::persistence_optimized::{AsyncEventWriter, OptimizedPersistenceConfig};
use super::{EngineError, EngineResult};
use crc32fast::Hasher as Crc32Hasher;
use std::fs;
use std::io::{BufWriter, Write};
use std::path::Path;

// ==================== CRC32 CHECKSUM UTILITIES ====================

/// Calculate CRC32 checksum for data
#[inline]
pub fn calculate_crc32(data: &[u8]) -> u32 {
    let mut hasher = Crc32Hasher::new();
    hasher.update(data);
    hasher.finalize()
}

/// Format event line with CRC32 checksum prefix
/// Format: "<crc32_hex>:<json_data>"
#[inline]
pub fn format_event_with_crc32(event_json: &str) -> String {
    let crc = calculate_crc32(event_json.as_bytes());
    format!("{:08x}:{}", crc, event_json)
}

/// Parse event line and validate CRC32 checksum
/// Returns Ok(json_data) if valid, Err if corrupted
/// Also accepts legacy format (no checksum) for backward compatibility
pub fn parse_and_validate_event(line: &str) -> Result<String, String> {
    // Check if line has CRC32 prefix (9 chars: 8 hex + ':')
    if line.len() > 9 && line.chars().nth(8) == Some(':') {
        let (crc_hex, json_data) = line.split_at(9);
        let crc_hex = &crc_hex[..8]; // Remove ':'

        // Parse expected CRC32
        let expected_crc = u32::from_str_radix(crc_hex, 16)
            .map_err(|_| format!("Invalid CRC32 hex: {}", crc_hex))?;

        // Calculate actual CRC32
        let actual_crc = calculate_crc32(json_data.as_bytes());

        if expected_crc != actual_crc {
            return Err(format!(
                "CRC32 mismatch: expected {:08x}, got {:08x} - DATA CORRUPTED",
                expected_crc, actual_crc
            ));
        }

        Ok(json_data.to_string())
    } else {
        // Legacy format (no checksum) - accept for backward compatibility
        Ok(line.to_string())
    }
}

/// File storage engine for events
///
/// This implements the Lithair database format with:
/// - Append-only event log for durability
/// - State snapshots for fast recovery
/// - Metadata for versioning and integrity
pub struct FileStorage {
    base_path: String,
    events_file: String,
    index_file: String,
    snapshot_file: String,
    metadata_file: String,
    dedup_ids_file: String,
    /// Buffered writer for events to avoid per-event syscalls in benchmarks
    pub(crate) writer: Option<BufWriter<std::fs::File>>,
    /// Buffered writer for binary events (PERFORMANCE FIX)
    pub(crate) binary_writer: Option<BufWriter<std::fs::File>>,
    /// Buffered writer for index entries (PERFORMANCE FIX)
    pub(crate) index_writer: Option<BufWriter<std::fs::File>>,
    /// Control fsync behavior per append
    pub(crate) fsync_on_append: bool,
    /// Rotate events file when exceeding this size (0 = disabled)
    pub(crate) max_log_file_size: usize,
    /// Event batch buffer for high-performance writes
    pub(crate) event_batch: Vec<String>,
    /// Current batch size counter
    pub(crate) batch_count: usize,
    /// Maximum batch size before auto-flush
    pub(crate) max_batch_size: usize,
    /// Optional async writer (optimized path)
    pub(crate) async_writer: Option<AsyncEventWriter>,
    /// Enable CRC32 checksums for data integrity (default: true)
    pub(crate) enable_checksums: bool,
}

impl FileStorage {
    /// Create a new file storage engine
    ///
    /// This will create the directory structure if it doesn't exist
    pub fn new(base_path: &str) -> EngineResult<Self> {
        // Ensure the directory exists
        fs::create_dir_all(base_path).map_err(|e| {
            EngineError::PersistenceError(format!(
                "Failed to create directory {}: {}",
                base_path, e
            ))
        })?;

        let mut storage = Self {
            base_path: base_path.to_string(),
            events_file: format!("{}/events.raftlog", base_path),
            index_file: format!("{}/events.raftidx", base_path),
            snapshot_file: format!("{}/state.raftsnap", base_path),
            metadata_file: format!("{}/meta.raftmeta", base_path),
            dedup_ids_file: format!("{}/dedup.raftids", base_path),
            writer: None,
            binary_writer: None,
            index_writer: None,
            fsync_on_append: true,
            max_log_file_size: 0,
            event_batch: Vec::new(),
            batch_count: 0,
            max_batch_size: 1000, // Batch 1000 events for optimal performance
            async_writer: None,
            enable_checksums: true, // CRC32 checksums enabled by default for data integrity
        };

        // Optional: enable rotation via environment variable (useful for tests/benchmarks)
        if let Ok(v) = std::env::var("RS_MAX_LOG_FILE_SIZE") {
            if let Ok(n) = v.parse::<usize>() {
                storage.max_log_file_size = n;
            }
        }

        // Create metadata file if it doesn't exist
        storage.ensure_metadata_file()?;

        // Optionally enable optimized async persistence via env
        storage.maybe_enable_async_writer();

        // Database initialized silently for performance

        Ok(storage)
    }

    /// Configure le mode fsync pour contrôler la durabilité
    ///
    /// # Arguments
    /// * `enable` - true = fsync après chaque flush (durabilité maximale, plus lent)
    ///   false = pas de fsync (performance maximale, risque perte données)
    pub fn set_fsync(&mut self, enable: bool) {
        self.fsync_on_append = enable;
    }

    /// Enable the optimized async writer if RS_OPT_PERSIST=1 (or "true")
    fn maybe_enable_async_writer(&mut self) {
        let enabled = std::env::var("RS_OPT_PERSIST")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        if !enabled {
            return;
        }

        let mut cfg = OptimizedPersistenceConfig::default();
        if let Ok(s) = std::env::var("RS_BUFFER_SIZE") {
            if let Ok(n) = s.parse::<usize>() {
                cfg.buffer_size = n;
            }
        }
        if let Ok(s) = std::env::var("RS_FLUSH_INTERVAL_MS") {
            if let Ok(n) = s.parse::<u64>() {
                cfg.flush_interval_ms = n;
            }
        }
        if let Ok(s) = std::env::var("RS_MAX_EVENTS_BUFFER") {
            if let Ok(n) = s.parse::<usize>() {
                cfg.max_events_buffer = n;
            }
        }
        cfg.enable_binary_format = false; // Stage A: JSON lines via async writer

        if let Ok(writer) = AsyncEventWriter::new(self.events_file.clone(), cfg) {
            self.async_writer = Some(writer);
            // Disable legacy batching path when async is enabled
            self.event_batch.clear();
            self.batch_count = 0;
            self.writer = None;
            // Note: rotation still based on file size; async writer keeps file open which is fine
        } else {
            eprintln!("⚠️ Failed to enable async writer, falling back to sync FileStorage");
        }
    }

    /// Append an event to the event log (batched for performance)
    ///
    /// Events are stored as JSON lines for human readability and debugging
    pub fn append_event(&mut self, event_json: &str) -> EngineResult<()> {
        // Optimized async path
        if let Some(ref aw) = self.async_writer {
            aw.write_event(event_json.to_string()).map_err(|e| {
                EngineError::PersistenceError(format!("Failed to queue async event: {}", e))
            })?;
            return Ok(());
        }

        // Legacy buffered path
        self.event_batch.push(event_json.to_string());
        self.batch_count += 1;
        if self.batch_count >= self.max_batch_size {
            self.flush_batch()?;
        }
        Ok(())
    }

    /// Append raw binary event bytes (Stage B enablement)
    /// Uses Length-Prefixed Framing (8 bytes length + payload) for robustness against collision.
    pub fn append_binary_event_bytes(&mut self, data: &[u8]) -> EngineResult<()> {
        // Optimized async path
        if let Some(ref aw) = self.async_writer {
            aw.write_binary_event(data.to_vec()).map_err(|e| {
                EngineError::PersistenceError(format!("Failed to queue async binary event: {}", e))
            })?;
            return Ok(());
        }

        // PERFORMANCE FIX: Use persistent buffered writer instead of reopening file
        if self.binary_writer.is_none() {
            let file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.events_file)
                .map_err(|e| {
                    EngineError::PersistenceError(format!("Failed to open events file: {}", e))
                })?;
            self.binary_writer = Some(BufWriter::new(file));
        }

        if let Some(writer) = self.binary_writer.as_mut() {
            use std::io::Write as _;

            // 1. Write Length Prefix (u64 little endian)
            let len = data.len() as u64;
            writer.write_all(&len.to_le_bytes()).map_err(|e| {
                EngineError::PersistenceError(format!("Failed to write binary event length: {}", e))
            })?;

            // 2. Write Payload
            writer.write_all(data).map_err(|e| {
                EngineError::PersistenceError(format!("Failed to write binary event data: {}", e))
            })?;

            // Flush immediately for binary events
            writer.flush().map_err(|e| {
                EngineError::PersistenceError(format!("Failed to flush binary event: {}", e))
            })?;

            if self.fsync_on_append {
                if let Ok(file) = writer.get_ref().try_clone() {
                    file.sync_all().map_err(|e| {
                        EngineError::PersistenceError(format!("Failed to sync binary event: {}", e))
                    })?;
                }
            }
        }
        Ok(())
    }

    /// Flush the current batch of events to disk
    pub fn flush_batch(&mut self) -> EngineResult<()> {
        // Optimized async path: delegate flush to async writer
        if let Some(ref aw) = self.async_writer {
            aw.flush()?;
            // Keep counters clean
            self.event_batch.clear();
            self.batch_count = 0;
            return Ok(());
        }

        if self.event_batch.is_empty() {
            return Ok(());
        }

        if self.writer.is_none() {
            let file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.events_file)
                .map_err(|e| {
                    EngineError::PersistenceError(format!("Failed to open events file: {}", e))
                })?;
            self.writer = Some(BufWriter::new(file));
        }

        if let Some(writer) = self.writer.as_mut() {
            // Write all batched events in one go (with optional CRC32 checksums)
            for event_json in &self.event_batch {
                let line = if self.enable_checksums {
                    format_event_with_crc32(event_json)
                } else {
                    event_json.clone()
                };
                writeln!(writer, "{}", line).map_err(|e| {
                    EngineError::PersistenceError(format!("Failed to write event: {}", e))
                })?;
            }

            // Flush to OS buffer
            writer.flush().map_err(|e| {
                EngineError::PersistenceError(format!("Failed to flush batch: {}", e))
            })?;

            // Optional fsync for durability
            if self.fsync_on_append {
                if let Ok(file) = writer.get_ref().try_clone() {
                    file.sync_all().map_err(|e| {
                        EngineError::PersistenceError(format!("Failed to sync events file: {}", e))
                    })?;
                }
            }
        }

        // Removed verbose logging for performance

        // Clear batch
        self.event_batch.clear();
        self.batch_count = 0;

        // Rotate if threshold exceeded
        self.maybe_rotate()?;
        Ok(())
    }

    /// Flush any remaining events and the buffered writer
    pub fn flush_events(&mut self) -> EngineResult<()> {
        // First flush any pending batch (handles async path too)
        self.flush_batch()?;

        // Then flush the writer buffer (legacy path)
        if let Some(writer) = self.writer.as_mut() {
            writer.flush().map_err(|e| {
                EngineError::PersistenceError(format!("Failed to flush events writer: {}", e))
            })?;
        }
        Ok(())
    }

    /// Get current size of events file (used to record start offset of next append)
    pub fn current_events_size(&self) -> EngineResult<u64> {
        let len = std::fs::metadata(&self.events_file).map(|m| m.len()).unwrap_or(0);
        Ok(len)
    }

    /// Append an index entry mapping aggregate_id to starting byte offset of the line in events.raftlog
    /// PERFORMANCE FIX: Changed from &self to &mut self to use persistent buffered writer
    pub fn append_index_entry(&mut self, aggregate_id: &str, offset: u64) -> EngineResult<()> {
        // PERFORMANCE FIX: Use persistent buffered writer instead of reopening file
        if self.index_writer.is_none() {
            let file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.index_file)
                .map_err(|e| {
                    EngineError::PersistenceError(format!("Failed to open index file: {}", e))
                })?;
            self.index_writer = Some(BufWriter::new(file));
        }

        if let Some(writer) = self.index_writer.as_mut() {
            let rec = serde_json::json!({
                "aggregate_id": aggregate_id,
                "offset": offset
            })
            .to_string();

            writeln!(writer, "{}", rec).map_err(|e| {
                EngineError::PersistenceError(format!("Failed to write index entry: {}", e))
            })?;

            // Flush immediately for index entries
            writer.flush().map_err(|e| {
                EngineError::PersistenceError(format!("Failed to flush index entry: {}", e))
            })?;

            if self.fsync_on_append {
                if let Ok(file) = writer.get_ref().try_clone() {
                    file.sync_all().map_err(|e| {
                        EngineError::PersistenceError(format!("Failed to sync index file: {}", e))
                    })?;
                }
            }
        }
        Ok(())
    }

    /// Read all offsets for a given aggregate_id from index
    pub fn read_index_offsets(&self, aggregate_id: &str) -> EngineResult<Vec<u64>> {
        if !Path::new(&self.index_file).exists() {
            return Ok(vec![]);
        }
        let mut out = Vec::new();
        if let Ok(content) = fs::read_to_string(&self.index_file) {
            for line in content.lines() {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                    if v.get("aggregate_id").and_then(|x| x.as_str()) == Some(aggregate_id) {
                        if let Some(off) = v.get("offset").and_then(|x| x.as_u64()) {
                            out.push(off);
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    /// Append a deduplication event_id line to index file
    pub fn append_dedup_id(&self, event_id: &str) -> EngineResult<()> {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.dedup_ids_file)
            .map_err(|e| {
                EngineError::PersistenceError(format!("Failed to open dedup ids file: {}", e))
            })?;
        writeln!(file, "{}", event_id).map_err(|e| {
            EngineError::PersistenceError(format!("Failed to write dedup id: {}", e))
        })?;
        if self.fsync_on_append {
            file.sync_all().map_err(|e| {
                EngineError::PersistenceError(format!("Failed to sync dedup ids file: {}", e))
            })?;
        }
        Ok(())
    }

    /// Read all deduplication ids
    pub fn read_all_dedup_ids(&self) -> EngineResult<Vec<String>> {
        if !Path::new(&self.dedup_ids_file).exists() {
            return Ok(vec![]);
        }
        let content = fs::read_to_string(&self.dedup_ids_file).map_err(|e| {
            EngineError::PersistenceError(format!("Failed to read dedup ids file: {}", e))
        })?;
        Ok(content
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|s| s.to_string())
            .collect())
    }

    /// Truncate the events log after snapshot (compaction)
    pub fn truncate_events(&mut self) -> EngineResult<()> {
        // Drop/flush async writer if enabled
        if let Some(aw) = self.async_writer.take() {
            let _ = aw.flush();
            let _ = aw.shutdown();
        }
        // Drop legacy writer so file can be replaced
        self.writer = None;
        fs::write(&self.events_file, "")
            .map_err(|e| EngineError::PersistenceError(format!("Failed to truncate log: {}", e)))?;
        Ok(())
    }

    /// Rotate events file if size exceeds configured threshold
    fn maybe_rotate(&mut self) -> EngineResult<()> {
        if self.max_log_file_size == 0 {
            return Ok(());
        }
        if let Ok(m) = fs::metadata(&self.events_file) {
            if m.len() as usize >= self.max_log_file_size {
                // Close writer and rotate to .1
                self.writer = None;
                let seg1 = format!("{}.1", &self.events_file);
                let _ = fs::remove_file(&seg1);
                fs::rename(&self.events_file, &seg1).map_err(|e| {
                    EngineError::PersistenceError(format!("Failed to rotate log: {}", e))
                })?;
                // Open fresh file
                let file = fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.events_file)
                    .map_err(|e| {
                        EngineError::PersistenceError(format!(
                            "Failed to open new events file: {}",
                            e
                        ))
                    })?;
                self.writer = Some(BufWriter::new(file));
                println!("🔁 Log rotated: {} -> {}", &self.events_file, &seg1);
            }
        }
        Ok(())
    }

    /// Read all events from the event log (supports simple one-segment rotation)
    ///
    /// Returns events as JSON strings, one per line
    /// Validates CRC32 checksums if present and rejects corrupted events
    pub fn read_all_events(&self) -> EngineResult<Vec<String>> {
        let mut all = Vec::new();
        let mut corrupted_count = 0;
        let seg1 = format!("{}.1", &self.events_file);
        for path in [seg1.as_str(), &self.events_file] {
            if !Path::new(path).exists() {
                continue;
            }
            let content = fs::read_to_string(path).map_err(|e| {
                EngineError::PersistenceError(format!("Failed to read events file {}: {}", path, e))
            })?;
            for (line_num, line) in content.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                match parse_and_validate_event(line) {
                    Ok(json_data) => all.push(json_data),
                    Err(e) => {
                        corrupted_count += 1;
                        eprintln!(
                            "⚠️ CRC32 validation error at {}:{}: {}",
                            path,
                            line_num + 1,
                            e
                        );
                        // Reject corrupted events - data integrity is critical
                    }
                }
            }
        }
        if all.is_empty() && corrupted_count == 0 {
            println!("📂 No events file found, starting with empty log");
        } else if corrupted_count > 0 {
            eprintln!(
                "🚨 Loaded {} events from log, {} corrupted events REJECTED",
                all.len(),
                corrupted_count
            );
        } else {
            println!("📂 Loaded {} events from log (CRC32 validated ✓)", all.len());
        }
        Ok(all)
    }

    /// Read all event lines as raw bytes (supports simple one-segment rotation)
    /// Uses Length-Prefixed Framing (8 bytes length + payload)
    pub fn read_all_event_bytes(&self) -> EngineResult<Vec<Vec<u8>>> {
        let mut all = Vec::new();
        let seg1 = format!("{}.1", &self.events_file);
        for path in [seg1.as_str(), &self.events_file] {
            if !Path::new(path).exists() {
                continue;
            }
            let content = std::fs::read(path).map_err(|e| {
                EngineError::PersistenceError(format!("Failed to read events file {}: {}", path, e))
            })?;

            // Parse framed data: [Length: u64][Payload: [u8; Length]]
            let mut cursor = 0;
            while cursor < content.len() {
                // Ensure we can read the length prefix
                if cursor + 8 > content.len() {
                    println!("⚠️ Warning: Incomplete length prefix at end of file {}", path);
                    break;
                }

                // Read 8 bytes for length
                let len_bytes: [u8; 8] = content[cursor..cursor+8].try_into().unwrap();
                let len = u64::from_le_bytes(len_bytes) as usize;
                cursor += 8;

                // Ensure we can read the payload
                if cursor + len > content.len() {
                    println!("⚠️ Warning: Incomplete payload at end of file {} (expected {} bytes, found {})", path, len, content.len() - cursor);
                    break;
                }

                // Extract payload
                let payload = content[cursor..cursor+len].to_vec();
                all.push(payload);
                cursor += len;
            }
        }
        if all.is_empty() {
            println!("📂 No events file found (or empty), starting with empty log");
        } else {
            println!("📂 Loaded {} binary events from log (Length-Prefixed)", all.len());
        }
        Ok(all)
    }

    /// Save a state snapshot
    ///
    /// Snapshots are stored as pretty-printed JSON for debugging
    pub fn save_snapshot(&self, state_json: &str) -> EngineResult<()> {
        fs::write(&self.snapshot_file, state_json).map_err(|e| {
            EngineError::PersistenceError(format!("Failed to write snapshot: {}", e))
        })?;

        println!("📸 State snapshot saved: {} bytes", state_json.len());

        Ok(())
    }

    /// Load the latest state snapshot
    pub fn load_snapshot(&self) -> EngineResult<Option<String>> {
        if !Path::new(&self.snapshot_file).exists() {
            println!("📂 No snapshot found, will use initial state");
            return Ok(None);
        }

        let content = fs::read_to_string(&self.snapshot_file).map_err(|e| {
            EngineError::PersistenceError(format!("Failed to read snapshot: {}", e))
        })?;

        println!("📂 Loaded state snapshot: {} bytes", content.len());

        Ok(Some(content))
    }

    /// Ensure metadata file exists with basic information
    fn ensure_metadata_file(&self) -> EngineResult<()> {
        if Path::new(&self.metadata_file).exists() {
            return Ok(());
        }

        let metadata = serde_json::json!({
            "version": "1.0.0",
            "format": "lithair-db",
            "created_at": "2025-01-29T11:04:14Z",
            "description": "Lithair event-sourced database"
        });

        let metadata_json = serde_json::to_string_pretty(&metadata).map_err(|e| {
            EngineError::SerializationError(format!("Failed to serialize metadata: {}", e))
        })?;

        fs::write(&self.metadata_file, metadata_json).map_err(|e| {
            EngineError::PersistenceError(format!("Failed to write metadata: {}", e))
        })?;

        // Metadata file created silently

        Ok(())
    }

    /// Get database statistics
    pub fn get_stats(&self) -> EngineResult<DatabaseStats> {
        let events_size = if Path::new(&self.events_file).exists() {
            fs::metadata(&self.events_file).map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };

        let snapshot_size = if Path::new(&self.snapshot_file).exists() {
            fs::metadata(&self.snapshot_file).map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };

        let event_count = self.read_all_events()?.len();

        Ok(DatabaseStats {
            events_file_size: events_size,
            snapshot_file_size: snapshot_size,
            total_events: event_count,
            database_path: self.base_path.clone(),
        })
    }

    /// Configure batch settings for optimal performance
    pub fn configure_batching(&mut self, max_batch_size: usize, fsync_on_append: bool) {
        self.max_batch_size = max_batch_size;
        self.fsync_on_append = fsync_on_append;
        // Batch configured silently for performance
    }

    /// Force flush any pending events (call before shutdown)
    pub fn force_flush(&mut self) -> EngineResult<()> {
        self.flush_batch()?;
        if let Some(writer) = self.writer.as_mut() {
            writer.flush().map_err(|e| {
                EngineError::PersistenceError(format!("Failed to force flush: {}", e))
            })?;
            if let Ok(file) = writer.get_ref().try_clone() {
                file.sync_all().map_err(|e| {
                    EngineError::PersistenceError(format!("Failed to force sync: {}", e))
                })?;
            }
        }
        Ok(())
    }
}

/// Automatic flush on drop to ensure data persistence
///
/// This implementation guarantees that buffered events are flushed to disk
/// when the FileStorage is dropped, preventing data loss on program exit.
impl Drop for FileStorage {
    fn drop(&mut self) {
        // Flush any remaining events in the buffer
        if !self.event_batch.is_empty() || self.async_writer.is_some() {
            if let Err(e) = self.flush_batch() {
                eprintln!("⚠️  Warning: Failed to flush events on drop: {}", e);
                eprintln!("    {} buffered events may have been lost", self.event_batch.len());
            } else if !self.event_batch.is_empty() {
                println!("💾 Auto-flushed {} events on storage drop", self.event_batch.len());
            }
        }

        // Close the writer properly
        if let Some(mut writer) = self.writer.take() {
            if let Err(e) = writer.flush() {
                eprintln!("⚠️  Warning: Failed to flush writer on drop: {}", e);
            }
        }
    }
}

/// Database statistics
#[derive(Debug, Clone)]
pub struct DatabaseStats {
    pub events_file_size: u64,
    pub snapshot_file_size: u64,
    pub total_events: usize,
    pub database_path: String,
}

/// Generic storage engine trait
pub trait StorageEngine {
    fn store(&mut self, key: &str, value: &[u8]) -> EngineResult<()>;
    fn load(&self, key: &str) -> EngineResult<Option<Vec<u8>>>;
    fn delete(&mut self, key: &str) -> EngineResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc32_calculation() {
        let data = b"hello world";
        let crc = calculate_crc32(data);
        assert_eq!(crc, 0x0d4a1185); // Known CRC32 for "hello world"
    }

    #[test]
    fn test_format_event_with_crc32() {
        let event = r#"{"type":"ArticleCreated","id":"1"}"#;
        let formatted = format_event_with_crc32(event);

        // Format should be "<crc32_hex>:<json_data>"
        assert!(formatted.len() > 9);
        assert_eq!(formatted.chars().nth(8), Some(':'));

        // CRC32 hex should be exactly 8 characters
        let crc_hex = &formatted[..8];
        assert!(u32::from_str_radix(crc_hex, 16).is_ok());

        // JSON should be preserved
        assert!(formatted.ends_with(event));
    }

    #[test]
    fn test_parse_and_validate_event_valid() {
        let event = r#"{"type":"ArticleCreated","id":"1"}"#;
        let formatted = format_event_with_crc32(event);

        let result = parse_and_validate_event(&formatted);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), event);
    }

    #[test]
    fn test_parse_and_validate_event_corrupted() {
        // Valid format but corrupted data (wrong CRC)
        let corrupted = "00000000:{\"type\":\"ArticleCreated\",\"id\":\"1\"}";
        let result = parse_and_validate_event(corrupted);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("CRC32 mismatch"));
    }

    #[test]
    fn test_parse_and_validate_event_legacy() {
        // Legacy format (no CRC) - should be accepted for backward compatibility
        let legacy = r#"{"type":"ArticleCreated","id":"1"}"#;
        let result = parse_and_validate_event(legacy);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), legacy);
    }

    #[test]
    fn test_crc32_round_trip() {
        let events = vec![
            r#"{"type":"ArticleCreated","id":"1","title":"Hello"}"#,
            r#"{"type":"ArticleUpdated","id":"1","title":"Updated"}"#,
            r#"{"type":"ArticleDeleted","id":"1"}"#,
        ];

        for event in events {
            let formatted = format_event_with_crc32(event);
            let parsed = parse_and_validate_event(&formatted);
            assert!(parsed.is_ok(), "Failed to parse: {}", formatted);
            assert_eq!(parsed.unwrap(), event);
        }
    }

    #[test]
    fn test_crc32_detects_bit_flip() {
        let event = r#"{"type":"ArticleCreated","id":"1"}"#;
        let formatted = format_event_with_crc32(event);

        // Flip a bit in the JSON data (change 'A' to 'B')
        let corrupted = formatted.replace("ArticleCreated", "BrticleCreated");

        let result = parse_and_validate_event(&corrupted);
        assert!(result.is_err(), "CRC32 should detect bit flip");
    }
}
