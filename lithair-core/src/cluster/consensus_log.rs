//! Consensus Log for CRUD operations
//!
//! This module provides a Raft-style ordered log for CRUD operations.
//! All write operations go through this log to ensure consistent ordering
//! across all nodes in the cluster.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::RwLock;

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
}

impl ConsensusLog {
    pub fn new() -> Self {
        Self {
            current_term: AtomicU64::new(1),
            next_index: AtomicU64::new(1),
            commit_index: AtomicU64::new(0),
            entries: RwLock::new(Vec::new()),
            applied_index: AtomicU64::new(0),
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
        entries.push(entry.clone());

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

        // Update commit index
        let current_commit = self.commit_index.load(Ordering::SeqCst);
        if leader_commit > current_commit {
            self.commit_index.store(leader_commit, Ordering::SeqCst);
        }

        true
    }

    /// Mark entries as committed up to the given index
    pub fn commit(&self, index: u64) {
        self.commit_index.store(index, Ordering::SeqCst);
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
    pub async fn get_unapplied_entries(&self) -> Vec<LogEntry> {
        let applied = self.applied_index.load(Ordering::SeqCst);
        let committed = self.commit_index.load(Ordering::SeqCst);

        if committed <= applied {
            return Vec::new();
        }

        let entries = self.entries.read().await;
        entries
            .iter()
            .filter(|e| e.log_id.index > applied && e.log_id.index <= committed)
            .cloned()
            .collect()
    }

    /// Mark an entry as applied
    pub fn mark_applied(&self, index: u64) {
        let current = self.applied_index.load(Ordering::SeqCst);
        if index > current {
            self.applied_index.store(index, Ordering::SeqCst);
        }
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
