//! Integration between DeclarativeModel and HTTP server
//!
//! This module provides the bridge between Lithair's DeclarativeModel system
//! and the Hyper HTTP server, automatically generating REST endpoints from model definitions.

use crate::http::FirewallConfig;
use bytes::Bytes;
use chrono;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::{Method, Request, Response, StatusCode};
use serde::{de::DeserializeOwned, Serialize};
use serde_json;
use std::convert::Infallible;
use std::sync::Arc;

use crate::consensus::ReplicatedModel;
#[cfg(feature = "cluster")]
use crate::consensus::{ConsensusConfig, DeclarativeConsensus};
use crate::engine::events::{EventEnvelope, EventStore};
use crate::lifecycle::LifecycleAware;

type RespBody = BoxBody<Bytes, Infallible>;
type Req = Request<Incoming>;
type Resp = Response<RespBody>;

#[inline]
fn body_from<T: Into<Bytes>>(data: T) -> RespBody {
    Full::new(data.into()).boxed()
}

/// Strip the body from a response, keeping status and headers (for HEAD requests)
fn strip_body(resp: Resp) -> Resp {
    let (parts, _body) = resp.into_parts();
    Response::from_parts(parts, Full::new(Bytes::new()).boxed())
}

/// Extract a session token from the request, trying the `Authorization`
/// header first then falling back to the `session_token=` cookie.
///
/// Bearer scheme matching is case-insensitive per common practice (the RFC
/// only mandates `Bearer` but many clients send `bearer`; route_guard.rs
/// and the `/auth/validate` route both already accept the cookie form).
/// Used by [`DeclarativeHttpHandler::has_valid_session`] to gate
/// auto-generated `/api/{model}` endpoints when
/// `with_models_require_session(true)` is on (issue #78).
///
/// Returns `None` if no usable token is found.
fn extract_session_token(req: &Req) -> Option<String> {
    // 1) Authorization: Bearer <token>  (case-insensitive scheme)
    if let Some(auth_header) = req.headers().get(http::header::AUTHORIZATION) {
        if let Ok(auth_str) = auth_header.to_str() {
            if auth_str.len() >= 7 && auth_str[..7].eq_ignore_ascii_case("bearer ") {
                let token = auth_str[7..].trim();
                if !token.is_empty() {
                    return Some(token.to_string());
                }
            }
        }
    }

    // 2) Cookie: session_token=<id>  (matches the pattern used by
    //    `LithairServerBuilder` line 642 and `route_guard.rs` line 167)
    if let Some(cookie_header) = req.headers().get(http::header::COOKIE) {
        if let Ok(cookie_str) = cookie_header.to_str() {
            for part in cookie_str.split(';') {
                let part = part.trim();
                if let Some(value) = part.strip_prefix("session_token=") {
                    let token = value.trim();
                    if !token.is_empty() {
                        return Some(token.to_string());
                    }
                }
            }
        }
    }

    None
}

/// Trait for models that can be exposed via HTTP
///
/// This trait is automatically implemented by the DeclarativeModel macro
/// when the #[http(expose)] attribute is used.
pub trait HttpExposable: Serialize + DeserializeOwned + Clone + Send + Sync + 'static {
    /// Get the base path for this model's REST endpoints
    /// Example: "products" for /api/products
    fn http_base_path() -> &'static str;

    /// Get the primary key field name
    fn primary_key_field() -> &'static str;

    /// Get the primary key value for this instance
    fn get_primary_key(&self) -> String;

    /// Validate the model according to #[http(validate)] attributes
    fn validate(&self) -> Result<(), String>;

    /// Optional declarative firewall configuration attached to the model type.
    /// Defaults to None; can be overridden by the derive macro via #[firewall(...)]
    fn firewall_config() -> Option<FirewallConfig> {
        None
    }

    /// Check if the current user can read this model
    /// Based on #[permission(read)] attributes
    fn can_read(&self, _user_permissions: &[String]) -> bool {
        true // Default: allow all
    }

    /// Check if the current user can write this model
    /// Based on #[permission(write)] attributes  
    fn can_write(&self, _user_permissions: &[String]) -> bool {
        true // Default: allow all
    }

    /// Apply lifecycle rules before persisting
    /// Based on `#[lifecycle]` attributes
    fn apply_lifecycle(&mut self) -> Result<(), String> {
        Ok(()) // Default: no lifecycle rules
    }
}

/// HTTP handler for DeclarativeModel CRUD operations
pub struct DeclarativeHttpHandler<T>
where
    T: HttpExposable + LifecycleAware + ReplicatedModel,
{
    event_store: Arc<tokio::sync::RwLock<EventStore>>,
    storage: Arc<tokio::sync::RwLock<std::collections::HashMap<String, T>>>,
    #[cfg(feature = "cluster")]
    consensus: Option<Arc<tokio::sync::RwLock<DeclarativeConsensus<T>>>>,
    permission_checker: Option<Arc<dyn crate::rbac::PermissionChecker>>,
    /// Optional extractor to resolve user permissions (as strings) from the HTTP request
    /// This enables declarative read filtering via HttpExposable::can_read()
    #[allow(clippy::type_complexity)]
    permission_extractor: Option<Arc<dyn Fn(&Req) -> Vec<String> + Send + Sync>>,
    pub(crate) session_store: Option<Arc<dyn std::any::Any + Send + Sync>>,
    /// Optional SSE broadcaster for real-time change notifications
    pub(crate) sse_broadcaster: Option<Arc<crate::http::sse::SseEventBroadcaster>>,
    /// When true, every non-OPTIONS request to this model's auto-generated
    /// CRUD endpoints must carry a valid (non-expired) session in the
    /// `Authorization: Bearer <session-id>` header — otherwise the handler
    /// returns HTTP 401. Set via `LithairServerBuilder::with_models_require_session(true)`.
    /// Defaults to `false` (current behavior preserved).
    pub(crate) require_session: bool,
}

impl<T> DeclarativeHttpHandler<T>
where
    T: HttpExposable + LifecycleAware + ReplicatedModel,
{
    #[inline]
    fn is_verbose() -> bool {
        std::env::var("LT_VERBOSE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    }
    pub fn new(event_store_path: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Initialize EventStore with batching configuration
        let mut event_store = EventStore::new(event_store_path)?;
        let max_batch_size: usize = std::env::var("LT_EVENT_MAX_BATCH")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(16_384);
        let fsync_on_append: bool = std::env::var("LT_FSYNC_ON_APPEND")
            .ok()
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        event_store.configure_batching(max_batch_size, fsync_on_append);

        let event_store = Arc::new(tokio::sync::RwLock::new(event_store));

        // Spawn a lightweight background flusher to persist batches periodically
        let flush_interval_ms: u64 = std::env::var("LT_FLUSH_INTERVAL_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(100);
        let store_clone = Arc::clone(&event_store);
        tokio::spawn(async move {
            let interval = std::time::Duration::from_millis(flush_interval_ms);
            loop {
                {
                    let mut store = store_clone.write().await;
                    let _ = store.flush_events();
                }
                tokio::time::sleep(interval).await;
            }
        });

        let handler = Self {
            event_store,
            storage: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            #[cfg(feature = "cluster")]
            consensus: None,
            permission_checker: None,
            permission_extractor: None,
            session_store: None,
            sse_broadcaster: None,
            require_session: false,
        };

        Ok(handler)
    }

    /// Get a reference to the event store for chain verification
    pub fn get_event_store(&self) -> &Arc<tokio::sync::RwLock<EventStore>> {
        &self.event_store
    }

    /// Create a new DeclarativeHttpHandler with automatic event replay
    ///
    /// This is a convenience method that creates the handler and automatically
    /// replays all persisted events to restore state from the event log.
    pub async fn new_with_replay(
        event_store_path: &str,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let handler = Self::new(event_store_path)?;
        handler.replay_events().await?;
        Ok(handler)
    }

    pub async fn replay_events(&self) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        // Snapshot-first replay (issue #69 follow-up):
        // If a snapshot was written by a previous `compact()` call, it
        // captures all storage state up to the moment the log was
        // truncated. Without loading it first, a restart after compaction
        // would see no events and reconstruct an empty `HashMap` —
        // permanent data loss. Load snapshot first, then replay any
        // events appended after the snapshot.
        let snapshot_json = {
            let store = self.event_store.read().await;
            // `load_snapshot()` returns `Err` on multi-file backends, which
            // don't support the simple snapshot+truncate path. Treat that
            // as "no snapshot available" — multi-file users are not on the
            // auto-compaction code path anyway.
            store.load_snapshot().ok().flatten()
        };

        let mut storage = self.storage.write().await;
        let mut from_snapshot = 0usize;
        if let Some(json) = snapshot_json {
            match serde_json::from_str::<std::collections::HashMap<String, T>>(&json) {
                Ok(snap) => {
                    from_snapshot = snap.len();
                    for (k, v) in snap {
                        storage.insert(k, v);
                    }
                }
                Err(e) => {
                    log::warn!(
                        "Snapshot present but failed to deserialize ({}), falling back to events-only replay",
                        e
                    );
                }
            }
        }

        let events = {
            let store = self.event_store.read().await;
            store.get_all_events()?
        };

        let mut replayed_count = 0;
        for event_json in events {
            if let Ok(envelope) = serde_json::from_str::<EventEnvelope>(&event_json) {
                if let Ok(item) = serde_json::from_str::<T>(&envelope.payload) {
                    let key = item.get_primary_key();
                    storage.insert(key, item);
                    replayed_count += 1;
                }
            }
        }

        if Self::is_verbose() || replayed_count > 0 || from_snapshot > 0 {
            log::info!(
                "Replayed {} events into memory ({} items restored from snapshot)",
                replayed_count,
                from_snapshot
            );
        }

        Ok(replayed_count)
    }

    /// Atomically snapshot the current storage state and truncate the
    /// event log (issue #69 follow-up — addresses Gemini review on PR #84).
    ///
    /// Acquires the storage read lock briefly to serialize state, then
    /// the event-store write lock to persist the snapshot and truncate.
    /// After this returns `Ok(())`, the on-disk `.raftlog` is empty but
    /// the on-disk `.snapshot` file holds the full state — a restart
    /// reconstructs storage from the snapshot via `replay_events()`.
    ///
    /// **Critical**: callers must NEVER call `truncate_events()` directly
    /// on the underlying `EventStore` without first writing a snapshot —
    /// doing so causes permanent data loss. This method is the only safe
    /// compaction primitive at the handler level.
    pub async fn compact(&self) -> Result<(), String> {
        // 1. Serialize the current storage map under the storage read lock.
        let state_json = {
            let storage = self.storage.read().await;
            serde_json::to_string(&*storage)
                .map_err(|e| format!("compact: failed to serialize storage: {}", e))?
        };

        // 2. Hold the event-store write lock for snapshot+truncate so the
        //    two operations are atomic from any other event-store caller's
        //    perspective. Callers that block waiting on the write lock
        //    will see either "pre-compact" or "post-compact" — never an
        //    intermediate state where the snapshot exists but the log
        //    has not yet been truncated (or vice versa, the dangerous
        //    case fixed here).
        let mut store = self.event_store.write().await;
        store
            .save_snapshot(&state_json)
            .map_err(|e| format!("compact: save_snapshot failed: {}", e))?;
        store
            .truncate_events()
            .map_err(|e| format!("compact: truncate_events failed: {}", e))?;
        Ok(())
    }

    /// Returns true if consensus is enabled for this handler
    pub fn is_consensus_enabled(&self) -> bool {
        #[cfg(feature = "cluster")]
        {
            self.consensus.is_some()
        }
        #[cfg(not(feature = "cluster"))]
        {
            false
        }
    }

    /// Set the permission checker for RBAC enforcement
    pub fn with_permission_checker(
        mut self,
        checker: Arc<dyn crate::rbac::PermissionChecker>,
    ) -> Self {
        self.permission_checker = Some(checker);
        self
    }

    /// Set the session store for extracting user roles
    pub fn with_session_store<S: 'static + Send + Sync>(mut self, store: Arc<S>) -> Self {
        self.session_store = Some(store as Arc<dyn std::any::Any + Send + Sync>);
        self
    }

    /// Provide a custom permission extractor from HTTP request to a list of permission identifiers
    /// These identifiers are passed to `T::can_read(&[String])` for declarative read filtering
    pub fn with_permission_extractor<F>(mut self, extractor: F) -> Self
    where
        F: Fn(&Req) -> Vec<String> + Send + Sync + 'static,
    {
        self.set_permission_extractor(extractor);
        self
    }

    /// Set the SSE broadcaster for real-time change notifications
    pub fn with_sse_broadcaster(
        mut self,
        broadcaster: Arc<crate::http::sse::SseEventBroadcaster>,
    ) -> Self {
        self.sse_broadcaster = Some(broadcaster);
        self
    }

    /// Mutably set the permission extractor in-place
    pub fn set_permission_extractor<F>(&mut self, extractor: F)
    where
        F: Fn(&Req) -> Vec<String> + Send + Sync + 'static,
    {
        self.permission_extractor = Some(Arc::new(extractor));
    }

    /// Extract role from Authorization header (Bearer token)
    async fn extract_role_from_request(&self, req: &Req) -> Option<String> {
        use crate::session::SessionStore;

        // Get session store (if configured)
        let session_store_any = self.session_store.as_ref()?.clone();

        // Try to downcast Arc<dyn Any> to Arc<PersistentSessionStore>
        let store: Arc<crate::session::PersistentSessionStore> =
            session_store_any.downcast().ok()?;

        // Extract Bearer token from Authorization header
        let auth_header = req.headers().get(http::header::AUTHORIZATION)?.to_str().ok()?;
        let token = auth_header.strip_prefix("Bearer ")?.trim();

        // Get session from store
        let session = store.get(token).await.ok()??;

        // Extract role from session
        let role: Option<String> = session.get("role");
        role
    }

    /// Returns true if the request carries a valid, non-expired session.
    ///
    /// Used by the `with_models_require_session(true)` builder switch to gate
    /// auto-generated `/api/{model}` endpoints with a session-presence check
    /// (no role, no RBAC). Returns `false` when any of these holds:
    /// - No session store is configured (e.g. `with_sessions(...)` not called)
    /// - The session store is neither `PersistentSessionStore` nor
    ///   `SessionManager<PersistentSessionStore>` (the two shapes Lithair
    ///   currently stores in `session_manager`)
    /// - Neither the `Authorization: Bearer <token>` header nor the
    ///   `Cookie: session_token=<id>` cookie carries a usable id
    /// - The session id is not found in the store
    /// - The session has expired
    ///
    /// The two-shape handling is intentional: `LithairServerBuilder` stores
    /// `Arc<PersistentSessionStore>` when sessions are configured via
    /// `with_rbac_config(...)` (lithair-core/src/app/builder.rs:540) but
    /// `Arc<SessionManager<S>>` when configured via `with_sessions(...)`
    /// (lithair-core/src/app/builder.rs:256). Both must work.
    ///
    /// Cookie fallback matches the pattern already used by
    /// `lithair-core/src/app/builder.rs` (the `/auth/validate` route, line 642)
    /// and `lithair-core/src/http/route_guard.rs` (line 167) — both lookup
    /// `session_token=...` in the `Cookie` header. Keeping this gate aligned
    /// avoids breaking callers (e.g. browser clients with cookie auth) who
    /// already rely on the cookie path elsewhere in the framework.
    async fn has_valid_session(&self, req: &Req) -> bool {
        use crate::session::RecognizedSessionStore;

        let Some(session_store_any) = self.session_store.as_ref() else {
            return false;
        };

        let Some(token) = extract_session_token(req) else {
            return false;
        };

        // The recognizer is the single source of truth for which session
        // store shapes Lithair supports here. The boot-time fail-fast in
        // `app/mod.rs::serve` uses the same helper, so adding a new shape
        // automatically benefits both sides (issue #80).
        let Some(recognized) = RecognizedSessionStore::recognize(session_store_any) else {
            return false;
        };
        recognized.get_live_session(&token).await.is_some()
    }

    /// Set whether this handler must reject requests lacking a valid session.
    ///
    /// When `true`, every non-OPTIONS request to the auto-generated CRUD
    /// endpoints returns HTTP 401 unless a valid session is present.
    /// Default is `false` (current behavior preserved).
    pub(crate) fn set_require_session(&mut self, require: bool) {
        self.require_session = require;
    }

    /// Return current in-memory storage item count (for debug/diagnostics)
    pub async fn storage_count(&self) -> usize {
        let storage = self.storage.read().await;
        storage.len()
    }

    /// Return all items from storage (cloned)
    /// Useful for relational queries and filtering
    pub async fn get_all_items(&self) -> Vec<T> {
        let storage = self.storage.read().await;
        storage.values().cloned().collect()
    }

    /// Return at most `limit` items from storage (cloned).
    ///
    /// Used by sampled diagnostics (e.g. `ModelStats::approx_ram_bytes`) to
    /// avoid cloning every item when only a small sample is needed. Iterates
    /// the underlying `HashMap` in unspecified order — callers must treat the
    /// result as a representative sample, not a stable selection.
    pub async fn get_sample_items(&self, limit: usize) -> Vec<T> {
        if limit == 0 {
            return Vec::new();
        }
        let storage = self.storage.read().await;
        storage.values().take(limit).cloned().collect()
    }

    /// Return all items matching a predicate (cloned)
    /// Useful for relational queries like "orders for consumer X"
    pub async fn query<F>(&self, predicate: F) -> Vec<T>
    where
        F: Fn(&T) -> bool,
    {
        let storage = self.storage.read().await;
        storage.values().filter(|item| predicate(*item)).cloned().collect()
    }

    /// Get a single item by ID (cloned)
    pub async fn get_by_id(&self, id: &str) -> Option<T> {
        let storage = self.storage.read().await;
        storage.get(id).cloned()
    }

    /// Replace local in-memory storage with authoritative items from leader (no persistence writes)
    pub async fn reconcile_replace_all(&self, items: Vec<T>) {
        let mut storage = self.storage.write().await;
        storage.clear();
        for item in items.into_iter() {
            let actual_key = serde_json::to_value(&item)
                .ok()
                .and_then(|v| v.get("id").and_then(|id| id.as_str().map(|s| s.to_string())))
                .unwrap_or_else(|| item.get_primary_key());
            storage.insert(actual_key, item);
        }
        if Self::is_verbose() {
            log::debug!(
                "Reconcile: storage replaced with authoritative snapshot ({} items)",
                storage.len()
            );
        }
    }

    /// Apply a single replicated item from leader (for followers to receive replication)
    /// This adds to storage AND persists to event store (idempotent via key-based storage)
    pub async fn apply_replicated_item(&self, item: T) -> Result<(), String> {
        let actual_key = serde_json::to_value(&item)
            .ok()
            .and_then(|v| v.get("id").and_then(|id| id.as_str().map(|s| s.to_string())))
            .unwrap_or_else(|| item.get_primary_key());

        // Insert into storage FIRST (this is the critical operation)
        {
            let mut storage = self.storage.write().await;
            storage.insert(actual_key.clone(), item.clone());
        }

        // Persist to event store (best-effort - don't fail the operation)
        // IMPORTANT: Storage is already updated, so operation must succeed for consistency
        if let Err(e) = self.persist_to_event_store("Replicated", &item).await {
            log::warn!(
                "Failed to persist replicated item event for {}: {:?} (storage already updated)",
                actual_key,
                e
            );
        }

        if Self::is_verbose() {
            log::debug!("Replicated item {} applied to follower", actual_key);
        }

        Ok(())
    }

    /// Apply multiple replicated items from leader (bulk replication for followers)
    pub async fn apply_replicated_items(&self, items: Vec<T>) -> Result<usize, String> {
        let count = items.len();
        for item in items {
            self.apply_replicated_item(item).await?;
        }
        if Self::is_verbose() {
            log::debug!("Bulk replicated {} items applied to follower", count);
        }
        Ok(count)
    }

    /// Apply a replicated UPDATE from leader (for followers to receive UPDATE replication)
    /// This updates storage AND persists to event store
    pub async fn apply_replicated_update(&self, id: &str, item: T) -> Result<(), String> {
        // Check if item exists
        {
            let storage = self.storage.read().await;
            let has_key = storage.contains_key(id);
            log::debug!(
                "APPLY UPDATE: id={}, exists_in_storage={}, storage_len={}",
                id,
                has_key,
                storage.len()
            );
            if !has_key {
                // If item doesn't exist, treat as create (eventual consistency)
                drop(storage);
                log::debug!("APPLY UPDATE: item doesn't exist, creating instead");
                return self.apply_replicated_item(item).await;
            }
        }

        // Update in storage
        {
            let mut storage = self.storage.write().await;
            storage.insert(id.to_string(), item.clone());
        }

        // Persist to event store (best-effort - don't fail the operation)
        // IMPORTANT: Storage is already updated, so we must succeed for consistency
        if let Err(e) = self.persist_to_event_store("Updated", &item).await {
            log::warn!(
                "Failed to persist update event for {}: {:?} (storage already updated)",
                id,
                e
            );
        }

        if Self::is_verbose() {
            log::debug!("Replicated UPDATE for {} applied", id);
        }

        Ok(())
    }

    /// Apply a replicated DELETE from leader (for followers to receive DELETE replication)
    /// This removes from storage AND persists deletion event to event store
    /// IMPORTANT: This must be fully idempotent and never fail once storage is modified
    pub async fn apply_replicated_delete(&self, id: &str) -> Result<bool, String> {
        // Remove from storage
        let removed_item = {
            let mut storage = self.storage.write().await;
            let has_key = storage.contains_key(id);
            log::debug!(
                "APPLY DELETE: id={}, exists_in_storage={}, storage_len={}",
                id,
                has_key,
                storage.len()
            );
            storage.remove(id)
        };

        if let Some(item) = removed_item {
            // Persist deletion to event store (best-effort - don't fail the operation)
            // This ensures idempotency: once item is removed from storage, operation succeeds
            if let Err(e) = self.persist_to_event_store("Deleted", &item).await {
                log::warn!(
                    "Failed to persist delete event for {}: {:?} (storage already updated)",
                    id,
                    e
                );
            }

            if Self::is_verbose() {
                log::debug!("Replicated DELETE for {} applied", id);
            }

            Ok(true)
        } else {
            // Item didn't exist (idempotent behavior - not an error)
            log::debug!("Replicated DELETE for {} - item not found (idempotent)", id);
            Ok(false)
        }
    }

    /// Check that unique constraints are satisfied.
    /// Scans storage for any other item with the same value on a unique field.
    async fn check_unique_constraints(
        &self,
        item: &T,
        exclude_id: Option<&str>,
    ) -> Result<(), String> {
        let item_json = serde_json::to_value(item).map_err(|e| e.to_string())?;
        let storage = self.storage.read().await;

        for field_name in item.all_field_names() {
            if let Some(policy) = item.lifecycle_policy_for_field(field_name) {
                if !policy.unique {
                    continue;
                }
                let new_val = match item_json.get(field_name) {
                    Some(v) if !v.is_null() => v,
                    _ => continue,
                };
                for (existing_id, existing_item) in storage.iter() {
                    if exclude_id == Some(existing_id.as_str()) {
                        continue;
                    }
                    if let Ok(existing_json) = serde_json::to_value(existing_item) {
                        if existing_json.get(field_name) == Some(new_val) {
                            return Err(format!(
                                "'{}' must be unique: value {} already exists",
                                field_name, new_val
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Check that no immutable fields have been modified between the existing and updated item.
    /// Uses JSON serialization to compare field values without requiring the Inspectable trait bound.
    fn check_immutable_fields(existing: &T, updated: &T) -> Result<(), String> {
        let old_json = serde_json::to_value(existing).map_err(|e| e.to_string())?;
        let new_json = serde_json::to_value(updated).map_err(|e| e.to_string())?;

        for field_name in existing.all_field_names() {
            if existing.is_field_immutable(field_name) {
                let old_val = old_json.get(field_name);
                let new_val = new_json.get(field_name);
                if old_val != new_val {
                    return Err(format!("'{}' is immutable and cannot be modified", field_name));
                }
            }
        }
        Ok(())
    }

    /// Log changes to audited fields between old and new versions.
    /// Fields annotated with #[lifecycle(audited)] get their changes recorded via the log crate.
    fn log_audited_changes(existing: &T, updated: &T) {
        let old_json = match serde_json::to_value(existing) {
            Ok(v) => v,
            Err(_) => return,
        };
        let new_json = match serde_json::to_value(updated) {
            Ok(v) => v,
            Err(_) => return,
        };
        let model_name = std::any::type_name::<T>().rsplit("::").next().unwrap_or("Unknown");
        let id = updated.get_primary_key();

        for field_name in existing.all_field_names() {
            if !existing.is_field_audited(field_name) {
                continue;
            }
            let old_val = old_json.get(field_name);
            let new_val = new_json.get(field_name);
            if old_val != new_val {
                log::info!(
                    "AUDIT {}/{}: field '{}' changed from {} to {}",
                    model_name,
                    id,
                    field_name,
                    old_val.map(|v| v.to_string()).unwrap_or_else(|| "null".into()),
                    new_val.map(|v| v.to_string()).unwrap_or_else(|| "null".into()),
                );
            }
        }
    }

    /// GET /api/{model}/_schema - Expose the declarative model specification as JSON
    async fn handle_schema(&self) -> Result<Resp, Infallible> {
        let model_name = std::any::type_name::<T>().rsplit("::").next().unwrap_or("Unknown");
        // Build a dummy instance via JSON to access trait methods
        // Use the storage to get a real item, or build field info from an empty default
        let storage = self.storage.read().await;
        let sample = storage.values().next();

        let mut fields = serde_json::Map::new();
        if let Some(item) = sample {
            for field_name in item.all_field_names() {
                let mut info = serde_json::Map::new();
                if let Some(policy) = item.lifecycle_policy_for_field(field_name) {
                    info.insert("immutable".into(), serde_json::json!(policy.immutable));
                    info.insert("audited".into(), serde_json::json!(policy.audited));
                    info.insert("unique".into(), serde_json::json!(policy.unique));
                    info.insert("indexed".into(), serde_json::json!(policy.indexed));
                    if policy.version_limit > 0 {
                        info.insert(
                            "version_limit".into(),
                            serde_json::json!(policy.version_limit),
                        );
                    }
                    if policy.fk {
                        info.insert("foreign_key".into(), serde_json::json!(true));
                    }
                }
                fields.insert(field_name.to_string(), serde_json::Value::Object(info));
            }
        }

        let count = storage.len();
        drop(storage);

        let schema = serde_json::json!({
            "model": model_name,
            "base_path": format!("/api/{}", T::http_base_path()),
            "primary_key": T::primary_key_field(),
            "item_count": count,
            "fields": fields,
            "methods": ["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"],
        });

        let body = serde_json::to_string_pretty(&schema).unwrap_or_default();
        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(body_from(body))
            .unwrap())
    }

    /// GET /api/{model}/count - Return item count only (lightweight read)
    async fn handle_count(&self) -> Result<Resp, Infallible> {
        let count = self.storage_count().await as u64;
        let body = serde_json::json!({"count": count}).to_string();
        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(body_from(body))
            .unwrap())
    }

    /// GET /api/{model}/random-id - Return any existing id to help UPDATE workloads
    async fn handle_random_id(&self) -> Result<Resp, Infallible> {
        let storage = self.storage.read().await;
        if let Some((id, _)) = storage.iter().next() {
            let body = serde_json::json!({"id": id}).to_string();
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(body_from(body))
                .unwrap())
        } else {
            Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header("content-type", "application/json")
                .body(body_from(r#"{"error":"no ids available"}"#))
                .unwrap())
        }
    }

    /// Enable consensus mode for distributed replication
    #[cfg(feature = "cluster")]
    pub async fn enable_consensus(
        &mut self,
        config: ConsensusConfig,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut consensus = DeclarativeConsensus::new(config);
        consensus.initialize().await?;
        self.consensus = Some(Arc::new(tokio::sync::RwLock::new(consensus)));
        Ok(())
    }

    /// Configure persistence settings based on declarative model attributes
    pub async fn configure_declarative_persistence(
        &mut self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        log::info!("Analyzing declarative persistence configuration...");

        // For now, just log that we're using the conservative settings
        // The actual logic would analyze T::get_declarative_spec() when the trait bounds are fixed
        log::info!("Using conservative persistence settings (enable_compaction: false by default)");
        log::info!("Compaction will only be enabled if declarative attributes specify it");
        log::info!("This prevents automatic deletion of .raftlog files");

        Ok(())
    }

    /// Handle HTTP request for this model type
    pub async fn handle_request(
        &self,
        req: Req,
        path_segments: &[&str],
    ) -> Result<Resp, Infallible> {
        let method = req.method();

        // ====================================================================
        // Session gate (issue #78): if `with_models_require_session(true)` was
        // set on the builder, every non-OPTIONS request to this model's
        // auto-generated CRUD endpoints must carry a valid session — otherwise
        // reject with HTTP 401 before any business logic runs.
        //
        // OPTIONS (CORS preflight) is exempt by design: preflight requests do
        // not carry credentials. Returning 401 on OPTIONS would break browsers
        // before they ever issue the real request.
        // ====================================================================
        if self.require_session && method != Method::OPTIONS && !self.has_valid_session(&req).await
        {
            return Ok(
                self.json_error_response(StatusCode::UNAUTHORIZED, "Authentication required")
            );
        }

        match (method, path_segments.len()) {
            // OPTIONS - CORS preflight (any path)
            (&Method::OPTIONS, _) => Ok(Response::builder()
                .status(StatusCode::NO_CONTENT)
                .header("allow", "GET, POST, PUT, PATCH, DELETE, OPTIONS")
                .body(body_from(Bytes::new()))
                .unwrap()),

            // GET /api/products - List all (with declarative read filtering)
            (&Method::GET, 0) => self.handle_list(&req).await,
            // HEAD /api/products - Same headers as GET list, no body
            (&Method::HEAD, 0) => self.handle_list(&req).await.map(strip_body),

            // GET /api/products/stream - SSE real-time change subscription
            (&Method::GET, 1) if path_segments[0] == "stream" => self.handle_sse_stream().await,

            // GET /api/products/count - Count items (lightweight read)
            (&Method::GET, 1) if path_segments[0] == "count" => self.handle_count().await,
            (&Method::HEAD, 1) if path_segments[0] == "count" => {
                self.handle_count().await.map(strip_body)
            }

            // GET /api/products/random-id - Return a single existing id (lightweight)
            (&Method::GET, 1) if path_segments[0] == "random-id" => self.handle_random_id().await,

            // GET /api/products/_schema - Declarative model introspection
            (&Method::GET, 1) if path_segments[0] == "_schema" => self.handle_schema().await,

            // POST /api/products - Create
            (&Method::POST, 0) => self.handle_create(req).await,

            // POST /api/products/_bulk - Bulk Create
            (&Method::POST, 1) if path_segments[0] == "_bulk" => self.handle_bulk_create(req).await,

            // GET /api/products/{id} - Get by ID (with declarative read filtering)
            (&Method::GET, 1) => {
                let id = crate::http::query::percent_decode(path_segments[0]);
                self.handle_get(&id, &req).await
            }
            // HEAD /api/products/{id} - Same headers as GET, no body
            (&Method::HEAD, 1) => {
                let id = crate::http::query::percent_decode(path_segments[0]);
                self.handle_get(&id, &req).await.map(strip_body)
            }

            // PUT /api/products/{id} - Update
            (&Method::PUT, 1) => {
                let id = crate::http::query::percent_decode(path_segments[0]);
                self.handle_update(&id, req).await
            }

            // DELETE /api/products/{id} - Delete
            (&Method::DELETE, 1) => {
                let id = crate::http::query::percent_decode(path_segments[0]);
                self.handle_delete(&id, req).await
            }

            // PATCH /api/products/{id} - Partial update (JSON merge)
            (&Method::PATCH, 1) => {
                let id = crate::http::query::percent_decode(path_segments[0]);
                self.handle_patch(&id, req).await
            }

            _ => {
                // Provide 405 Method Not Allowed for known resources with wrong methods
                let resp = if path_segments.is_empty() {
                    // Collection root: allow GET, POST
                    self.method_not_allowed_response("GET, POST")
                } else if path_segments.len() == 1 {
                    let seg = path_segments[0];
                    if seg == "count" || seg == "random-id" || seg == "stream" {
                        // Only GET allowed
                        self.method_not_allowed_response("GET")
                    } else if seg == "_bulk" {
                        // Only POST allowed
                        self.method_not_allowed_response("POST")
                    } else {
                        // Item resource: GET, PUT, PATCH, DELETE allowed
                        self.method_not_allowed_response("GET, PUT, PATCH, DELETE")
                    }
                } else {
                    // Unknown nested path → 404
                    self.not_found_response()
                };
                Ok(resp)
            }
        }
    }

    /// GET /api/{model} - List all items with filtering, sorting, and pagination
    async fn handle_list(&self, req: &Req) -> Result<Resp, Infallible> {
        use crate::http::query::{
            compare_json_values, matches_filter, parse_query_params, DEFAULT_MAX_TAKE,
        };

        // Extract permissions from request if extractor is provided
        let user_perms: Vec<String> =
            self.permission_extractor.as_ref().map(|f| f(req)).unwrap_or_default();

        // Parse query parameters
        let query_str = req.uri().query().unwrap_or("");
        let params = parse_query_params(query_str);

        // Clone readable items while holding the lock, then release before expensive transforms
        let json_items: Vec<serde_json::Value> = {
            let storage = self.storage.read().await;
            storage
                .values()
                .filter(|item| item.can_read(&user_perms))
                .filter_map(|item| serde_json::to_value(item).ok())
                .collect()
        };

        let mut json_items = json_items;

        // Apply filters
        if !params.filters.is_empty() {
            json_items.retain(|item| params.filters.iter().all(|f| matches_filter(item, f)));
        }

        let total = json_items.len() as u64;

        // Apply sorting
        if let Some(ref sort) = params.sort {
            let field = sort.field.clone();
            let desc = sort.descending;
            json_items.sort_by(|a, b| {
                let va = a.get(&field).unwrap_or(&serde_json::Value::Null);
                let vb = b.get(&field).unwrap_or(&serde_json::Value::Null);
                let ord = compare_json_values(va, vb);
                if desc {
                    ord.reverse()
                } else {
                    ord
                }
            });
        }

        // Apply pagination
        let skip = params.skip as usize;
        if skip > 0 && skip < json_items.len() {
            json_items = json_items.into_iter().skip(skip).collect();
        } else if skip >= json_items.len() && !json_items.is_empty() {
            json_items.clear();
        }

        // Apply take limit (use default max if not specified to prevent unbounded responses)
        let effective_take = params.take.unwrap_or(DEFAULT_MAX_TAKE) as usize;
        let has_more = json_items.len() > effective_take;
        json_items.truncate(effective_take);

        // Build wrapper response
        let response = serde_json::json!({
            "data": json_items,
            "total": total,
            "skip": params.skip,
            "take": effective_take,
            "has_more": has_more,
        });

        match serde_json::to_string(&response) {
            Ok(json) => Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(body_from(json))
                .unwrap()),
            Err(_) => Ok(self.internal_error_response()),
        }
    }

    /// GET /api/{model}/stream - SSE real-time change subscription
    async fn handle_sse_stream(&self) -> Result<Resp, Infallible> {
        match &self.sse_broadcaster {
            Some(broadcaster) => {
                let model_name = T::http_base_path();
                Ok(broadcaster.create_sse_response(model_name).await)
            }
            None => Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header("content-type", "application/json")
                .body(body_from(r#"{"error":"SSE not enabled"}"#))
                .unwrap()),
        }
    }

    /// Broadcast an SSE event if the broadcaster is configured
    async fn broadcast_sse(&self, operation: &str, item: &T) {
        if let Some(ref broadcaster) = self.sse_broadcaster {
            if let Ok(data) = serde_json::to_value(item) {
                broadcaster.broadcast(T::http_base_path(), operation, data).await;
            }
        }
    }

    /// POST /api/{model} - Create new item
    async fn handle_create(&self, req: Req) -> Result<Resp, Infallible> {
        // Agnostic write enforcement using permission_extractor + can_write()
        let extracted_perms: Option<Vec<String>> =
            self.permission_extractor.as_ref().map(|f| f(&req));

        // Extract role BEFORE consuming body (if needed for legacy fallback)
        let extracted_role = if extracted_perms.is_none() {
            self.extract_role_from_request(&req).await
        } else {
            None
        };

        // Validate content type
        if !Self::has_json_content_type(&req) {
            return Ok(self.unsupported_media_type_response());
        }
        // Enforce max body size (single)
        if let Some(cl) = Self::content_length(&req) {
            if cl > Self::max_body_bytes_single() {
                return Ok(self.entity_too_large_response(Self::max_body_bytes_single()));
            }
        }
        // Parse request body (bounded)
        let body_bytes = match req.into_body().collect().await.map(|c| c.to_bytes()) {
            Ok(bytes) => bytes,
            Err(_) => return Ok(self.bad_request_response("Invalid body")),
        };
        if body_bytes.len() > Self::max_body_bytes_single() {
            return Ok(self.entity_too_large_response(Self::max_body_bytes_single()));
        }

        let mut item: T = match serde_json::from_slice(&body_bytes) {
            Ok(item) => item,
            Err(_) => return Ok(self.bad_request_response("Invalid JSON")),
        };

        // If extractor provided, enforce can_write() using extracted permissions
        if let Some(ref perms) = extracted_perms {
            if !item.can_write(perms) {
                return Ok(Response::builder()
                    .status(StatusCode::FORBIDDEN)
                    .header("content-type", "application/json")
                    .body(body_from(r#"{"error":"Insufficient permissions"}"#))
                    .unwrap());
            }
        } else if let Some(checker) = &self.permission_checker {
            // Permission checker configured - authentication REQUIRED
            let role = match extracted_role {
                Some(r) => r,
                None => {
                    // No token provided - REJECT
                    return Ok(Response::builder()
                        .status(StatusCode::UNAUTHORIZED)
                        .header("content-type", "application/json")
                        .body(body_from(r#"{"error":"Authentication required"}"#))
                        .unwrap());
                }
            };

            // Check permissions
            let model_name = std::any::type_name::<T>().split("::").last().unwrap_or("Item");
            let specific_perm = format!("{}Write", model_name);
            if !checker.has_permission(&role, &specific_perm)
                && !checker.has_permission(&role, "Write")
            {
                return Ok(Response::builder()
                    .status(StatusCode::FORBIDDEN)
                    .header("content-type", "application/json")
                    .body(body_from(r#"{"error":"Insufficient permissions"}"#))
                    .unwrap());
            }
        }

        // Validate the model
        if let Err(validation_error) = item.validate() {
            return Ok(self.bad_request_response(&validation_error));
        }

        // Enforce unique constraints
        if let Err(unique_err) = self.check_unique_constraints(&item, None).await {
            return Ok(self.json_error_response(StatusCode::CONFLICT, &unique_err));
        }

        // Apply lifecycle rules
        if let Err(lifecycle_error) = item.apply_lifecycle() {
            return Ok(self.bad_request_response(&lifecycle_error));
        }

        let primary_key = item.get_primary_key();

        // RAFT INTEGRATION: Check if consensus is required
        #[cfg(feature = "cluster")]
        {
            if let Some(consensus_arc) = &self.consensus {
                log::debug!("Raft: Proposing create operation for item {}", primary_key);

                // Real Raft consensus proposal
                match consensus_arc
                    .read()
                    .await
                    .propose_create(item.clone(), primary_key.clone())
                    .await
                {
                    Ok(_) => {
                        log::info!("Raft: Consensus achieved, applying operation locally");

                        // Apply to local storage after successful consensus
                        // Use the item's actual ID as key, not the placeholder
                        let actual_key = serde_json::to_value(&item)
                            .ok()
                            .and_then(|v| {
                                v.get("id").and_then(|id| id.as_str().map(|s| s.to_string()))
                            })
                            .unwrap_or_else(|| primary_key.clone());

                        log::debug!(
                            "DEBUG: primary_key = {}, actual_key = {}",
                            primary_key,
                            actual_key
                        );
                        log::debug!(
                            "DEBUG: item JSON = {}",
                            serde_json::to_string(&item).unwrap_or_default()
                        );

                        {
                            let mut storage = self.storage.write().await;
                            storage.insert(actual_key.clone(), item.clone());
                            log::debug!("DEBUG: Storage now has {} items", storage.len());
                        }

                        if (self.persist_to_event_store("Created", &item).await).is_err() {
                            return Ok(self.internal_error_response());
                        }

                        log::info!(
                            "Raft: Successfully replicated item {} across cluster",
                            primary_key
                        );
                    }
                    Err(e) => {
                        return Ok(self.json_error_response(
                            StatusCode::SERVICE_UNAVAILABLE,
                            &format!("Consensus failed: {}", e),
                        ));
                    }
                }
            } else {
                // Local-only mode (no replication)
                {
                    let mut storage = self.storage.write().await;
                    storage.insert(primary_key.clone(), item.clone());
                }

                if (self.persist_to_event_store("Created", &item).await).is_err() {
                    return Ok(self.internal_error_response());
                }

                log::debug!("Local: Item {} stored locally only", primary_key);
            }
        }
        #[cfg(not(feature = "cluster"))]
        {
            // Local-only mode (no replication)
            {
                let mut storage = self.storage.write().await;
                storage.insert(primary_key.clone(), item.clone());
            }

            if (self.persist_to_event_store("Created", &item).await).is_err() {
                return Ok(self.internal_error_response());
            }

            log::debug!("Local: Item {} stored locally only", primary_key);
        }

        self.broadcast_sse("create", &item).await;

        match serde_json::to_string(&item) {
            Ok(json) => Ok(Response::builder()
                .status(StatusCode::CREATED)
                .header("content-type", "application/json")
                .body(body_from(json))
                .unwrap()),
            Err(_) => Ok(self.internal_error_response()),
        }
    }

    /// POST /api/{model}/_bulk - Create multiple items
    async fn handle_bulk_create(&self, req: Req) -> Result<Resp, Infallible> {
        // Validate content type
        if !Self::has_json_content_type(&req) {
            return Ok(self.unsupported_media_type_response());
        }
        // Enforce max body size (bulk)
        if let Some(cl) = Self::content_length(&req) {
            if cl > Self::max_body_bytes_bulk() {
                return Ok(self.entity_too_large_response(Self::max_body_bytes_bulk()));
            }
        }
        // Parse request body as array of items (bounded)
        let body_bytes = match req.into_body().collect().await.map(|c| c.to_bytes()) {
            Ok(bytes) => bytes,
            Err(_) => return Ok(self.bad_request_response("Invalid body")),
        };
        if body_bytes.len() > Self::max_body_bytes_bulk() {
            return Ok(self.entity_too_large_response(Self::max_body_bytes_bulk()));
        }

        let mut items: Vec<T> = match serde_json::from_slice(&body_bytes) {
            Ok(items) => items,
            Err(_) => return Ok(self.bad_request_response("Invalid JSON array")),
        };

        let mut created: Vec<T> = Vec::with_capacity(items.len());
        let disable_consensus: bool = std::env::var("LT_DISABLE_CONSENSUS")
            .ok()
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        // Process sequentially for simplicity and determinism
        for mut item in items.drain(..) {
            if let Err(e) = item.validate() {
                return Ok(self.bad_request_response(&e));
            }
            // Enforce unique constraints
            if let Err(unique_err) = self.check_unique_constraints(&item, None).await {
                return Ok(self.json_error_response(StatusCode::CONFLICT, &unique_err));
            }
            if let Err(e) = item.apply_lifecycle() {
                return Ok(self.bad_request_response(&e));
            }

            let primary_key = item.get_primary_key();

            #[cfg(feature = "cluster")]
            {
                if let Some(consensus_arc) = &self.consensus {
                    if !disable_consensus {
                        // Consensus path
                        match consensus_arc
                            .read()
                            .await
                            .propose_create(item.clone(), primary_key.clone())
                            .await
                        {
                            Ok(_) => {
                                // Apply to local storage after successful consensus
                                let actual_key = serde_json::to_value(&item)
                                    .ok()
                                    .and_then(|v| {
                                        v.get("id")
                                            .and_then(|id| id.as_str().map(|s| s.to_string()))
                                    })
                                    .unwrap_or_else(|| primary_key.clone());
                                {
                                    let mut storage = self.storage.write().await;
                                    storage.insert(actual_key, item.clone());
                                }
                                if (self.persist_to_event_store("Created", &item).await).is_err() {
                                    return Ok(self.internal_error_response());
                                }
                                created.push(item);
                            }
                            Err(e) => {
                                return Ok(self.json_error_response(
                                    StatusCode::SERVICE_UNAVAILABLE,
                                    &format!("Consensus failed: {}", e),
                                ));
                            }
                        }
                    } else {
                        // Consensus disabled -> local path
                        {
                            let mut storage = self.storage.write().await;
                            storage.insert(primary_key.clone(), item.clone());
                        }
                        if (self.persist_to_event_store("Created", &item).await).is_err() {
                            return Ok(self.internal_error_response());
                        }
                        created.push(item);
                    }
                } else {
                    // No consensus configured -> local path
                    {
                        let mut storage = self.storage.write().await;
                        storage.insert(primary_key.clone(), item.clone());
                    }
                    if (self.persist_to_event_store("Created", &item).await).is_err() {
                        return Ok(self.internal_error_response());
                    }
                    created.push(item);
                }
            }
            #[cfg(not(feature = "cluster"))]
            {
                // No consensus configured -> local path
                let _ = disable_consensus; // suppress unused warning
                {
                    let mut storage = self.storage.write().await;
                    storage.insert(primary_key.clone(), item.clone());
                }
                if (self.persist_to_event_store("Created", &item).await).is_err() {
                    return Ok(self.internal_error_response());
                }
                created.push(item);
            }
        }

        match serde_json::to_string(&created) {
            Ok(json) => Ok(Response::builder()
                .status(StatusCode::CREATED)
                .header("content-type", "application/json")
                .body(body_from(json))
                .unwrap()),
            Err(_) => Ok(self.internal_error_response()),
        }
    }

    /// GET /api/{model}/{id} - Get item by ID (declarative read filtering)
    async fn handle_get(&self, id: &str, req: &Req) -> Result<Resp, Infallible> {
        // Extract permissions from request if extractor is provided
        let user_perms: Vec<String> =
            self.permission_extractor.as_ref().map(|f| f(req)).unwrap_or_default();

        let storage = self.storage.read().await;

        match storage.get(id) {
            Some(item) => {
                if !item.can_read(&user_perms) {
                    return Ok(Response::builder()
                        .status(StatusCode::FORBIDDEN)
                        .header("content-type", "application/json")
                        .body(body_from(r#"{"error":"Insufficient permissions"}"#))
                        .unwrap());
                }
                match serde_json::to_string(item) {
                    Ok(json) => Ok(Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(body_from(json))
                        .unwrap()),
                    Err(_) => Ok(self.internal_error_response()),
                }
            }
            None => Ok(self.not_found_response()),
        }
    }

    /// PUT /api/{model}/{id} - Update item
    async fn handle_update(&self, id: &str, req: Req) -> Result<Resp, Infallible> {
        // Agnostic write enforcement using permission_extractor + can_write()
        let extracted_perms: Option<Vec<String>> =
            self.permission_extractor.as_ref().map(|f| f(&req));

        // Extract role BEFORE consuming body (if needed for legacy fallback)
        let extracted_role = if extracted_perms.is_none() {
            self.extract_role_from_request(&req).await
        } else {
            None
        };

        // Validate content type
        if !Self::has_json_content_type(&req) {
            return Ok(self.unsupported_media_type_response());
        }
        // Enforce max body size (single)
        if let Some(cl) = Self::content_length(&req) {
            if cl > Self::max_body_bytes_single() {
                return Ok(self.entity_too_large_response(Self::max_body_bytes_single()));
            }
        }
        // Parse request body (bounded)
        let body_bytes = match req.into_body().collect().await.map(|c| c.to_bytes()) {
            Ok(bytes) => bytes,
            Err(_) => return Ok(self.bad_request_response("Invalid body")),
        };
        if body_bytes.len() > Self::max_body_bytes_single() {
            return Ok(self.entity_too_large_response(Self::max_body_bytes_single()));
        }

        let mut updated_item: T = match serde_json::from_slice(&body_bytes) {
            Ok(item) => item,
            Err(_) => return Ok(self.bad_request_response("Invalid JSON")),
        };

        if let Some(ref perms) = extracted_perms {
            if !updated_item.can_write(perms) {
                return Ok(Response::builder()
                    .status(StatusCode::FORBIDDEN)
                    .header("content-type", "application/json")
                    .body(body_from(r#"{"error":"Insufficient permissions"}"#))
                    .unwrap());
            }
        } else if let Some(checker) = &self.permission_checker {
            // Permission checker configured - authentication REQUIRED
            let role = match extracted_role {
                Some(r) => r,
                None => {
                    // No token provided - REJECT
                    return Ok(Response::builder()
                        .status(StatusCode::UNAUTHORIZED)
                        .header("content-type", "application/json")
                        .body(body_from(r#"{"error":"Authentication required"}"#))
                        .unwrap());
                }
            };

            // Check permissions
            let model_name = std::any::type_name::<T>().split("::").last().unwrap_or("Item");
            let specific_perm = format!("{}Write", model_name);
            if !checker.has_permission(&role, &specific_perm)
                && !checker.has_permission(&role, "Write")
            {
                return Ok(Response::builder()
                    .status(StatusCode::FORBIDDEN)
                    .header("content-type", "application/json")
                    .body(body_from(r#"{"error":"Insufficient permissions"}"#))
                    .unwrap());
            }
        }

        // Enforce immutable fields and log audited changes
        {
            let storage = self.storage.read().await;
            if let Some(existing) = storage.get(id) {
                if let Err(immutable_err) = Self::check_immutable_fields(existing, &updated_item) {
                    return Ok(self.bad_request_response(&immutable_err));
                }
                Self::log_audited_changes(existing, &updated_item);
            }
        }

        // Validate
        if let Err(validation_error) = updated_item.validate() {
            return Ok(self.bad_request_response(&validation_error));
        }

        // Enforce unique constraints (exclude self from check)
        if let Err(unique_err) = self.check_unique_constraints(&updated_item, Some(id)).await {
            return Ok(self.json_error_response(StatusCode::CONFLICT, &unique_err));
        }

        // Apply lifecycle
        if let Err(lifecycle_error) = updated_item.apply_lifecycle() {
            return Ok(self.bad_request_response(&lifecycle_error));
        }

        // RAFT INTEGRATION: Check if consensus is required for UPDATE
        #[cfg(feature = "cluster")]
        {
            if let Some(consensus_arc) = &self.consensus {
                log::debug!("Raft: Proposing UPDATE operation for item {}", id);
                match consensus_arc
                    .read()
                    .await
                    .propose_update(updated_item.clone(), id.to_string())
                    .await
                {
                    Ok(_) => {
                        // Apply to local storage after successful consensus
                        let mut storage = self.storage.write().await;
                        if !storage.contains_key(id) {
                            return Ok(self.not_found_response());
                        }
                        storage.insert(id.to_string(), updated_item.clone());
                    }
                    Err(e) => {
                        return Ok(self.json_error_response(
                            StatusCode::SERVICE_UNAVAILABLE,
                            &format!("Consensus failed: {}", e),
                        ));
                    }
                }
            } else {
                // No consensus - update storage directly (single-node mode)
                let mut storage = self.storage.write().await;
                if !storage.contains_key(id) {
                    return Ok(self.not_found_response());
                }
                storage.insert(id.to_string(), updated_item.clone());
            }
        }
        #[cfg(not(feature = "cluster"))]
        {
            // No consensus - update storage directly (single-node mode)
            let mut storage = self.storage.write().await;
            if !storage.contains_key(id) {
                return Ok(self.not_found_response());
            }
            storage.insert(id.to_string(), updated_item.clone());
        }

        // Persist to EventStore
        if (self.persist_to_event_store("Updated", &updated_item).await).is_err() {
            return Ok(self.internal_error_response());
        }

        self.broadcast_sse("update", &updated_item).await;

        match serde_json::to_string(&updated_item) {
            Ok(json) => Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(body_from(json))
                .unwrap()),
            Err(_) => Ok(self.internal_error_response()),
        }
    }

    /// PATCH /api/{model}/{id} - Partial update via JSON merge
    async fn handle_patch(&self, id: &str, req: Req) -> Result<Resp, Infallible> {
        // Validate content type
        if !Self::has_json_content_type(&req) {
            return Ok(self.unsupported_media_type_response());
        }
        if let Some(cl) = Self::content_length(&req) {
            if cl > Self::max_body_bytes_single() {
                return Ok(self.entity_too_large_response(Self::max_body_bytes_single()));
            }
        }

        let body_bytes = match req.into_body().collect().await.map(|c| c.to_bytes()) {
            Ok(bytes) => bytes,
            Err(_) => return Ok(self.bad_request_response("Invalid body")),
        };
        if body_bytes.len() > Self::max_body_bytes_single() {
            return Ok(self.entity_too_large_response(Self::max_body_bytes_single()));
        }

        let changes: serde_json::Value = match serde_json::from_slice(&body_bytes) {
            Ok(v) => v,
            Err(_) => return Ok(self.bad_request_response("Invalid JSON")),
        };

        match self.submit_admin_edit(id, changes).await {
            Ok(item) => {
                // Broadcast SSE event
                self.broadcast_sse("patched", &item).await;

                let body = serde_json::to_string(&item).unwrap_or_default();
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(body_from(body))
                    .unwrap())
            }
            Err(e) if e.contains("not found") => Ok(self.not_found_response()),
            Err(e) => Ok(self.bad_request_response(&e)),
        }
    }

    /// DELETE /api/{model}/{id} - Delete item
    async fn handle_delete(&self, id: &str, req: Req) -> Result<Resp, Infallible> {
        // Agnostic write/delete enforcement using permission_extractor + can_write()
        let extracted_perms: Option<Vec<String>> =
            self.permission_extractor.as_ref().map(|f| f(&req));

        // First, fetch the item if present to evaluate permissions against it
        let existing_item_opt = {
            let storage = self.storage.read().await;
            storage.get(id).cloned()
        };
        if let Some(ref item) = existing_item_opt {
            if let Some(ref perms) = extracted_perms {
                if !item.can_write(perms) {
                    return Ok(Response::builder()
                        .status(StatusCode::FORBIDDEN)
                        .header("content-type", "application/json")
                        .body(body_from(r#"{"error":"Insufficient permissions"}"#))
                        .unwrap());
                }
            } else if let Some(checker) = &self.permission_checker {
                // Permission checker configured - authentication REQUIRED
                let role = match self.extract_role_from_request(&req).await {
                    Some(r) => r,
                    None => {
                        // No token provided - REJECT
                        return Ok(Response::builder()
                            .status(StatusCode::UNAUTHORIZED)
                            .header("content-type", "application/json")
                            .body(body_from(r#"{"error":"Authentication required"}"#))
                            .unwrap());
                    }
                };

                // Check permissions
                let model_name = std::any::type_name::<T>().split("::").last().unwrap_or("Item");
                let specific_perm = format!("{}Delete", model_name);
                if !checker.has_permission(&role, &specific_perm)
                    && !checker.has_permission(&role, "Delete")
                {
                    return Ok(Response::builder()
                        .status(StatusCode::FORBIDDEN)
                        .header("content-type", "application/json")
                        .body(body_from(r#"{"error":"Insufficient permissions"}"#))
                        .unwrap());
                }
            }
        }

        // RAFT INTEGRATION: Check if consensus is required for DELETE
        #[cfg(feature = "cluster")]
        {
            if let Some(consensus_arc) = &self.consensus {
                log::debug!("Raft: Proposing DELETE operation for item {}", id);
                match consensus_arc.read().await.propose_delete(id.to_string()).await {
                    Ok(_) => {
                        // Apply to local storage after successful consensus
                        let removed_item = {
                            let mut storage = self.storage.write().await;
                            storage.remove(id)
                        };

                        match removed_item {
                            Some(item) => {
                                // Persist deletion to EventStore
                                if (self.persist_to_event_store("Deleted", &item).await).is_err() {
                                    return Ok(self.internal_error_response());
                                }

                                self.broadcast_sse("delete", &item).await;

                                return Ok(Response::builder()
                                    .status(StatusCode::NO_CONTENT)
                                    .body(body_from(Bytes::new()))
                                    .unwrap());
                            }
                            None => return Ok(self.not_found_response()),
                        }
                    }
                    Err(e) => {
                        return Ok(self.json_error_response(
                            StatusCode::SERVICE_UNAVAILABLE,
                            &format!("Consensus failed: {}", e),
                        ));
                    }
                }
            }
        }
        // No consensus - delete directly (single-node mode)
        let removed_item = {
            let mut storage = self.storage.write().await;
            storage.remove(id)
        };

        match removed_item {
            Some(item) => {
                // Persist deletion to EventStore
                if (self.persist_to_event_store("Deleted", &item).await).is_err() {
                    return Ok(self.internal_error_response());
                }

                self.broadcast_sse("delete", &item).await;

                Ok(Response::builder()
                    .status(StatusCode::NO_CONTENT)
                    .body(body_from(Bytes::new()))
                    .unwrap())
            }
            None => Ok(self.not_found_response()),
        }
    }

    /// Persist operation to EventStore
    async fn persist_to_event_store(
        &self,
        operation: &str,
        item: &T,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let envelope = EventEnvelope {
            event_type: format!("{}{}", std::any::type_name::<T>(), operation),
            event_id: format!(
                "{}:{}:{}",
                std::any::type_name::<T>(),
                operation,
                item.get_primary_key()
            ),
            timestamp: chrono::Utc::now().timestamp() as u64,
            payload: serde_json::to_string(item)?,
            aggregate_id: Some(item.get_primary_key()),
            // Hash chain fields - computed automatically by EventStore when enabled
            event_hash: None,
            previous_hash: None,
        };

        let mut event_store = self.event_store.write().await;
        event_store.append_envelope(&envelope)?;
        // Flush is handled by the background flusher for high throughput

        Ok(())
    }

    // Helper methods for responses
    fn not_found_response(&self) -> Resp {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("content-type", "application/json")
            .body(body_from(r#"{"error":"Not found"}"#))
            .unwrap()
    }

    fn bad_request_response(&self, message: &str) -> Resp {
        self.json_error_response(StatusCode::BAD_REQUEST, message)
    }

    fn internal_error_response(&self) -> Resp {
        self.json_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
    }

    fn unsupported_media_type_response(&self) -> Resp {
        self.json_error_response(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported media type, expected application/json",
        )
    }

    fn entity_too_large_response(&self, max: usize) -> Resp {
        self.json_error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            &format!("request body too large (max {} bytes)", max),
        )
    }

    fn method_not_allowed_response(&self, allowed: &str) -> Resp {
        let body = serde_json::json!({"error": "method not allowed", "allow": allowed});
        Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .header("content-type", "application/json")
            .header("allow", allowed)
            .body(body_from(body.to_string()))
            .unwrap()
    }

    /// Build a JSON error response with proper escaping (prevents JSON injection)
    fn json_error_response(&self, status: StatusCode, message: &str) -> Resp {
        let body = serde_json::json!({"error": message});
        Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(body_from(body.to_string()))
            .unwrap()
    }

    #[inline]
    fn has_json_content_type(req: &Req) -> bool {
        req.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_ascii_lowercase().contains("application/json"))
            .unwrap_or(false)
    }

    #[inline]
    fn content_length(req: &Req) -> Option<usize> {
        req.headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<usize>().ok())
    }

    #[inline]
    fn max_body_bytes_single() -> usize {
        std::env::var("LT_HTTP_MAX_BODY_BYTES_SINGLE")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(2 * 1024 * 1024) // 2 MiB
    }

    #[inline]
    fn max_body_bytes_bulk() -> usize {
        std::env::var("LT_HTTP_MAX_BODY_BYTES_BULK")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(12 * 1024 * 1024) // 12 MiB
    }

    // ========================================================================
    // EVENT HISTORY - For data admin dashboard
    // ========================================================================

    /// Get all events for a specific entity (by aggregate_id)
    /// Returns a list of events showing the entity's change history
    pub async fn get_entity_history(&self, id: &str) -> Vec<EventEnvelope> {
        let event_store = self.event_store.read().await;

        // Get all events and filter by aggregate_id
        match event_store.get_all_events() {
            Ok(events) => events
                .into_iter()
                .filter_map(|event_json| serde_json::from_str::<EventEnvelope>(&event_json).ok())
                .filter(|envelope| {
                    envelope.aggregate_id.as_ref().map(|aid| aid == id).unwrap_or(false)
                })
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Get event count for a specific entity
    pub async fn get_entity_event_count(&self, id: &str) -> usize {
        self.get_entity_history(id).await.len()
    }

    /// Submit an admin edit event (event-sourced: appends new event, updates state)
    /// Returns the new state after applying the edit
    pub async fn submit_admin_edit(&self, id: &str, changes: serde_json::Value) -> Result<T, String>
    where
        T: serde::de::DeserializeOwned,
    {
        // Get current item
        let current_item = {
            let storage = self.storage.read().await;
            storage.get(id).cloned()
        };

        let mut item = current_item.ok_or_else(|| format!("Entity '{}' not found", id))?;

        // Merge changes into current item
        let mut item_json =
            serde_json::to_value(&item).map_err(|e| format!("Failed to serialize item: {}", e))?;

        if let (Some(item_obj), Some(changes_obj)) =
            (item_json.as_object_mut(), changes.as_object())
        {
            for (key, value) in changes_obj {
                item_obj.insert(key.clone(), value.clone());
            }
        }

        // Deserialize back to item
        let original = item.clone();
        item = serde_json::from_value(item_json)
            .map_err(|e| format!("Failed to apply changes: {}", e))?;

        // Enforce immutable fields and log audited changes
        Self::check_immutable_fields(&original, &item)?;
        Self::log_audited_changes(&original, &item);

        // Enforce unique constraints
        self.check_unique_constraints(&item, Some(id))
            .await
            .map_err(|e| format!("Unique constraint: {}", e))?;

        // Validate the updated item
        if let Err(validation_error) = item.validate() {
            return Err(format!("Validation failed: {}", validation_error));
        }

        // Apply lifecycle rules
        if let Err(lifecycle_error) = item.apply_lifecycle() {
            return Err(format!("Lifecycle error: {}", lifecycle_error));
        }

        // Update in-memory storage
        {
            let mut storage = self.storage.write().await;
            storage.insert(id.to_string(), item.clone());
        }

        // Persist as AdminEdit event (different from regular Updated)
        let envelope = EventEnvelope {
            event_type: format!("{}AdminEdit", std::any::type_name::<T>()),
            event_id: format!(
                "{}:AdminEdit:{}:{}",
                std::any::type_name::<T>(),
                id,
                chrono::Utc::now().timestamp_millis()
            ),
            timestamp: chrono::Utc::now().timestamp() as u64,
            payload: serde_json::to_string(&item)
                .map_err(|e| format!("Failed to serialize: {}", e))?,
            aggregate_id: Some(id.to_string()),
            // Hash chain fields - computed automatically by EventStore when enabled
            event_hash: None,
            previous_hash: None,
        };

        {
            let mut event_store = self.event_store.write().await;
            event_store
                .append_envelope(&envelope)
                .map_err(|e| format!("Failed to persist event: {}", e))?;
        }

        Ok(item)
    }
}
