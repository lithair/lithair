//! Thread-safe in-memory state management
//!
//! This module provides high-performance, concurrent access to application state
//! using RwLock for optimal read performance.

use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use super::{EngineError, EngineResult};

/// Thread-safe state container with concurrent read access
///
/// The StateEngine provides efficient access to application state with:
/// - Multiple concurrent readers
/// - Exclusive writer access
/// - Panic-safe operations
///
/// # Example
///
/// ```rust
/// use lithair_core::engine::StateEngine;
///
/// let engine = StateEngine::new(vec![1, 2, 3]);
///
/// // Multiple readers can access simultaneously
/// let count = engine.with_state(|state| state.len()).unwrap();
///
/// // Writers get exclusive access
/// engine.with_state_mut(|state| {
///     state.push(4);
/// }).unwrap();
/// ```
pub struct StateEngine<S> {
    state: Arc<RwLock<S>>,
}

impl<S> StateEngine<S> {
    /// Create a new state engine with the given initial state
    pub fn new(initial_state: S) -> Self {
        Self { state: Arc::new(RwLock::new(initial_state)) }
    }

    /// Execute a read-only operation on the state
    ///
    /// This allows multiple concurrent readers and is very fast.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use lithair_core::engine::StateEngine;
    /// let engine = StateEngine::new(vec![1, 2, 3]);
    /// let len = engine.with_state(|state| state.len()).unwrap();
    /// assert_eq!(len, 3);
    /// ```
    pub fn with_state<F, R>(&self, f: F) -> EngineResult<R>
    where
        F: FnOnce(&S) -> R,
    {
        let guard = self.state.read().map_err(|e| {
            EngineError::ConcurrencyError(format!("Failed to acquire read lock: {}", e))
        })?;

        Ok(f(&*guard))
    }

    /// Execute a mutable operation on the state
    ///
    /// This requires exclusive access and will block other readers/writers.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use lithair_core::engine::StateEngine;
    /// let engine = StateEngine::new(vec![1, 2, 3]);
    /// engine.with_state_mut(|state| {
    ///     state.push(4);
    /// }).unwrap();
    /// ```
    pub fn with_state_mut<F, R>(&self, f: F) -> EngineResult<R>
    where
        F: FnOnce(&mut S) -> R,
    {
        let mut guard = self.state.write().map_err(|e| {
            EngineError::ConcurrencyError(format!("Failed to acquire write lock: {}", e))
        })?;

        Ok(f(&mut *guard))
    }

    /// Try to execute a read-only operation without blocking
    ///
    /// Returns `None` if the lock cannot be acquired immediately.
    pub fn try_with_state<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&S) -> R,
    {
        if let Ok(guard) = self.state.try_read() {
            Some(f(&*guard))
        } else {
            None
        }
    }

    /// Try to execute a mutable operation without blocking
    ///
    /// Returns `None` if the lock cannot be acquired immediately.
    pub fn try_with_state_mut<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&mut S) -> R,
    {
        if let Ok(mut guard) = self.state.try_write() {
            Some(f(&mut *guard))
        } else {
            None
        }
    }

    /// Get a read guard for direct access to the state
    ///
    /// Use this when you need to hold the lock for multiple operations.
    /// Be careful not to hold the lock too long as it will block writers.
    pub fn read(&self) -> EngineResult<RwLockReadGuard<'_, S>> {
        self.state.read().map_err(|e| {
            EngineError::ConcurrencyError(format!("Failed to acquire read lock: {}", e))
        })
    }

    /// Get a write guard for direct access to the state
    ///
    /// Use this when you need to hold the lock for multiple operations.
    /// Be very careful not to hold the lock too long as it will block all other access.
    pub fn write(&self) -> EngineResult<RwLockWriteGuard<'_, S>> {
        self.state.write().map_err(|e| {
            EngineError::ConcurrencyError(format!("Failed to acquire write lock: {}", e))
        })
    }

    /// Clone the current state
    ///
    /// This is useful for creating snapshots, but can be expensive for large states.
    pub fn snapshot(&self) -> EngineResult<S>
    where
        S: Clone,
    {
        self.with_state(|state| state.clone())
    }

    /// Replace the entire state with a new value
    ///
    /// This is useful for restoring from snapshots or major state updates.
    pub fn replace_state(&self, new_state: S) -> EngineResult<S> {
        self.with_state_mut(|state| std::mem::replace(state, new_state))
    }
}

impl<S> Clone for StateEngine<S> {
    fn clone(&self) -> Self {
        Self { state: Arc::clone(&self.state) }
    }
}

// TODO: Implement more advanced features
impl<S> StateEngine<S> {
    /// Create a state engine that shares the same underlying state
    ///
    /// This is useful for creating multiple handles to the same state.
    pub fn share(&self) -> Self {
        self.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    #[test]
    fn test_basic_operations() {
        let engine = StateEngine::new(vec![1, 2, 3]);

        // Read operation
        let len = engine.with_state(|state| state.len()).unwrap();
        assert_eq!(len, 3);

        // Write operation
        engine
            .with_state_mut(|state| {
                state.push(4);
            })
            .unwrap();

        let len = engine.with_state(|state| state.len()).unwrap();
        assert_eq!(len, 4);
    }

    #[test]
    fn test_concurrent_reads() {
        let engine = Arc::new(StateEngine::new(vec![1, 2, 3, 4, 5]));
        let counter = Arc::new(AtomicUsize::new(0));

        let mut handles = vec![];

        // Spawn multiple reader threads
        for _ in 0..10 {
            let engine_clone = Arc::clone(&engine);
            let counter_clone = Arc::clone(&counter);

            let handle = thread::spawn(move || {
                for _ in 0..100 {
                    let sum = engine_clone.with_state(|state| state.iter().sum::<i32>()).unwrap();

                    assert_eq!(sum, 15); // 1+2+3+4+5
                    counter_clone.fetch_add(1, Ordering::Relaxed);
                }
            });

            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }

        // All reads should have completed
        assert_eq!(counter.load(Ordering::Relaxed), 1000);
    }

    #[test]
    fn test_snapshot() {
        let engine = StateEngine::new(vec![1, 2, 3]);

        let snapshot = engine.snapshot().unwrap();
        assert_eq!(snapshot, vec![1, 2, 3]);

        // Modify original
        engine
            .with_state_mut(|state| {
                state.push(4);
            })
            .unwrap();

        // Snapshot should be unchanged
        assert_eq!(snapshot, vec![1, 2, 3]);

        // Original should be modified
        let current = engine.snapshot().unwrap();
        assert_eq!(current, vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_try_operations() {
        let engine = StateEngine::new(42);

        // Should succeed when lock is available
        let value = engine.try_with_state(|state| *state).unwrap();
        assert_eq!(value, 42);

        let updated = engine
            .try_with_state_mut(|state| {
                *state = 100;
                *state
            })
            .unwrap();
        assert_eq!(updated, 100);
    }

    #[test]
    fn test_replace_state() {
        let engine = StateEngine::new(vec![1, 2, 3]);

        let old_state = engine.replace_state(vec![4, 5, 6]).unwrap();
        assert_eq!(old_state, vec![1, 2, 3]);

        let new_state = engine.snapshot().unwrap();
        assert_eq!(new_state, vec![4, 5, 6]);
    }
}
