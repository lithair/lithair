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

    /// Rebuild the consensus log from WAL entries after a node restart.
    ///
    /// The WAL (Write-Ahead Log) persists every operation to disk. When a node
    /// restarts, the in-memory ConsensusLog is empty. This method replays the
    /// WAL entries to restore the log to its pre-crash state.
    ///
    /// After replay:
    /// - `entries` contains all recovered operations in order
    /// - `next_index` is set past the last recovered entry
    /// - `current_term` is restored to the highest term seen
    /// - `commit_index` and `applied_index` are left at 0
    ///
    /// **Why not mark entries as committed?** On a leader, entries are written
    /// to the WAL *before* the replication quorum is reached. If the leader
    /// crashes before majority ack, those entries are uncommitted. We
    /// conservatively leave commit_index at 0 — the cluster leader will
    /// re-establish the correct commit_index via the normal replication flow.
    /// Followers will receive the correct commit_index from the leader's
    /// AppendEntries RPCs. The EventStore (model state) has its own persistence
    /// and doesn't depend on the consensus log's commit_index for recovery.
    pub async fn replay_from_wal_entries(&self, wal_entries: Vec<LogEntry>) -> usize {
        if wal_entries.is_empty() {
            return 0;
        }

        let mut entries = self.entries.write().await;

        let mut max_index = 0u64;
        let mut max_term = 0u64;

        for entry in wal_entries {
            if entry.log_id.index > max_index {
                max_index = entry.log_id.index;
            }
            if entry.log_id.term > max_term {
                max_term = entry.log_id.term;
            }
            entries.push(entry);
        }

        // WAL entries are append-only and already ordered by index.
        // Assert ordering in debug builds rather than sorting (O(n) vs O(n log n)).
        debug_assert!(
            entries.windows(2).all(|w| w[0].log_id.index <= w[1].log_id.index),
            "WAL entries not ordered by index — WAL corruption?"
        );

        let count = entries.len();

        // Restore only the index counters:
        // - next_index: one past the last entry, so new appends don't collide
        // - current_term: the highest term seen in the WAL
        // - commit_index: stays at 0 (conservative — see docstring above)
        // - applied_index: stays at 0 (re-apply via normal replication flow)
        self.next_index.store(max_index + 1, Ordering::SeqCst);
        self.current_term.store(max_term, Ordering::SeqCst);

        count
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
        let pos = entries.iter().position(|e| e.log_id > entry.log_id).unwrap_or(entries.len());
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
                let pos = log.iter().position(|e| e.log_id > entry.log_id).unwrap_or(log.len());
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
        entries.iter().filter(|e| e.log_id.index >= from_index).cloned().collect()
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

        let entry = log
            .append(CrudOperation::Create {
                model_path: "/api/products".to_string(),
                data: serde_json::json!({"name": "Test"}),
            })
            .await;

        assert_eq!(entry.log_id.term, 1);
        assert_eq!(entry.log_id.index, 1);
    }

    #[tokio::test]
    async fn test_log_ordering() {
        let log = ConsensusLog::new();

        let entry1 = log
            .append(CrudOperation::Create {
                model_path: "/api/products".to_string(),
                data: serde_json::json!({"id": "1"}),
            })
            .await;

        let entry2 = log
            .append(CrudOperation::Update {
                model_path: "/api/products".to_string(),
                id: "1".to_string(),
                data: serde_json::json!({"id": "1", "name": "Updated"}),
            })
            .await;

        let entry3 = log
            .append(CrudOperation::Delete {
                model_path: "/api/products".to_string(),
                id: "1".to_string(),
            })
            .await;

        assert!(entry1.log_id < entry2.log_id);
        assert!(entry2.log_id < entry3.log_id);
    }

    #[tokio::test]
    async fn test_commit_and_apply() {
        let log = ConsensusLog::new();

        log.append(CrudOperation::Create {
            model_path: "/api/products".to_string(),
            data: serde_json::json!({"id": "1"}),
        })
        .await;

        log.append(CrudOperation::Create {
            model_path: "/api/products".to_string(),
            data: serde_json::json!({"id": "2"}),
        })
        .await;

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

    #[tokio::test(flavor = "multi_thread")]
    async fn test_concurrent_appends_all_present() {
        use std::sync::Arc;

        let log = Arc::new(ConsensusLog::new());
        let mut handles = Vec::new();

        for _ in 0..100 {
            let l = Arc::clone(&log);
            handles.push(tokio::spawn(async move {
                l.append(CrudOperation::Create {
                    model_path: "/api/items".to_string(),
                    data: serde_json::json!({}),
                })
                .await
            }));
        }

        for h in handles {
            h.await.unwrap();
        }

        let entries = log.entries.read().await;
        assert_eq!(entries.len(), 100);

        // Verify all indices 1..=100 are present and sorted
        for (i, entry) in entries.iter().enumerate() {
            assert_eq!(entry.log_id.index, (i + 1) as u64);
        }
    }

    #[tokio::test]
    async fn test_unapplied_stops_at_gap() {
        let log = ConsensusLog::new();

        // Append entries 1 and 2
        log.append(CrudOperation::Create {
            model_path: "/api/a".to_string(),
            data: serde_json::json!({}),
        })
        .await;
        log.append(CrudOperation::Create {
            model_path: "/api/b".to_string(),
            data: serde_json::json!({}),
        })
        .await;

        // Skip index 3 — manually append entry with index 4
        let entry4 = LogEntry {
            log_id: LogId::new(1, 4),
            operation: CrudOperation::Create {
                model_path: "/api/d".to_string(),
                data: serde_json::json!({}),
            },
            timestamp_ms: 0,
        };
        log.append_entries(vec![entry4], 4).await;

        // Commit up to 4, but gap at 3 means we only get 1 and 2
        let unapplied = log.get_unapplied_entries().await;
        assert_eq!(unapplied.len(), 2);
        assert_eq!(unapplied[0].log_id.index, 1);
        assert_eq!(unapplied[1].log_id.index, 2);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_commit_monotonicity() {
        use std::sync::Arc;

        let log = Arc::new(ConsensusLog::new());
        let mut handles = Vec::new();

        for val in [5, 3, 10, 7, 1, 8, 2, 9, 4, 6] {
            let l = Arc::clone(&log);
            handles.push(tokio::spawn(async move {
                l.commit(val);
            }));
        }

        for h in handles {
            h.await.unwrap();
        }

        // commit_index should be the maximum: 10
        assert_eq!(log.commit_index(), 10);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_mark_applied_monotonicity() {
        use std::sync::Arc;

        let log = Arc::new(ConsensusLog::new());
        let mut handles = Vec::new();

        for val in [5, 3, 10, 7, 1, 8] {
            let l = Arc::clone(&log);
            handles.push(tokio::spawn(async move {
                l.mark_applied(val);
            }));
        }

        for h in handles {
            h.await.unwrap();
        }

        assert_eq!(log.applied_index(), 10);
    }

    #[tokio::test]
    async fn test_append_entries_dedup() {
        let log = ConsensusLog::new();

        // Append entries 1-5 locally
        for _ in 0..5 {
            log.append(CrudOperation::Create {
                model_path: "/api/x".to_string(),
                data: serde_json::json!({}),
            })
            .await;
        }

        // Simulate follower receiving entries 3-8 from leader (overlap on 3-5)
        let new_entries: Vec<LogEntry> = (3..=8)
            .map(|i| LogEntry {
                log_id: LogId::new(1, i),
                operation: CrudOperation::Create {
                    model_path: "/api/x".to_string(),
                    data: serde_json::json!({}),
                },
                timestamp_ms: 0,
            })
            .collect();

        log.append_entries(new_entries, 8).await;

        let entries = log.entries.read().await;
        assert_eq!(entries.len(), 8); // No duplicates
        for (i, entry) in entries.iter().enumerate() {
            assert_eq!(entry.log_id.index, (i + 1) as u64);
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_lock_apply_serialization() {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::sync::Arc;

        let log = Arc::new(ConsensusLog::new());

        // Append and commit 10 entries
        for _ in 0..10 {
            log.append(CrudOperation::Create {
                model_path: "/api/items".to_string(),
                data: serde_json::json!({}),
            })
            .await;
        }
        log.commit(10);

        let apply_count = Arc::new(AtomicU64::new(0));
        let mut handles = Vec::new();

        // 5 tasks race to apply entries
        for _ in 0..5 {
            let l = Arc::clone(&log);
            let count = Arc::clone(&apply_count);
            handles.push(tokio::spawn(async move {
                let _guard = l.lock_apply().await;
                let unapplied = l.get_unapplied_entries().await;
                for entry in &unapplied {
                    l.mark_applied(entry.log_id.index);
                    count.fetch_add(1, Ordering::Relaxed);
                }
            }));
        }

        for h in handles {
            h.await.unwrap();
        }

        // Each entry should be applied exactly once (total = 10)
        assert_eq!(apply_count.load(Ordering::Relaxed), 10);
        assert_eq!(log.applied_index(), 10);
    }

    #[tokio::test]
    async fn test_replay_from_wal_entries() {
        // Simulate a node restart: create entries, then replay into a fresh log
        let original = ConsensusLog::new();

        for _ in 0..5 {
            original
                .append(CrudOperation::Create {
                    model_path: "/api/products".to_string(),
                    data: serde_json::json!({"name": "Test"}),
                })
                .await;
        }
        original.commit(5);
        original.mark_applied(5);

        // Capture entries as if read from WAL
        let entries = original.entries.read().await.clone();

        // Create a fresh log (simulates restart with empty memory)
        let restored = ConsensusLog::new();
        assert_eq!(restored.commit_index(), 0);

        let count = restored.replay_from_wal_entries(entries).await;
        assert_eq!(count, 5);
        assert_eq!(restored.current_term(), 1);
        // commit_index and applied_index stay at 0 (conservative replay —
        // the cluster will re-establish commit via normal replication)
        assert_eq!(restored.commit_index(), 0);
        assert_eq!(restored.applied_index(), 0);

        // New appends should continue from index 6
        let entry = restored
            .append(CrudOperation::Create {
                model_path: "/api/products".to_string(),
                data: serde_json::json!({"name": "New"}),
            })
            .await;
        assert_eq!(entry.log_id.index, 6);
    }

    #[tokio::test]
    async fn test_replay_empty_wal() {
        let log = ConsensusLog::new();
        let count = log.replay_from_wal_entries(vec![]).await;
        assert_eq!(count, 0);
        assert_eq!(log.commit_index(), 0);

        // Should still work normally
        let entry = log
            .append(CrudOperation::Create {
                model_path: "/api/x".to_string(),
                data: serde_json::json!({}),
            })
            .await;
        assert_eq!(entry.log_id.index, 1);
    }

    #[tokio::test]
    async fn test_term_operations() {
        let log = ConsensusLog::new();
        assert_eq!(log.current_term(), 1);

        let new_term = log.increment_term();
        assert_eq!(new_term, 2);
        assert_eq!(log.current_term(), 2);

        log.set_term(5);
        assert_eq!(log.current_term(), 5);

        // Append in new term
        let entry = log
            .append(CrudOperation::Create {
                model_path: "/api/x".to_string(),
                data: serde_json::json!({}),
            })
            .await;
        assert_eq!(entry.log_id.term, 5);
    }
}
