//! Consensus Log for CRUD operations
//!
//! This module provides a Raft-style ordered log for CRUD operations.
//! All write operations go through this log to ensure consistent ordering
//! across all nodes in the cluster.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use super::upgrade::{SchemaChange, Version};

/// A unique identifier for a log entry (term, index)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LogId {
    pub term: u64,
    pub index: u64,
}

impl LogId {
    pub fn new(term: u64, index: u64) -> Self {
        Self { term, index }
    }
}

/// CRUD operation types that go through consensus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CrudOperation {
    Create {
        model_path: String,
        data: serde_json::Value,
    },
    Update {
        model_path: String,
        id: String,
        data: serde_json::Value,
    },
    Delete {
        model_path: String,
        id: String,
    },
    // === Migration Operations (Phase 1: Foundation) ===
    /// Begin migration transaction
    MigrationBegin {
        from_version: Version,
        to_version: Version,
        migration_id: Uuid,
    },
    /// Individual migration step (applied in order)
    MigrationStep {
        migration_id: Uuid,
        step_index: u32,
        operation: SchemaChange,
    },
    /// Commit migration (all steps succeeded)
    MigrationCommit {
        migration_id: Uuid,
        checksum: String,
    },
    /// Rollback migration (step failed)
    MigrationRollback {
        migration_id: Uuid,
        failed_step: u32,
        reason: String,
    },
}

/// A log entry containing a CRUD operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub log_id: LogId,
    pub operation: CrudOperation,
    /// Timestamp when the entry was created (for debugging)
    pub timestamp_ms: u64,
}

/// Result of applying a log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ApplyResult {
    Success { data: Option<serde_json::Value> },
    Error { message: String },
}

/// The consensus log maintains ordered CRUD operations
pub struct ConsensusLog {
    /// Current term (increments on leader change)
    current_term: AtomicU64,
    /// Next log index to assign
    next_index: AtomicU64,
    /// Committed log index (all entries up to this are committed)
    commit_index: AtomicU64,
    /// The log entries
    entries: RwLock<Vec<LogEntry>>,
    /// Applied index (all entries up to this have been applied to state machine)
    applied_index: AtomicU64,
    /// Mutex to serialize entry application (prevents concurrent handlers from racing)
    apply_mutex: Mutex<()>,
}

impl ConsensusLog {
    pub fn new() -> Self {
        Self {
            current_term: AtomicU64::new(1),
            next_index: AtomicU64::new(1),
            commit_index: AtomicU64::new(0),
            entries: RwLock::new(Vec::new()),
            applied_index: AtomicU64::new(0),
            apply_mutex: Mutex::new(()),
        }
    }

    /// Get the current term
    pub fn current_term(&self) -> u64 {
        self.current_term.load(Ordering::SeqCst)
    }

    /// Increment term (called on leader election)
    pub fn increment_term(&self) -> u64 {
        self.current_term.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Set term (when accepting a higher term from another node)
    pub fn set_term(&self, term: u64) {
        self.current_term.store(term, Ordering::SeqCst);
    }

    /// Append a new operation to the log (leader only)
    /// Returns the LogId assigned to this entry
    ///
    /// NOTE: With concurrent requests, entries might acquire indices out of order
    /// (request A gets index 5, request B gets index 6, but B acquires lock first).
    /// We insert in sorted order to ensure the entries Vec is always ordered by log_id.
    pub async fn append(&self, operation: CrudOperation) -> LogEntry {
        let term = self.current_term.load(Ordering::SeqCst);
        let index = self.next_index.fetch_add(1, Ordering::SeqCst);

        let entry = LogEntry {
            log_id: LogId::new(term, index),
            operation,
            timestamp_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        };

        let mut entries = self.entries.write().await;
        // Insert in sorted order by log_id to handle concurrent requests
        // that might acquire the lock out of index order
        let pos = entries.iter()
            .position(|e| e.log_id > entry.log_id)
            .unwrap_or(entries.len());
        entries.insert(pos, entry.clone());

        entry
    }

    /// Append entries received from leader (follower)
    /// Returns true if entries were appended successfully
    pub async fn append_entries(&self, entries: Vec<LogEntry>, leader_commit: u64) -> bool {
        let mut log = self.entries.write().await;

        for entry in entries {
            // Check if we already have this entry
            let existing = log.iter().find(|e| e.log_id == entry.log_id);
            if existing.is_none() {
                // Insert in order
                let pos = log.iter()
                    .position(|e| e.log_id > entry.log_id)
                    .unwrap_or(log.len());
                log.insert(pos, entry);
            }
        }

        // Update commit index atomically (only increase, never decrease)
        // Using fetch_max ensures thread-safe updates under concurrent requests
        self.commit_index.fetch_max(leader_commit, Ordering::SeqCst);

        true
    }

    /// Mark entries as committed up to the given index
    /// Uses atomic fetch_max to ensure commit_index never goes backwards
    /// even when concurrent requests commit out of order
    pub fn commit(&self, index: u64) {
        // Use fetch_max to ensure monotonic increase only
        // This prevents race conditions where request A (index 5) commits after
        // request B (index 6), which would incorrectly lower commit_index from 6 to 5
        self.commit_index.fetch_max(index, Ordering::SeqCst);
    }

    /// Get the current commit index
    pub fn commit_index(&self) -> u64 {
        self.commit_index.load(Ordering::SeqCst)
    }

    /// Get the current applied index
    pub fn applied_index(&self) -> u64 {
        self.applied_index.load(Ordering::SeqCst)
    }

    /// Get entries that need to be applied (committed but not yet applied)
    ///
    /// IMPORTANT: Returns entries in strict sequential order starting from applied_index + 1.
    /// Stops at any gap to ensure entries are always applied in order.
    /// This prevents the bug where entry N+1 gets applied before entry N, causing N to be skipped.
    pub async fn get_unapplied_entries(&self) -> Vec<LogEntry> {
        let applied = self.applied_index.load(Ordering::SeqCst);
        let committed = self.commit_index.load(Ordering::SeqCst);

        if committed <= applied {
            return Vec::new();
        }

        let entries = self.entries.read().await;
        let mut result = Vec::new();
        let mut expected_index = applied + 1;

        // Walk through entries in strict sequential order
        // Stop at any gap to ensure we don't skip entries
        while expected_index <= committed {
            // Find entry with the expected index
            let entry = entries.iter().find(|e| e.log_id.index == expected_index);
            match entry {
                Some(e) => {
                    result.push(e.clone());
                    expected_index += 1;
                }
                None => {
                    // Gap detected - stop here to avoid skipping entries
                    // The missing entry will arrive later and we'll apply in order
                    break;
                }
            }
        }

        result
    }

    /// Mark an entry as applied
    /// Uses atomic fetch_max to ensure applied_index never goes backwards
    /// even under concurrent access from multiple threads
    pub fn mark_applied(&self, index: u64) {
        // Use fetch_max for atomic compare-and-swap that only increases the value
        // This prevents race conditions where concurrent threads could cause
        // applied_index to go backwards (e.g., thread A marks 10, thread B marks 5)
        self.applied_index.fetch_max(index, Ordering::SeqCst);
    }

    /// Lock the apply mutex to serialize entry application
    /// This prevents race conditions where multiple concurrent handlers could
    /// process the same entries or process entries out of order.
    ///
    /// IMPORTANT: Hold this lock for the ENTIRE duration of applying entries.
    /// The returned guard should be held until all entries are applied.
    pub async fn lock_apply(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.apply_mutex.lock().await
    }

    /// Get entries from a given index for replication to followers
    pub async fn get_entries_from(&self, from_index: u64) -> Vec<LogEntry> {
        let entries = self.entries.read().await;
        entries
            .iter()
            .filter(|e| e.log_id.index >= from_index)
            .cloned()
            .collect()
    }

    /// Get the last log entry
    pub async fn last_entry(&self) -> Option<LogEntry> {
        let entries = self.entries.read().await;
        entries.last().cloned()
    }

    /// Get the last log index
    pub async fn last_index(&self) -> u64 {
        let entries = self.entries.read().await;
        entries.last().map(|e| e.log_id.index).unwrap_or(0)
    }
}

impl Default for ConsensusLog {
    fn default() -> Self {
        Self::new()
    }
}

/// Request to append entries (sent from leader to followers)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendEntriesRequest {
    /// Leader's term
    pub term: u64,
    /// Leader's node ID
    pub leader_id: u64,
    /// Index of log entry immediately preceding new ones
    pub prev_log_index: u64,
    /// Term of prev_log_index entry
    pub prev_log_term: u64,
    /// Log entries to store (empty for heartbeat)
    pub entries: Vec<LogEntry>,
    /// Leader's commit index
    pub leader_commit: u64,
}

/// Response to append entries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendEntriesResponse {
    /// Current term, for leader to update itself
    pub term: u64,
    /// True if follower contained entry matching prev_log_index and prev_log_term
    pub success: bool,
    /// The follower's last log index (for leader to know where to send from)
    pub last_log_index: u64,
    /// The follower's applied index (entries actually applied to state machine)
    #[serde(default)]
    pub applied_index: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_log_append() {
        let log = ConsensusLog::new();

        let entry = log.append(CrudOperation::Create {
            model_path: "/api/products".to_string(),
            data: serde_json::json!({"name": "Test"}),
        }).await;

        assert_eq!(entry.log_id.term, 1);
        assert_eq!(entry.log_id.index, 1);
    }

    #[tokio::test]
    async fn test_log_ordering() {
        let log = ConsensusLog::new();

        let entry1 = log.append(CrudOperation::Create {
            model_path: "/api/products".to_string(),
            data: serde_json::json!({"id": "1"}),
        }).await;

        let entry2 = log.append(CrudOperation::Update {
            model_path: "/api/products".to_string(),
            id: "1".to_string(),
            data: serde_json::json!({"id": "1", "name": "Updated"}),
        }).await;

        let entry3 = log.append(CrudOperation::Delete {
            model_path: "/api/products".to_string(),
            id: "1".to_string(),
        }).await;

        assert!(entry1.log_id < entry2.log_id);
        assert!(entry2.log_id < entry3.log_id);
    }

    #[tokio::test]
    async fn test_commit_and_apply() {
        let log = ConsensusLog::new();

        log.append(CrudOperation::Create {
            model_path: "/api/products".to_string(),
            data: serde_json::json!({"id": "1"}),
        }).await;

        log.append(CrudOperation::Create {
            model_path: "/api/products".to_string(),
            data: serde_json::json!({"id": "2"}),
        }).await;

        // Nothing committed yet
        let unapplied = log.get_unapplied_entries().await;
        assert!(unapplied.is_empty());

        // Commit first entry
        log.commit(1);
        let unapplied = log.get_unapplied_entries().await;
        assert_eq!(unapplied.len(), 1);

        // Mark as applied
        log.mark_applied(1);
        let unapplied = log.get_unapplied_entries().await;
        assert!(unapplied.is_empty());

        // Commit second entry
        log.commit(2);
        let unapplied = log.get_unapplied_entries().await;
        assert_eq!(unapplied.len(), 1);
    }
}
