# StateEngine API Documentation

## Overview

`StateEngine<S>` is a thread-safe, high-performance state container that provides concurrent read access with exclusive write access. It's the foundation for Lithair's memory-first architecture.

## Core API

### Creating a StateEngine

```rust
use lithair_core::engine::StateEngine;
use std::collections::HashMap;

// Create with initial state
let engine = StateEngine::new(HashMap::new());
```

### Read Operations

#### `with_state<F, R>(&self, f: F) -> EngineResult<R>`

Execute a read-only operation with multiple concurrent readers allowed.

```rust
let count = engine.with_state(|state| state.len())?;
```

#### `try_with_state<F, R>(&self, f: F) -> Option<R>`

Try to read without blocking. Returns `None` if lock cannot be acquired immediately.

```rust
if let Some(count) = engine.try_with_state(|state| state.len()) {
    println!("Count: {}", count);
}
```

#### `snapshot(&self) -> EngineResult<S>` where `S: Clone`

Create a complete clone of the current state.

```rust
let snapshot = engine.snapshot()?;
```

### Write Operations

#### `with_state_mut<F, R>(&self, f: F) -> EngineResult<R>`

Execute a mutable operation with exclusive access.

```rust
engine.with_state_mut(|state| {
    state.insert(key, value);
})?;
```

#### `try_with_state_mut<F, R>(&self, f: F) -> Option<R>`

Try to write without blocking. Returns `None` if lock cannot be acquired immediately.

```rust
if let Some(_) = engine.try_with_state_mut(|state| state.clear()) {
    println!("State cleared");
}
```

#### `replace_state(&self, new_state: S) -> EngineResult<S>`

Replace the entire state, returning the old state.

```rust
let old_state = engine.replace_state(new_state)?;
```

### Direct Lock Access

For advanced use cases requiring multiple operations under the same lock:

#### `read(&self) -> EngineResult<RwLockReadGuard<'_, S>>`

Get a read guard for direct access. **Caution:** Holding the lock too long blocks writers.

```rust
let guard = engine.read()?;
let value = guard.get(&key);
```

#### `write(&self) -> EngineResult<RwLockWriteGuard<'_, S>>`

Get a write guard for direct access. **Caution:** Holding the lock too long blocks all access.

```rust
let mut guard = engine.write()?;
guard.insert(key, value);
```

## Usage in PersistentSessionStore

### Initialization with Loading

```rust
pub fn new(data_path: PathBuf) -> Result<Self> {
    // Create engine with empty state
    let engine = StateEngine::new(HashMap::new());
    
    // Load from disk if exists
    let sessions_file = data_path.join("sessions.json");
    if sessions_file.exists() {
        let data = std::fs::read_to_string(&sessions_file)?;
        let sessions: HashMap<String, SessionModel> = serde_json::from_str(&data)?;
        
        // Replace state with loaded data
        engine.replace_state(sessions)?;
        
        log::info!("📂 Loaded {} sessions", count);
    }
    
    Ok(Self {
        engine: Arc::new(engine),
        data_path,
    })
}
```

### Read Operations

```rust
async fn get(&self, session_id: &str) -> Result<Option<Session>> {
    let result = self.engine.with_state(|state| {
        state.get(session_id).cloned()
    })?;
    
    match result {
        Some(model) => Ok(Some(Self::from_model(&model)?)),
        None => Ok(None),
    }
}
```

### Write Operations with Persistence

```rust
async fn set(&self, session: Session) -> Result<()> {
    let model = Self::to_model(&session)?;
    let session_id = model.id.clone();
    
    // Update in-memory state
    self.engine.with_state_mut(|state| {
        state.insert(session_id, model);
    })?;
    
    // Persist to disk
    self.save_to_disk()?;
    
    Ok(())
}

async fn delete(&self, session_id: &str) -> Result<()> {
    // Remove from memory
    self.engine.with_state_mut(|state| {
        state.remove(session_id);
    })?;
    
    // Persist to disk
    self.save_to_disk()?;
    
    Ok(())
}
```

### Bulk Operations

```rust
async fn cleanup_expired(&self) -> Result<usize> {
    let now = Utc::now();
    let mut removed = 0;
    
    self.engine.with_state_mut(|state| {
        state.retain(|_, model| {
            if model.expires_at <= now {
                removed += 1;
                false
            } else {
                true
            }
        });
    })?;
    
    if removed > 0 {
        self.save_to_disk()?;
    }
    
    Ok(removed)
}
```

## Performance Characteristics

- **Read Operations**: Lock-free for multiple concurrent readers
- **Write Operations**: Exclusive lock, blocks all other access
- **Memory**: Shared via `Arc`, zero-copy cloning of the `StateEngine` itself
- **Snapshots**: Full clone of state data (use sparingly for large states)

## Best Practices

1. **Keep Critical Sections Short**: Execute operations quickly inside closures
2. **Avoid Holding Guards**: Use `with_state` and `with_state_mut` instead of direct guards
3. **Batch Operations**: Combine multiple state changes in a single `with_state_mut` call
4. **Use Try Methods**: For non-blocking operations when appropriate
5. **Clone the Engine, Not the State**: Use `engine.share()` or `.clone()` for multiple handles

## Error Handling

All operations return `EngineResult<T>` which can fail with `EngineError::ConcurrencyError` if:
- The lock is poisoned (a thread panicked while holding the lock)
- The lock cannot be acquired (rare, indicates a bug)

```rust
match engine.with_state(|state| state.len()) {
    Ok(count) => println!("Count: {}", count),
    Err(e) => eprintln!("Error: {:?}", e),
}
```
