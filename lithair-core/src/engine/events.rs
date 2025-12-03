//! Event sourcing system with immutable event log

use super::{EngineError, EngineResult, FileStorage, MultiFileEventStore};
use bincode::config::standard;
use bincode::serde::{decode_from_slice, encode_to_vec};
use serde::{Deserialize, Serialize};

/// Base trait for all events in the system
pub trait Event: Send + Sync {
    /// The state type this event can be applied to
    type State;

    /// Apply this event to the given state
    fn apply(&self, state: &mut Self::State);

    /// Optional: provide an idempotence key to uniquely identify this event.
    /// If provided, the engine will use it to detect duplicates instead of hashing the JSON.
    fn idempotence_key(&self) -> Option<String> {
        None
    }

    /// Optional: provide an aggregate id for indexing/filtering (e.g., product id)
    /// Returning a short String (e.g., numeric id to_string) is recommended.
    /// Default: None (engine will not index by aggregate id)
    fn aggregate_id(&self) -> Option<String> {
        None
    }

    /// Serialize this event to JSON for persistence
    ///
    /// Default implementation uses Debug formatting - override for production
    fn to_json(&self) -> String {
        format!(
            "{{\"event_type\": \"{}\", \"timestamp\": \"{}\"}}",
            std::any::type_name::<Self>(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
        )
    }

    /// Deserialize an event from JSON
    ///
    /// This is a placeholder - in a real implementation, you'd use serde
    fn from_json(_json: &str) -> EngineResult<Self>
    where
        Self: Sized,
    {
        Err(EngineError::SerializationError(
            "Event deserialization not implemented".to_string(),
        ))
    }
}

/// A pluggable deserializer that can apply an event to a given state from its JSON payload
pub trait EventDeserializer: Send + Sync {
    type State;
    /// Kind identifier, typically the fully qualified Rust type name used when persisting
    fn event_type(&self) -> &str;
    /// Parse the JSON payload and apply the event to the state
    fn apply_from_json(&self, state: &mut Self::State, payload_json: &str) -> Result<(), String>;
}

/// Standard envelope used to persist events with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    /// Fully-qualified Rust type name or logical kind
    pub event_type: String,
    /// Idempotence identifier (exactly-once key)
    pub event_id: String,
    /// Unix timestamp seconds
    pub timestamp: u64,
    /// Original event JSON payload
    pub payload: String,
    /// Optional aggregate id for fast filtering/indexing (e.g., product id)
    pub aggregate_id: Option<String>,
}

/// Backend storage for EventStore
enum EventStoreBackend {
    /// Single file storage (default, backward compatible)
    Single(FileStorage),
    /// Multi-file storage (one file per aggregate_id)
    /// Boxed to break infinite recursion since MultiFileEventStore contains HashMap<String, EventStore>
    Multi(Box<MultiFileEventStore>),
}

/// Event store for persistent event logging with file storage
pub struct EventStore {
    backend: EventStoreBackend,
    events_count: usize,
    pub(crate) log_verbose: bool,
    pending_since_flush: usize,
    flush_every: usize, // Add flush control
    binary_mode: bool,
    disable_index: bool,
    dedup_persist: bool,
}

impl EventStore {
    /// Create a new event store with file path (single-file mode)
    pub fn new(file_path: &str) -> EngineResult<Self> {
        Self::new_with_config(file_path, false)
    }

    /// Create a new event store with configuration
    pub fn new_with_config(file_path: &str, use_multi_file_store: bool) -> EngineResult<Self> {
        Self::new_with_options(file_path, use_multi_file_store, false)
    }

    /// Create a new event store with full options
    pub fn new_with_options(
        file_path: &str,
        use_multi_file_store: bool,
        binary_mode: bool,
    ) -> EngineResult<Self> {
        let backend = if use_multi_file_store {
            let multi_store = MultiFileEventStore::new(file_path)?;
            EventStoreBackend::Multi(Box::new(multi_store))
        } else {
            let storage = FileStorage::new(file_path)?;
            EventStoreBackend::Single(storage)
        };

        let events_count = match &backend {
            EventStoreBackend::Single(s) => {
                if binary_mode {
                    s.read_all_event_bytes()?.len()
                } else {
                    s.read_all_events()?.len()
                }
            }
            EventStoreBackend::Multi(m) => m.read_all_envelopes()?.len(),
        };

        Ok(Self {
            backend,
            events_count,
            log_verbose: false, // Disabled for performance
            pending_since_flush: 0,
            flush_every: 1,
            binary_mode: binary_mode
                || std::env::var("RS_ENABLE_BINARY")
                    .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                    .unwrap_or(false),
            disable_index: std::env::var("RS_DISABLE_INDEX")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false),
            dedup_persist: !std::env::var("RS_DEDUP_PERSIST")
                .map(|v| v == "0" || v.eq_ignore_ascii_case("false"))
                .unwrap_or(false),
        })
    }

    /// Create a new event store with existing file storage (single-file mode)
    pub fn with_storage(storage: FileStorage) -> EngineResult<Self> {
        let events_count = storage.read_all_events()?.len();

        Ok(Self {
            backend: EventStoreBackend::Single(storage),
            events_count,
            log_verbose: false, // Disabled for performance
            pending_since_flush: 0,
            flush_every: 1,
            binary_mode: std::env::var("RS_ENABLE_BINARY")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false),
            disable_index: std::env::var("RS_DISABLE_INDEX")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false),
            dedup_persist: !std::env::var("RS_DEDUP_PERSIST")
                .map(|v| v == "0" || v.eq_ignore_ascii_case("false"))
                .unwrap_or(false),
        })
    }

    /// Enable or disable binary storage mode
    pub fn set_binary_mode(&mut self, enabled: bool) {
        self.binary_mode = enabled;
    }

    /// Set the number of events to buffer before flushing (0 = manual flush, 1 = every event)
    pub fn set_flush_every(&mut self, count: usize) {
        self.flush_every = count;
    }

    /// Append an event to the persistent log
    pub fn append_event<E: Event>(&mut self, event: &E) -> EngineResult<()> {
        let event_json = event.to_json();
        match &mut self.backend {
            EventStoreBackend::Single(storage) => storage.append_event(&event_json)?,
            EventStoreBackend::Multi(_) => {
                return Err(EngineError::InvalidOperation(
                    "append_event not supported in multi-file mode, use append_envelope".to_string()
                ));
            }
        }
        self.events_count += 1;
        self.pending_since_flush += 1;
        if self.flush_every > 0 && self.pending_since_flush >= self.flush_every {
            self.flush()?;
        }
        Ok(())
    }

    /// Append a raw JSON line (used by AsyncWriter)
    pub fn append_raw_line(&mut self, line: &str) -> EngineResult<()> {
        match &mut self.backend {
            EventStoreBackend::Single(storage) => storage.append_event(line)?,
            EventStoreBackend::Multi(_) => {
                // Multi-file requires envelope parsing to find aggregate_id
                // Try to parse line as envelope
                if let Ok(envelope) = serde_json::from_str::<EventEnvelope>(line) {
                    return self.append_envelope(&envelope);
                }
                return Err(EngineError::InvalidOperation(
                    "append_raw_line with non-envelope data not supported in multi-file mode".to_string()
                ));
            }
        }
        self.events_count += 1;
        self.pending_since_flush += 1;
        // No auto-flush here as AsyncWriter controls flushing
        Ok(())
    }

    /// Append an envelope to the persistent log (preferred)
    pub fn append_envelope(&mut self, envelope: &EventEnvelope) -> EngineResult<()> {
        match &mut self.backend {
            EventStoreBackend::Single(storage) => {
                // Original single-file logic
                let mut offset: u64 = 0;
                let will_index = !self.disable_index && envelope.aggregate_id.is_some();
                if will_index {
                    offset = storage.current_events_size()?;
                }

                if self.binary_mode {
                    let bytes = encode_to_vec(envelope, standard()).map_err(|e| {
                        EngineError::SerializationError(format!(
                            "Failed to serialize envelope (bincode): {}",
                            e
                        ))
                    })?;
                    storage.append_binary_event_bytes(&bytes)?;
                } else {
                    let json = serde_json::to_string(envelope).map_err(|e| {
                        EngineError::SerializationError(format!("Failed to serialize envelope: {}", e))
                    })?;
                    storage.append_event(&json)?;
                }
                if will_index {
                    if let Some(agg) = &envelope.aggregate_id {
                        let _ = storage.append_index_entry(agg, offset);
                    }
                }
            }
            EventStoreBackend::Multi(multi_store) => {
                // Multi-file logic - delegate to MultiFileEventStore
                multi_store.append_envelope(envelope)?;
            }
        }
        self.events_count += 1;
        self.pending_since_flush += 1;
        if self.flush_every > 0 && self.pending_since_flush >= self.flush_every {
            self.flush()?;
        }
        Ok(())
    }

    /// Flush pending events to disk
    pub fn flush(&mut self) -> EngineResult<()> {
        match &mut self.backend {
            EventStoreBackend::Single(storage) => storage.flush_events()?,
            EventStoreBackend::Multi(multi_store) => multi_store.flush_all()?,
        }
        self.pending_since_flush = 0;
        Ok(())
    }

    /// Configure batch settings for optimal performance
    pub fn configure_batching(&mut self, max_batch_size: usize, fsync_on_append: bool) {
        match &mut self.backend {
            EventStoreBackend::Single(storage) => storage.configure_batching(max_batch_size, fsync_on_append),
            EventStoreBackend::Multi(_) => {
                // TODO: Multi-file mode doesn't support configure_batching yet
            }
        }
    }

    /// Force flush any pending events (call before shutdown)
    pub fn force_flush(&mut self) -> EngineResult<()> {
        match &mut self.backend {
            EventStoreBackend::Single(storage) => storage.force_flush()?,
            EventStoreBackend::Multi(multi_store) => multi_store.flush_all()?,
        }
        self.pending_since_flush = 0;
        Ok(())
    }

    /// Get all events from storage
    pub fn get_all_events(&self) -> EngineResult<Vec<String>> {
        match &self.backend {
            EventStoreBackend::Single(storage) => {
                if !self.binary_mode {
                    return storage.read_all_events();
                }
                // Binary mode: read bytes and decode envelopes, then convert to JSON strings for compatibility
                let mut all = Vec::new();
                let bytes_lines = storage.read_all_event_bytes()?;
                for line in bytes_lines {
                    if line.is_empty() {
                        continue;
                    }
                    match decode_from_slice::<EventEnvelope, _>(&line, standard()).map(|(v, _)| v) {
                        Ok(env) => {
                            let json = serde_json::to_string(&env).map_err(|e| {
                                EngineError::SerializationError(format!(
                                    "Failed to reserialize envelope to JSON: {}",
                                    e
                                ))
                            })?;
                            all.push(json);
                        }
                        Err(_) => {
                            // Fallback: attempt to interpret as UTF-8 JSON line (mixed logs)
                            if let Ok(s) = std::str::from_utf8(&line) {
                                if !s.trim().is_empty() {
                                    all.push(s.to_string());
                                }
                            }
                        }
                    }
                }
                Ok(all)
            }
            EventStoreBackend::Multi(multi_store) => {
                // Multi-file mode: get all envelopes and convert to JSON strings
                let envelopes = multi_store.read_all_envelopes()?;
                envelopes
                    .into_iter()
                    .map(|env| {
                        serde_json::to_string(&env)
                            .map_err(|e| EngineError::SerializationError(e.to_string()))
                    })
                    .collect()
            }
        }
    }

    /// Get the number of events in the store
    pub fn event_count(&self) -> usize {
        self.events_count
    }

    /// Save a state snapshot
    pub fn save_snapshot(&self, state_json: &str) -> EngineResult<()> {
        match &self.backend {
            EventStoreBackend::Single(storage) => storage.save_snapshot(state_json),
            EventStoreBackend::Multi(_) => {
                Err(EngineError::InvalidOperation(
                    "save_snapshot not supported in multi-file mode".to_string()
                ))
            }
        }
    }

    /// Save a deduplication event id for durable exactly-once
    pub fn save_dedup_id(&self, event_id: &str) -> EngineResult<()> {
        if !self.dedup_persist {
            return Ok(());
        }
        match &self.backend {
            EventStoreBackend::Single(storage) => storage.append_dedup_id(event_id),
            EventStoreBackend::Multi(multi_store) => multi_store.append_dedup_id(event_id),
        }
    }

    /// Load all persisted deduplication event ids
    pub fn load_dedup_ids(&self) -> EngineResult<Vec<String>> {
        match &self.backend {
            EventStoreBackend::Single(storage) => storage.read_all_dedup_ids(),
            EventStoreBackend::Multi(multi_store) => multi_store.load_dedup_ids(),
        }
    }

    /// Flush the buffered writer and optionally fsync
    pub fn flush_events(&mut self) -> EngineResult<()> {
        match &mut self.backend {
            EventStoreBackend::Single(storage) => storage.flush_events(),
            EventStoreBackend::Multi(multi_store) => multi_store.flush_all(),
        }
    }

    /// Truncate the events log file (compaction)
    pub fn truncate_events(&mut self) -> EngineResult<()> {
        match &mut self.backend {
            EventStoreBackend::Single(storage) => storage.truncate_events()?,
            EventStoreBackend::Multi(_) => {
                return Err(EngineError::InvalidOperation(
                    "truncate_events not supported in multi-file mode".to_string()
                ));
            }
        }
        // Reset the in-memory event count to reflect the empty log
        self.events_count = 0;
        Ok(())
    }

    /// Load a state snapshot
    pub fn load_snapshot(&self) -> EngineResult<Option<String>> {
        match &self.backend {
            EventStoreBackend::Single(storage) => storage.load_snapshot(),
            EventStoreBackend::Multi(_) => {
                Err(EngineError::InvalidOperation(
                    "load_snapshot not supported in multi-file mode".to_string()
                ))
            }
        }
    }
}

/// Event stream for real-time event processing
pub struct EventStream {
    // TODO: Implement event streaming
}

impl EventStream {
    pub fn new() -> Self {
        Self {
            // TODO: Initialize event stream
        }
    }
}

impl Default for EventStream {
    fn default() -> Self {
        Self::new()
    }
}
