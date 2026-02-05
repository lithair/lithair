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
    /// SHA256 hash of this event's content (for tamper detection)
    /// Computed from: event_type + event_id + timestamp + payload + previous_hash
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_hash: Option<String>,
    /// SHA256 hash of the previous event in the chain (None for genesis event)
    /// Forms a hash chain for tamper-evident audit trail
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_hash: Option<String>,
}

impl EventEnvelope {
    /// Create a new envelope with hash chain support
    pub fn new(
        event_type: String,
        event_id: String,
        timestamp: u64,
        payload: String,
        aggregate_id: Option<String>,
        previous_hash: Option<String>,
    ) -> Self {
        let mut envelope = Self {
            event_type,
            event_id,
            timestamp,
            payload,
            aggregate_id,
            event_hash: None,
            previous_hash,
        };
        // Compute and set the hash after all fields are populated
        envelope.event_hash = Some(envelope.compute_hash());
        envelope
    }

    /// Compute SHA256 hash of this event's content
    /// The hash covers all fields except event_hash itself
    pub fn compute_hash(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();

        // Include all fields that should be protected
        hasher.update(self.event_type.as_bytes());
        hasher.update(b"|");
        hasher.update(self.event_id.as_bytes());
        hasher.update(b"|");
        hasher.update(self.timestamp.to_le_bytes());
        hasher.update(b"|");
        hasher.update(self.payload.as_bytes());
        hasher.update(b"|");
        if let Some(ref agg) = self.aggregate_id {
            hasher.update(agg.as_bytes());
        }
        hasher.update(b"|");
        if let Some(ref prev) = self.previous_hash {
            hasher.update(prev.as_bytes());
        }

        let result = hasher.finalize();
        hex::encode(result)
    }

    /// Verify that this event's hash is valid
    pub fn verify_hash(&self) -> bool {
        match &self.event_hash {
            Some(stored_hash) => {
                let computed = self.compute_hash();
                stored_hash == &computed
            }
            None => true, // Legacy events without hash are considered valid
        }
    }

    /// Check if this event links correctly to a previous event
    pub fn links_to(&self, previous: &EventEnvelope) -> bool {
        match (&self.previous_hash, &previous.event_hash) {
            (Some(prev), Some(prev_hash)) => prev == prev_hash,
            (None, None) => true, // Both legacy
            (None, _) => false,   // This event should have previous_hash
            (_, None) => false,   // Previous event should have event_hash
        }
    }
}

/// Backend storage for EventStore
enum EventStoreBackend {
    /// Single file storage (default, backward compatible)
    Single(Box<FileStorage>),
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
    /// Last event hash for hash chain continuity
    /// Initialized from the last event when loading, updated on each append
    last_event_hash: Option<String>,
    /// Enable hash chain for new events (default: true for new stores)
    enable_hash_chain: bool,
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
            EventStoreBackend::Single(Box::new(storage))
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

        // Load the last event hash for chain continuity
        let last_event_hash = Self::load_last_event_hash(&backend, binary_mode)?;

        // Enable hash chain by default, can be disabled via env var
        let enable_hash_chain = !std::env::var("RS_DISABLE_HASH_CHAIN")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

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
            last_event_hash,
            enable_hash_chain,
        })
    }

    /// Create a new event store with existing file storage (single-file mode)
    pub fn with_storage(storage: FileStorage) -> EngineResult<Self> {
        let events_count = storage.read_all_events()?.len();
        let backend = EventStoreBackend::Single(Box::new(storage));

        // Load the last event hash for chain continuity
        let last_event_hash = Self::load_last_event_hash(&backend, false)?;

        // Enable hash chain by default
        let enable_hash_chain = !std::env::var("RS_DISABLE_HASH_CHAIN")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        Ok(Self {
            backend,
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
            last_event_hash,
            enable_hash_chain,
        })
    }

    /// Load the last event hash from existing events (for chain continuity)
    fn load_last_event_hash(
        backend: &EventStoreBackend,
        binary_mode: bool,
    ) -> EngineResult<Option<String>> {
        let events = match backend {
            EventStoreBackend::Single(s) => {
                if binary_mode {
                    // Binary mode: decode envelopes
                    let bytes_lines = s.read_all_event_bytes()?;
                    let mut envelopes = Vec::new();
                    for line in bytes_lines {
                        if line.is_empty() {
                            continue;
                        }
                        if let Ok((env, _)) =
                            decode_from_slice::<EventEnvelope, _>(&line, standard())
                        {
                            envelopes.push(env);
                        }
                    }
                    envelopes
                } else {
                    // JSON mode: parse envelopes
                    s.read_all_events()?
                        .into_iter()
                        .filter_map(|line| serde_json::from_str::<EventEnvelope>(&line).ok())
                        .collect()
                }
            }
            EventStoreBackend::Multi(m) => m.read_all_envelopes()?,
        };

        // Get the last event's hash
        Ok(events.last().and_then(|e| e.event_hash.clone()))
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
                    "append_event not supported in multi-file mode, use append_envelope"
                        .to_string(),
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
                    "append_raw_line with non-envelope data not supported in multi-file mode"
                        .to_string(),
                ));
            }
        }
        self.events_count += 1;
        self.pending_since_flush += 1;
        // No auto-flush here as AsyncWriter controls flushing
        Ok(())
    }

    /// Append an envelope to the persistent log (preferred)
    ///
    /// If hash chain is enabled, this will automatically add hash chain fields
    /// to the envelope before persisting.
    pub fn append_envelope(&mut self, envelope: &EventEnvelope) -> EngineResult<()> {
        // Apply hash chain if enabled and envelope doesn't already have hashes
        let envelope_to_persist = if self.enable_hash_chain && envelope.event_hash.is_none() {
            let mut chained = envelope.clone();
            chained.previous_hash = self.last_event_hash.clone();
            chained.event_hash = Some(chained.compute_hash());
            chained
        } else {
            envelope.clone()
        };

        match &mut self.backend {
            EventStoreBackend::Single(storage) => {
                // Original single-file logic
                let mut offset: u64 = 0;
                let will_index = !self.disable_index && envelope_to_persist.aggregate_id.is_some();
                if will_index {
                    offset = storage.current_events_size()?;
                }

                if self.binary_mode {
                    let bytes = encode_to_vec(&envelope_to_persist, standard()).map_err(|e| {
                        EngineError::SerializationError(format!(
                            "Failed to serialize envelope (bincode): {}",
                            e
                        ))
                    })?;
                    storage.append_binary_event_bytes(&bytes)?;
                } else {
                    let json = serde_json::to_string(&envelope_to_persist).map_err(|e| {
                        EngineError::SerializationError(format!(
                            "Failed to serialize envelope: {}",
                            e
                        ))
                    })?;
                    storage.append_event(&json)?;
                }
                if will_index {
                    if let Some(agg) = &envelope_to_persist.aggregate_id {
                        let _ = storage.append_index_entry(agg, offset);
                    }
                }
            }
            EventStoreBackend::Multi(multi_store) => {
                // Multi-file logic - delegate to MultiFileEventStore
                multi_store.append_envelope(&envelope_to_persist)?;
            }
        }

        // Update the last event hash for chain continuity
        if self.enable_hash_chain {
            self.last_event_hash = envelope_to_persist.event_hash.clone();
        }

        self.events_count += 1;
        self.pending_since_flush += 1;
        if self.flush_every > 0 && self.pending_since_flush >= self.flush_every {
            self.flush()?;
        }
        Ok(())
    }

    /// Enable or disable hash chain for new events
    pub fn set_hash_chain(&mut self, enabled: bool) {
        self.enable_hash_chain = enabled;
    }

    /// Get the current last event hash (for external chain verification)
    pub fn get_last_event_hash(&self) -> Option<&String> {
        self.last_event_hash.as_ref()
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
            EventStoreBackend::Single(storage) => {
                storage.configure_batching(max_batch_size, fsync_on_append)
            }
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
            EventStoreBackend::Multi(_) => Err(EngineError::InvalidOperation(
                "save_snapshot not supported in multi-file mode".to_string(),
            )),
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
                    "truncate_events not supported in multi-file mode".to_string(),
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
            EventStoreBackend::Multi(_) => Err(EngineError::InvalidOperation(
                "load_snapshot not supported in multi-file mode".to_string(),
            )),
        }
    }

    /// Get all envelopes for chain verification
    pub fn get_all_envelopes(&self) -> EngineResult<Vec<EventEnvelope>> {
        match &self.backend {
            EventStoreBackend::Single(s) => {
                if self.binary_mode {
                    let bytes_lines = s.read_all_event_bytes()?;
                    let mut envelopes = Vec::new();
                    for line in bytes_lines {
                        if line.is_empty() {
                            continue;
                        }
                        if let Ok((env, _)) =
                            decode_from_slice::<EventEnvelope, _>(&line, standard())
                        {
                            envelopes.push(env);
                        }
                    }
                    Ok(envelopes)
                } else {
                    Ok(s.read_all_events()?
                        .into_iter()
                        .filter_map(|line| serde_json::from_str::<EventEnvelope>(&line).ok())
                        .collect())
                }
            }
            EventStoreBackend::Multi(m) => m.read_all_envelopes(),
        }
    }

    /// Verify the integrity of the entire hash chain
    ///
    /// Returns a `ChainVerificationResult` with details about the verification.
    /// - Checks each event's hash matches its computed hash
    /// - Checks each event's previous_hash links to the prior event's hash
    /// - Legacy events (without hashes) are accepted but noted
    pub fn verify_chain(&self) -> EngineResult<ChainVerificationResult> {
        let envelopes = self.get_all_envelopes()?;

        let mut result = ChainVerificationResult {
            total_events: envelopes.len(),
            verified_events: 0,
            legacy_events: 0,
            invalid_hashes: Vec::new(),
            broken_links: Vec::new(),
            is_valid: true,
        };

        let mut prev_hash: Option<String> = None;

        for (index, envelope) in envelopes.iter().enumerate() {
            // Check if this is a legacy event (no hashes)
            if envelope.event_hash.is_none() {
                result.legacy_events += 1;
                // Legacy events don't break the chain, just skip chain verification for them
                prev_hash = None;
                continue;
            }

            // Verify the event's own hash
            if !envelope.verify_hash() {
                result.invalid_hashes.push(ChainError {
                    event_index: index,
                    event_id: envelope.event_id.clone(),
                    error: "Event hash does not match computed hash (tampered?)".to_string(),
                });
                result.is_valid = false;
            }

            // Verify the link to previous event
            match (&envelope.previous_hash, &prev_hash) {
                (Some(prev), Some(expected)) if prev != expected => {
                    result.broken_links.push(ChainError {
                        event_index: index,
                        event_id: envelope.event_id.clone(),
                        error: format!(
                            "Chain broken: previous_hash {} doesn't match expected {}",
                            prev, expected
                        ),
                    });
                    result.is_valid = false;
                }
                (None, Some(_)) => {
                    result.broken_links.push(ChainError {
                        event_index: index,
                        event_id: envelope.event_id.clone(),
                        error: "Missing previous_hash but chain expected continuation".to_string(),
                    });
                    result.is_valid = false;
                }
                _ => {
                    // Valid link (or genesis event with no previous)
                    result.verified_events += 1;
                }
            }

            prev_hash = envelope.event_hash.clone();
        }

        Ok(result)
    }
}

/// Result of hash chain verification
#[derive(Debug, Clone)]
pub struct ChainVerificationResult {
    /// Total number of events checked
    pub total_events: usize,
    /// Number of events with valid hashes and links
    pub verified_events: usize,
    /// Number of legacy events (without hash chain)
    pub legacy_events: usize,
    /// Events with invalid hashes (potential tampering)
    pub invalid_hashes: Vec<ChainError>,
    /// Events with broken links (chain discontinuity)
    pub broken_links: Vec<ChainError>,
    /// Overall chain validity
    pub is_valid: bool,
}

impl ChainVerificationResult {
    /// Check if the chain is fully verified (no legacy events)
    pub fn is_fully_verified(&self) -> bool {
        self.is_valid && self.legacy_events == 0
    }

    /// Human-readable summary
    pub fn summary(&self) -> String {
        if self.is_valid {
            if self.legacy_events > 0 {
                format!(
                    "✅ Chain valid: {}/{} events verified ({} legacy events without hash chain)",
                    self.verified_events, self.total_events, self.legacy_events
                )
            } else {
                format!(
                    "✅ Chain fully verified: {}/{} events",
                    self.verified_events, self.total_events
                )
            }
        } else {
            format!(
                "❌ Chain INVALID: {} hash errors, {} broken links out of {} events",
                self.invalid_hashes.len(),
                self.broken_links.len(),
                self.total_events
            )
        }
    }
}

/// Details about a chain verification error
#[derive(Debug, Clone)]
pub struct ChainError {
    /// Index of the event in the chain
    pub event_index: usize,
    /// Event ID for identification
    pub event_id: String,
    /// Description of the error
    pub error: String,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_envelope_hash_computation() {
        let envelope = EventEnvelope::new(
            "TestEvent".to_string(),
            "event-001".to_string(),
            1234567890,
            r#"{"test": "data"}"#.to_string(),
            Some("aggregate-1".to_string()),
            None, // Genesis event
        );

        // Hash should be computed
        assert!(envelope.event_hash.is_some());
        assert!(envelope.previous_hash.is_none()); // Genesis

        // Hash should be verifiable
        assert!(envelope.verify_hash());
    }

    #[test]
    fn test_envelope_hash_chain() {
        // Create genesis event
        let genesis = EventEnvelope::new(
            "TestEvent".to_string(),
            "event-001".to_string(),
            1234567890,
            r#"{"test": "genesis"}"#.to_string(),
            Some("aggregate-1".to_string()),
            None,
        );

        // Create second event linked to genesis
        let second = EventEnvelope::new(
            "TestEvent".to_string(),
            "event-002".to_string(),
            1234567891,
            r#"{"test": "second"}"#.to_string(),
            Some("aggregate-1".to_string()),
            genesis.event_hash.clone(),
        );

        // Verify both events
        assert!(genesis.verify_hash());
        assert!(second.verify_hash());

        // Verify chain link
        assert!(second.links_to(&genesis));
    }

    #[test]
    fn test_envelope_tamper_detection() {
        let envelope = EventEnvelope::new(
            "TestEvent".to_string(),
            "event-001".to_string(),
            1234567890,
            r#"{"test": "data"}"#.to_string(),
            None,
            None,
        );

        // Original is valid
        assert!(envelope.verify_hash());

        // Tamper with the payload
        let mut tampered = envelope.clone();
        tampered.payload = r#"{"test": "TAMPERED"}"#.to_string();

        // Tampered envelope should fail verification
        assert!(!tampered.verify_hash());
    }

    #[test]
    fn test_envelope_chain_break_detection() {
        let event1 = EventEnvelope::new(
            "TestEvent".to_string(),
            "event-001".to_string(),
            1234567890,
            "{}".to_string(),
            None,
            None,
        );

        let event2 = EventEnvelope::new(
            "TestEvent".to_string(),
            "event-002".to_string(),
            1234567891,
            "{}".to_string(),
            None,
            event1.event_hash.clone(),
        );

        // Create event3 with wrong previous hash (chain break)
        let event3 = EventEnvelope::new(
            "TestEvent".to_string(),
            "event-003".to_string(),
            1234567892,
            "{}".to_string(),
            None,
            Some("WRONG_HASH".to_string()),
        );

        // event2 should link to event1
        assert!(event2.links_to(&event1));

        // event3 should NOT link to event2
        assert!(!event3.links_to(&event2));
    }

    #[test]
    fn test_legacy_envelope_compatibility() {
        // Simulate a legacy envelope (no hash fields)
        let legacy = EventEnvelope {
            event_type: "LegacyEvent".to_string(),
            event_id: "legacy-001".to_string(),
            timestamp: 1234567890,
            payload: "{}".to_string(),
            aggregate_id: None,
            event_hash: None,
            previous_hash: None,
        };

        // Legacy events should be considered valid (no hash to verify)
        assert!(legacy.verify_hash());

        // Serialize/deserialize should work
        let json = serde_json::to_string(&legacy).unwrap();
        let deserialized: EventEnvelope = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.event_id, "legacy-001");
        assert!(deserialized.event_hash.is_none());
    }
}
