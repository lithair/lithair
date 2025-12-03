//! Lock-Free Ultra-Performance Engine for Lithair
//!
//! This module implements a lock-free, sharded architecture for maximum throughput
//! and minimal latency. Designed for handling thousands of concurrent connections.
//!
//! # Architecture
//!
//! - Lock-free atomic state management
//! - Sharding by hash for zero contention
//! - Single writer per shard for simplicity
//!

use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    hash::{Hash, Hasher},
    sync::{
        atomic::{AtomicPtr, AtomicU64, Ordering},
        mpsc, Arc,
    },
    thread,
};

use super::{EngineError, EngineResult, Event};

/// Number of shards for optimal performance
/// Should be a power of 2 for efficient modulo operations
const DEFAULT_SHARD_COUNT: usize = 16;

/// Versioned state for MVCC-style concurrency
#[derive(Debug)]
pub struct VersionedState<S> {
    pub version: u64,
    pub state: S,
    pub timestamp: std::time::Instant,
}

impl<S> VersionedState<S> {
    pub fn new(state: S, version: u64) -> Self {
        Self { version, state, timestamp: std::time::Instant::now() }
    }
}

/// Command to be processed by a shard
#[derive(Debug, Clone)]
pub struct ShardCommand<E: Event> {
    pub event: E,
    pub shard_key: String,
    pub response_sender: Option<mpsc::Sender<EngineResult<()>>>,
}

/// Lock-free shard with single writer thread
pub struct LockFreeShard<S, E>
where
    S: Clone + Send + 'static,
    E: Event<State = S> + Send + 'static,
{
    /// Atomic pointer to current versioned state
    state: Arc<AtomicPtr<VersionedState<S>>>,
    /// Version counter for MVCC
    _version_counter: Arc<AtomicU64>,
    /// Command channel sender
    command_sender: mpsc::Sender<ShardCommand<E>>,
    /// Worker thread handle
    _worker_handle: Option<thread::JoinHandle<()>>,
    /// Shard identifier
    shard_id: usize,
}

impl<S, E> Drop for LockFreeShard<S, E>
where
    S: Clone + Send + 'static,
    E: Event<State = S> + Send + 'static,
{
    fn drop(&mut self) {
        println!("🔄 Shutting down lock-free shard {}...", self.shard_id);
        // Worker threads will shut down when command channels are dropped
    }
}

impl<S, E> LockFreeShard<S, E>
where
    S: Clone + Send + 'static,
    E: Event<State = S> + Send + 'static,
{
    /// Create a new lock-free shard
    pub fn new(initial_state: S, shard_id: usize) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<ShardCommand<E>>();

        // Initialize versioned state
        let initial_versioned = Box::new(VersionedState::new(initial_state, 0));
        let state_ptr = Arc::new(AtomicPtr::new(Box::into_raw(initial_versioned)));
        let version_counter = Arc::new(AtomicU64::new(1));

        // Clone Arc references for worker thread
        let state_ptr_clone = Arc::clone(&state_ptr);
        let version_counter_clone = Arc::clone(&version_counter);

        let worker_handle = thread::spawn(move || {
            Self::worker_thread(cmd_rx, state_ptr_clone, version_counter_clone, shard_id);
        });

        Self {
            state: state_ptr,
            _version_counter: version_counter,
            command_sender: cmd_tx,
            _worker_handle: Some(worker_handle),
            shard_id,
        }
    }

    /// Read current state (lock-free, zero contention)
    pub fn read_state<R>(&self, reader: impl FnOnce(&S) -> R) -> R {
        // Load current state pointer atomically
        let state_ptr = self.state.load(Ordering::Acquire);

        // SAFETY: The pointer is valid as long as we don't modify it
        // The single writer thread ensures consistency
        let versioned_state = unsafe { &*state_ptr };

        reader(&versioned_state.state)
    }

    /// Get current state version
    pub fn get_version(&self) -> u64 {
        let state_ptr = self.state.load(Ordering::Acquire);
        let versioned_state = unsafe { &*state_ptr };
        versioned_state.version
    }

    /// Submit command for processing (async)
    pub fn submit_command(&self, event: E, shard_key: String) -> EngineResult<()> {
        let command = ShardCommand { event, shard_key, response_sender: None };

        self.command_sender
            .send(command)
            .map_err(|_| EngineError::ConcurrencyError("Shard worker thread died".to_string()))?;

        Ok(())
    }

    /// Submit command and wait for response (sync)
    pub fn submit_command_sync(&self, event: E, shard_key: String) -> EngineResult<()> {
        let (response_tx, response_rx) = mpsc::channel();

        let command = ShardCommand { event, shard_key, response_sender: Some(response_tx) };

        self.command_sender
            .send(command)
            .map_err(|_| EngineError::ConcurrencyError("Shard worker thread died".to_string()))?;

        response_rx
            .recv()
            .map_err(|_| EngineError::ConcurrencyError("Response channel closed".to_string()))?
    }

    /// Single writer thread for this shard
    fn worker_thread(
        cmd_rx: mpsc::Receiver<ShardCommand<E>>,
        state_ptr: Arc<AtomicPtr<VersionedState<S>>>,
        version_counter: Arc<AtomicU64>,
        shard_id: usize,
    ) {
        println!("🚀 Lock-free shard {} worker started", shard_id);

        while let Ok(command) = cmd_rx.recv() {
            println!("🔧 Shard {} processing command for key: {}", shard_id, command.shard_key);

            // Load current state
            let current_ptr = state_ptr.load(Ordering::Acquire);
            let current_state = unsafe { &*current_ptr };

            // Clone and apply event
            let mut new_state = current_state.state.clone();
            let label =
                if std::any::type_name::<S>().contains("BlogState") { "articles" } else { "items" };
            println!("   📝 Before apply: state has {} items", label);

            command.event.apply(&mut new_state);
            println!("   ✅ After apply: event applied successfully");

            // Create new versioned state
            let new_version = version_counter.fetch_add(1, Ordering::AcqRel);
            let new_versioned = Box::new(VersionedState::new(new_state, new_version));
            let new_ptr = Box::into_raw(new_versioned);

            // Atomic swap to new state
            let old_ptr = state_ptr.swap(new_ptr, Ordering::AcqRel);
            println!("   🔄 State swapped to version {}", new_version);

            // Clean up old state (after a delay for safety)
            // Note: We need to ensure Send safety for the cleanup thread
            let old_ptr_addr = old_ptr as usize;
            thread::spawn(move || {
                thread::sleep(std::time::Duration::from_millis(100));
                let ptr = old_ptr_addr as *mut VersionedState<S>;
                unsafe { drop(Box::from_raw(ptr)) };
            });

            // Send success response if requested
            if let Some(sender) = command.response_sender {
                let _ = sender.send(Ok(()));
            }
        }

        println!("🔄 Lock-free shard {} worker shutting down", shard_id);
    }
}

/// Ultra-performance lock-free engine with sharding
pub struct LockFreeEngine<S, E: Event<State = S>>
where
    S: Clone + Send + 'static,
    E: Send + 'static,
{
    /// Array of lock-free shards
    shards: Vec<LockFreeShard<S, E>>,

    /// Number of shards
    shard_count: usize,
}

impl<S, E> LockFreeEngine<S, E>
where
    S: Clone + Send + 'static,
    E: Event<State = S> + Send + 'static,
{
    /// Create new lock-free engine with specified shard count
    pub fn new(initial_state: S, shard_count: Option<usize>) -> Self {
        let shard_count = shard_count.unwrap_or(DEFAULT_SHARD_COUNT);

        println!("🚀 Initializing lock-free engine with {} shards", shard_count);

        let mut shards = Vec::with_capacity(shard_count);

        for shard_id in 0..shard_count {
            let shard = LockFreeShard::new(initial_state.clone(), shard_id);
            shards.push(shard);
        }

        Self { shards, shard_count }
    }

    /// Hash key to determine shard
    fn hash_to_shard(&self, key: &str) -> usize {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        (hasher.finish() as usize) % self.shard_count
    }

    /// Read from specific shard (lock-free)
    pub fn read<R>(&self, key: &str, reader: impl FnOnce(&S) -> R) -> R {
        let shard_id = self.hash_to_shard(key);
        println!("🔍 LockFreeEngine: Reading key '{}' from shard {}", key, shard_id);
        self.shards[shard_id].read_state(reader)
    }

    /// Write to specific shard (async)
    pub fn write(&self, key: String, event: E) -> EngineResult<()> {
        let shard_id = self.hash_to_shard(&key);
        self.shards[shard_id].submit_command(event, key)
    }

    /// Write to specific shard (sync)
    pub fn write_sync(&self, key: String, event: E) -> EngineResult<()> {
        let shard_id = self.hash_to_shard(&key);
        println!("🔧 LockFreeEngine: Writing key '{}' to shard {}", key, shard_id);
        let result = self.shards[shard_id].submit_command_sync(event, key);
        println!("🔧 LockFreeEngine: Write result: {:?}", result);
        result
    }

    /// Get statistics about all shards
    pub fn get_stats(&self) -> HashMap<String, u64> {
        let mut stats = HashMap::new();

        for (i, shard) in self.shards.iter().enumerate() {
            let version = shard.get_version();
            stats.insert(format!("shard_{}_version", i), version);
        }

        stats.insert("total_shards".to_string(), self.shard_count as u64);
        stats
    }
}

/// Drop implementation to clean up worker threads
impl<S, E> Drop for LockFreeEngine<S, E>
where
    S: Clone + Send + 'static,
    E: Event<State = S> + Send + 'static,
{
    fn drop(&mut self) {
        println!("🔄 Shutting down lock-free engine...");
        // Worker threads will shut down when command channels are dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, Default)]
    struct TestState {
        counter: u64,
        data: HashMap<String, String>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    enum TestEvent {
        Increment { amount: u64 },
        SetData { key: String, value: String },
    }

    impl Event for TestEvent {
        type State = TestState;

        fn apply(&self, state: &mut Self::State) {
            match self {
                TestEvent::Increment { amount } => {
                    state.counter += amount;
                }
                TestEvent::SetData { key, value } => {
                    state.data.insert(key.clone(), value.clone());
                }
            }
        }
    }

    #[test]
    fn test_lock_free_shard() {
        let shard = LockFreeShard::new(TestState::default(), 0);

        // Test read
        let initial_counter = shard.read_state(|state| state.counter);
        assert_eq!(initial_counter, 0);

        // Test write
        shard
            .submit_command_sync(TestEvent::Increment { amount: 1 }, "test_key".to_string())
            .unwrap();

        let new_counter = shard.read_state(|state| state.counter);
        assert_eq!(new_counter, 1);
    }

    #[test]
    fn test_lock_free_engine() {
        let engine = LockFreeEngine::new(TestState::default(), Some(4));

        // Test sharded operations
        engine
            .write_sync("key1".to_string(), TestEvent::Increment { amount: 1 })
            .unwrap();
        engine
            .write_sync(
                "key2".to_string(),
                TestEvent::SetData { key: "test".to_string(), value: "value".to_string() },
            )
            .unwrap();

        // Read from different shards
        let counter = engine.read("key1", |state| state.counter);
        let data = engine.read("key2", |state| state.data.get("test").cloned());

        assert_eq!(counter, 1);
        assert_eq!(data, Some("value".to_string()));

        // Check stats
        let stats = engine.get_stats();
        assert_eq!(stats.get("total_shards"), Some(&4));
    }
}
