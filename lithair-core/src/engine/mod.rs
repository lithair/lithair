//! Core engine module
//!
//! This module contains the core Lithair engine types and traits.

use crate::config::LithairConfig;
use crate::model::ModelSpec;
use crate::model_inspect::Inspectable;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;
use std::sync::{Arc, RwLock};

// Re-export useful types
pub mod async_writer;
pub mod events;
pub mod lockfree_engine;
pub mod multi_file_store;
pub mod persistence;
pub mod persistence_optimized;
pub mod relations;
pub mod scc2_engine;
pub mod snapshot;
pub mod state;

// Re-export types
pub use async_writer::{AsyncWriter, DurabilityMode, WriteEvent};
pub use events::{
    ChainError, ChainVerificationResult, Event, EventDeserializer, EventEnvelope, EventStore,
};
pub use multi_file_store::MultiFileEventStore;
pub use persistence::{DatabaseStats, FileStorage};
pub use persistence_optimized::{AsyncEventWriter, OptimizedPersistenceConfig};
pub use relations::{AutoJoiner, DataSource, RelationRegistry};
pub use scc2_engine::{Scc2Engine, Scc2EngineConfig, VersionedEntry};
pub use snapshot::{RecoveryContext, Snapshot, SnapshotMetadata, SnapshotStats, SnapshotStore};
pub use state::StateEngine;

/// The core application trait that users must implement
pub trait RaftstoneApplication: Send + Sync + Sized + 'static {
    /// The state type managed by the application
    type State: Clone + Send + Sync + Default + Serialize + Inspectable + ModelSpec + 'static;

    /// The command type (optional, for CQRS)
    type Command: Send + Sync + 'static;

    /// The event type that modifies the state
    type Event: Event<State = Self::State> + Serialize + for<'de> Deserialize<'de> + 'static;

    /// Create initial state
    fn initial_state() -> Self::State;

    /// Get application routes
    fn routes() -> Vec<crate::http::Route<Self::State>>;

    /// Get command routes (for write operations)
    fn command_routes() -> Vec<crate::http::CommandRoute<Self>>;

    /// Get event deserializers
    fn event_deserializers() -> Vec<Box<dyn EventDeserializer<State = Self::State>>>;

    /// Startup hook (optional)
    fn on_startup(state: &mut Self::State) -> Result<()> {
        let _ = state; // Suppress unused warning
        Ok(())
    }
}

/// Engine configuration
#[derive(Debug, Clone, Default)]
pub struct EngineConfig {
    pub raft_config: LithairConfig,
    pub event_log_path: String,
    pub flush_every: usize,
    pub fsync_on_append: bool,
    pub snapshot_every: u64,
    pub use_multi_file_store: bool,
}

/// Engine result type
pub type EngineResult<T> = Result<T, EngineError>;

/// Engine error type
#[derive(thiserror::Error, Debug)]
pub enum EngineError {
    #[error("State not found")]
    NotFound,
    #[error("Persistence error: {0}")]
    PersistenceError(String),
    #[error("Serialization error: {0}")]
    SerializationError(String),
    #[error("Concurrency error: {0}")]
    ConcurrencyError(String),
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
    #[error("Internal engine error: {0}")]
    EngineError(String),
    #[error("Duplicate event: {0}")]
    DuplicateEvent(String),
    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    SerdeJson(#[from] serde_json::Error),
}

impl From<crate::Error> for EngineError {
    fn from(err: crate::Error) -> Self {
        match err {
            crate::Error::EngineError(msg) => EngineError::EngineError(msg),
            _ => EngineError::EngineError(err.to_string()),
        }
    }
}

/// The Lithair Engine
pub struct Engine<A: RaftstoneApplication> {
    /// The event store
    event_store: Option<Arc<RwLock<EventStore>>>,

    /// The state storage backend
    state_storage: StateStorage<A::State>,

    /// The relation registry for auto-joins
    pub relations: RelationRegistry,

    _phantom: PhantomData<A>,
}

/// State storage backend variants
enum StateStorage<S> {
    /// Simple RwLock-based state (good for small/medium scale)
    RwLock(Arc<RwLock<S>>),
    /// SCC2-based state engine (best for high performance)
    Scc2(Arc<Scc2Engine<S>>),
}

impl<A: RaftstoneApplication> Engine<A> {
    /// Create a new Lithair engine
    pub fn new(config: EngineConfig) -> Result<Self> {
        Self::new_with_deserializers(config, vec![])
    }

    /// Create a new Lithair engine with custom event deserializers
    pub fn new_with_deserializers(
        config: EngineConfig,
        _deserializers: Vec<Box<dyn EventDeserializer<State = A::State>>>,
    ) -> Result<Self> {
        // Initialize persistence
        let data_dir = if !config.event_log_path.is_empty() {
            config.event_log_path.clone()
        } else {
            config.raft_config.storage.data_dir.clone()
        };

        let use_multi_file = config.use_multi_file_store
            || std::env::var("RS_MULTI_FILE")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);

        let use_binary = std::env::var("RS_ENABLE_BINARY")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let mut event_store =
            EventStore::new_with_options(&data_dir, use_multi_file, use_binary)
                .map_err(|e| anyhow::anyhow!("Failed to initialize event store: {}", e))?;

        // Configure event store based on config
        if config.flush_every > 0 {
            event_store.set_flush_every(config.flush_every);
        }
        // Note: fsync_on_append is configured via configure_batching usually, or not exposed on EventStore directly
        // except via configure_batching.

        // Deserializer registration is skipped here due to a Box vs Arc type mismatch.
        // Standard Event trait deserialization via serde_json::from_str works without
        // explicit registration for simple payloads. Custom events that require registered
        // deserializers may need an alternative replay path.

        let event_store_arc = Arc::new(RwLock::new(event_store));

        // Choose state storage backend
        let use_scc2 = std::env::var("RS_USE_SCC2")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(true); // Default to true

        let state_storage = if use_scc2 {
            let scc_config = Scc2EngineConfig {
                verbose_logging: config.raft_config.logging.level == "debug"
                    || config.raft_config.logging.level == "trace",
                enable_snapshots: true,
                snapshot_interval: if config.snapshot_every > 0 {
                    config.snapshot_every
                } else {
                    1000
                },
                enable_deduplication: true,
                auto_persist_writes: true,
                force_immediate_persistence: false,
            };

            let scc_engine = Scc2Engine::new(event_store_arc.clone(), scc_config)
                .map_err(|e| anyhow::anyhow!("Failed to initialize SCC2 engine: {}", e))?;

            // Replay events
            scc_engine
                .replay_events::<A::Event>()
                .map_err(|e| anyhow::anyhow!("Failed to replay events: {}", e))?;

            StateStorage::Scc2(Arc::new(scc_engine))
        } else {
            let state = A::State::default();
            StateStorage::RwLock(Arc::new(RwLock::new(state)))
        };

        Ok(Self {
            event_store: Some(event_store_arc),
            state_storage,
            relations: RelationRegistry::new(),
            _phantom: PhantomData,
        })
    }

    /// Get a read-only reference to the state
    pub fn read_state<F, R>(&self, aggregate_id: &str, f: F) -> Option<R>
    where
        F: FnOnce(&A::State) -> R,
    {
        match &self.state_storage {
            StateStorage::RwLock(lock) => {
                let guard = lock.read().ok()?;
                Some(f(&guard))
            }
            StateStorage::Scc2(scc) => scc.read(aggregate_id, f),
        }
    }

    /// Apply a command (event) to the state
    pub fn apply_event(&self, key: String, event: A::Event) -> EngineResult<()> {
        self.apply_event_internal(key, event)
    }

    fn apply_event_internal(&self, key: String, event: A::Event) -> EngineResult<()> {
        match &self.state_storage {
            StateStorage::RwLock(lock) => {
                let mut guard =
                    lock.write().map_err(|_| EngineError::EngineError("Lock poisoned".into()))?;
                event.apply(&mut guard);
                Ok(())
            }
            StateStorage::Scc2(scc) => {
                let handle = tokio::runtime::Handle::try_current()
                    .map_err(|_| EngineError::EngineError("No Tokio runtime available".into()))?;
                tokio::task::block_in_place(|| {
                    handle.block_on(async { scc.apply_event(key, event, true).await })
                })
                .map(|_| ())
                .map_err(EngineError::from)
            }
        }
    }

    /// Manually write state (advanced usage)
    pub fn write_state<F, R>(&self, aggregate_id: &str, f: F) -> EngineResult<R>
    where
        F: FnOnce(&mut A::State) -> R,
    {
        match &self.state_storage {
            StateStorage::RwLock(lock) => {
                let mut guard =
                    lock.write().map_err(|_| EngineError::EngineError("Lock poisoned".into()))?;
                Ok(f(&mut guard))
            }
            StateStorage::Scc2(scc) => {
                scc.update_entry_volatile(aggregate_id, f).ok_or(EngineError::NotFound)
            }
        }
    }

    /// Register a relation data source
    pub fn register_relation(&self, collection: &str, source: Arc<dyn DataSource>) {
        self.relations.register(collection, source);
    }

    /// Get the relation registry
    pub fn relations(&self) -> &RelationRegistry {
        &self.relations
    }

    /// Get the event store (useful for tests and advanced usage)
    pub fn event_store(&self) -> Option<Arc<RwLock<EventStore>>> {
        self.event_store.clone()
    }

    /// Flush pending writes
    pub fn flush(&self) -> EngineResult<()> {
        match &self.state_storage {
            StateStorage::RwLock(_) => Ok(()),
            StateStorage::Scc2(scc) => {
                let handle = tokio::runtime::Handle::try_current()
                    .map_err(|_| EngineError::EngineError("No Tokio runtime available".into()))?;
                tokio::task::block_in_place(|| handle.block_on(async { scc.flush().await }))
                    .map_err(EngineError::from)
            }
        }
    }

    /// Trigger a manual state snapshot
    pub fn save_state_snapshot(&self) -> EngineResult<()> {
        match &self.state_storage {
            StateStorage::RwLock(_) => Err(EngineError::InvalidOperation(
                "Snapshots not supported for RwLock backend yet".into(),
            )),
            StateStorage::Scc2(scc) => scc.snapshot().map_err(EngineError::from),
        }
    }

    /// Compact event log after snapshot (truncate)
    pub fn compact_after_snapshot(&self) -> EngineResult<()> {
        if let Some(store) = &self.event_store {
            store
                .write()
                .map_err(|_| EngineError::EngineError("Event store lock poisoned".into()))?
                .truncate_events()
        } else {
            Err(EngineError::InvalidOperation("No event store configured".into()))
        }
    }

    /// Shutdown the engine
    pub fn shutdown(&self) -> EngineResult<()> {
        self.flush()
    }
}
