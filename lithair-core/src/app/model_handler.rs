//! Type-erased model handler for LithairServer

use crate::consensus::ReplicatedModel;
use crate::http::{DeclarativeHttpHandler, HttpExposable};
use crate::lifecycle::LifecycleAware;
use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use hyper::body::Incoming;
use hyper::{Request, Response};
use std::convert::Infallible;
use std::sync::Arc;

type RespBody = BoxBody<Bytes, Infallible>;
type Req = Request<Incoming>;
type Resp = Response<RespBody>;

/// Per-model storage and memory statistics (issue #72).
///
/// Surfaced through:
/// - `GET /_admin/data/models/{name}/_stats` — JSON, for ad-hoc debugging
/// - `GET /metrics` — Prometheus text, with `model="..."` label, for scraping
///
/// `approx_ram_bytes` is a **sample-based estimate**, not exact. The handler
/// JSON-serializes up to 16 items, averages the byte size, and multiplies by
/// the live item count. Biased upward vs in-memory `T` (JSON is verbose) and
/// biased downward vs total heap usage (ignores indexes, replication state,
/// audit trails). Use for order-of-magnitude capacity planning, not billing.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelStats {
    /// Logical name of the model (e.g. `"Mail"`).
    pub model: String,
    /// Current number of items held in RAM.
    pub item_count: usize,
    /// Approximate RAM cost (see struct docs for methodology).
    pub approx_ram_bytes: u64,
    /// Size of the on-disk `events.raftlog` file in bytes. `0` if the file
    /// isn't found (e.g. memory-only model, or compaction in progress).
    pub raftlog_size_bytes: u64,
    /// Events appended since the last snapshot/compaction. Currently always
    /// `null` — surfacing it is gated on issue #69 (auto-compaction) tracking
    /// the counter explicitly.
    // TODO(#72/#69): wire from snapshot store once compaction tracks it.
    pub events_since_last_compaction: Option<u64>,
    /// Wall-clock time of the last compaction in RFC 3339. Currently always
    /// `null` — see `events_since_last_compaction`.
    // TODO(#72/#69): wire from snapshot store once compaction tracks it.
    pub last_compaction_at: Option<String>,
}

/// Type-erased trait for model handlers
#[async_trait::async_trait]
pub trait ModelHandler: Send + Sync {
    /// Handle HTTP request for this model
    async fn handle_request(&self, req: Req, path_segments: &[&str]) -> Result<Resp, Infallible>;

    // ========================================================================
    // DATA ADMIN METHODS - For admin dashboard and external API access
    // ========================================================================

    /// Get all items as JSON array
    async fn get_all_data_json(&self) -> serde_json::Value;

    /// Get at most `limit` items as a JSON array, for sampled diagnostics.
    ///
    /// The default impl falls back to `get_all_data_json().await` then trims
    /// to `limit`, which still pays the cost of cloning every item — a real
    /// liability for large models. Implementors backed by a storage primitive
    /// that supports bounded iteration MUST override this to avoid that cost
    /// (see Gemini review on PR #83). The order is unspecified — the result
    /// is a representative sample, not a stable selection.
    async fn get_sample_data_json(&self, limit: usize) -> serde_json::Value {
        let all = self.get_all_data_json().await;
        match all {
            serde_json::Value::Array(items) => {
                serde_json::Value::Array(items.into_iter().take(limit).collect())
            }
            other => other,
        }
    }

    /// Get single item by ID as JSON
    async fn get_item_json(&self, id: &str) -> Option<serde_json::Value>;

    /// Get total count of items
    async fn get_count(&self) -> usize;

    /// Export all data with metadata (for backup/external access)
    async fn export_json(&self) -> serde_json::Value;

    /// Compute per-model storage and memory stats (issue #72).
    ///
    /// `data_path` is the on-disk directory passed at registration time.
    /// The default impl looks for `events.raftlog` under that path when
    /// reporting `raftlog_size_bytes`; absent file -> `0`.
    ///
    /// `approx_ram_bytes` samples up to 16 items via `get_sample_data_json`,
    /// JSON-serializes them, averages, multiplies by the live count. See
    /// [`ModelStats`] for methodology and caveats. Implementors with cheaper
    /// sizing primitives are encouraged to override either this method or
    /// `get_sample_data_json` (whichever is cheaper to specialize).
    async fn get_stats(&self, data_path: &str) -> ModelStats {
        // Sample size for the RAM estimator — matched on both surfaces (JSON
        // + Prometheus). Bumping this trades estimate stability for cost; 16
        // empirically lands within a few percent of the full-scan number on
        // homogeneous models without paying the full clone tax.
        const SAMPLE_LIMIT: usize = 16;

        let count = self.get_count().await;
        // Only fetch a bounded sample — Gemini PR #83 review flagged that the
        // previous impl called `get_all_data_json` and then `.take(16)`, which
        // still cloned every item. For a model with 50k items × 4 KB each
        // that's ~200 MB allocated per stats call. `get_sample_data_json`
        // bounds the fetch at the storage layer when the implementor supports
        // it; the default fallback still trims after the fact, but the trait
        // contract pushes implementors toward the cheap path.
        let approx_ram_bytes = if count == 0 {
            0
        } else {
            let sample = self.get_sample_data_json(SAMPLE_LIMIT).await;
            if let Some(arr) = sample.as_array() {
                let sample_n = arr.len().min(SAMPLE_LIMIT);
                if sample_n == 0 {
                    0
                } else {
                    let total: u64 = arr
                        .iter()
                        .take(sample_n)
                        .map(|v| serde_json::to_string(v).map(|s| s.len() as u64).unwrap_or(0))
                        .sum();
                    let avg = total / sample_n as u64;
                    avg.saturating_mul(count as u64)
                }
            } else {
                0
            }
        };

        // Use `tokio::fs::metadata` so this `async fn` doesn't park the
        // runtime on a blocking syscall — flagged by Gemini on PR #83.
        let raftlog_size_bytes =
            tokio::fs::metadata(std::path::Path::new(data_path).join("events.raftlog"))
                .await
                .map_or(0, |m| m.len());

        ModelStats {
            model: self.model_name().to_string(),
            item_count: count,
            approx_ram_bytes,
            raftlog_size_bytes,
            events_since_last_compaction: None,
            last_compaction_at: None,
        }
    }

    /// Get model name
    fn model_name(&self) -> &str;

    /// Get base API path for this model
    fn base_path(&self) -> &str;

    // ========================================================================
    // EVENT HISTORY - For data admin dashboard history visualization
    // ========================================================================

    /// Get event history for a specific entity (by aggregate_id)
    /// Returns list of events with timestamps showing how the entity changed over time
    async fn get_entity_history(&self, id: &str) -> serde_json::Value;

    /// Get total event count for a specific entity
    async fn get_entity_event_count(&self, id: &str) -> usize;

    /// Submit an edit event (event-sourced update - never replaces, always appends)
    /// Returns the new state after applying the edit event
    async fn submit_edit_event(
        &self,
        id: &str,
        changes: serde_json::Value,
    ) -> Result<serde_json::Value, String>;

    // ========================================================================
    // REPLICATION METHODS - For cluster data replication from leader to followers
    // ========================================================================

    /// Apply a replicated item from leader (type-erased via JSON)
    /// Called by followers when receiving replication data from leader
    async fn apply_replicated_item_json(&self, item_json: serde_json::Value) -> Result<(), String>;

    /// Apply multiple replicated items from leader (bulk replication)
    async fn apply_replicated_items_json(
        &self,
        items_json: Vec<serde_json::Value>,
    ) -> Result<usize, String>;

    /// Apply a replicated UPDATE from leader (type-erased via JSON)
    /// Called by followers when receiving UPDATE replication data from leader
    async fn apply_replicated_update_json(
        &self,
        id: &str,
        item_json: serde_json::Value,
    ) -> Result<(), String>;

    /// Apply a replicated DELETE from leader
    /// Called by followers when receiving DELETE replication from leader
    async fn apply_replicated_delete_json(&self, id: &str) -> Result<bool, String>;

    /// Get the schema specification for this model (for OpenAPI generation)
    /// Returns None if no schema spec is available
    fn schema_spec(&self) -> Option<crate::schema::ModelSpec> {
        None
    }

    /// Set the SSE broadcaster for real-time change notifications (no-op by default)
    fn set_sse_broadcaster(&mut self, _broadcaster: Arc<crate::http::sse::SseEventBroadcaster>) {}

    /// Enable or disable the session-presence gate for this model's
    /// auto-generated `/api/{model}` endpoints (issue #78).
    ///
    /// Called by `LithairServer::serve()` after the model factory runs, so
    /// that the global `with_models_require_session(...)` flag applies
    /// regardless of the order in which the builder methods were chained.
    ///
    /// Default impl is a no-op so non-Declarative handlers (e.g. future
    /// custom `ModelHandler` impls) stay unaffected.
    fn set_require_session(&mut self, _require: bool) {}

    /// Attach a type-erased session store to this handler.
    ///
    /// Called by `LithairServer::serve()` after the model factory runs to
    /// thread the configured session store (set via `with_sessions(...)` or
    /// `with_rbac_config(...)`) into every registered model. This makes the
    /// session lookup available regardless of the builder-method ordering.
    ///
    /// Default impl is a no-op for custom handlers that don't use sessions.
    fn set_session_store_any(&mut self, _store: Arc<dyn std::any::Any + Send + Sync>) {}

    /// Return a shared handle to this model's underlying `EventStore`, when
    /// one exists (issue #69).
    ///
    /// Used by `LithairServer::serve()` to wire the builder-driven
    /// auto-compaction background task: the task periodically calls
    /// `event_count()` on the store and triggers `truncate_events()` when the
    /// count crosses the configured threshold.
    ///
    /// Returns `None` for handlers that don't back themselves with an
    /// `EventStore` (custom in-memory `ModelHandler` impls, future
    /// non-event-sourced models). When `None`, auto-compaction silently skips
    /// the model — the feature is opt-in and best-effort by design.
    fn event_store_arc(
        &self,
    ) -> Option<Arc<tokio::sync::RwLock<crate::engine::events::EventStore>>> {
        None
    }
}

/// Wrapper for DeclarativeHttpHandler that implements ModelHandler
pub struct DeclarativeModelHandler<T>
where
    T: HttpExposable + LifecycleAware + ReplicatedModel,
{
    handler: DeclarativeHttpHandler<T>,
    model_name: String,
    base_path: String,
    cached_schema_spec: Option<crate::schema::ModelSpec>,
}

impl<T> DeclarativeModelHandler<T>
where
    T: HttpExposable + LifecycleAware + ReplicatedModel,
{
    pub async fn new(data_path: String) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let handler = DeclarativeHttpHandler::<T>::new_with_replay(&data_path).await?;
        let model_name =
            std::any::type_name::<T>().split("::").last().unwrap_or("Unknown").to_string();
        let base_path = T::http_base_path().to_string();
        Ok(Self { handler, model_name, base_path, cached_schema_spec: None })
    }

    /// Override the base path used in metadata and JSON responses (e.g., `export_json`).
    ///
    /// Note: This does NOT change HTTP routing. Routing is determined by
    /// `ModelRegistrationInfo::base_path` set during `LithairServerBuilder` setup.
    /// To change routing, configure the base path in the builder instead.
    pub fn with_base_path(mut self, path: impl Into<String>) -> Self {
        self.base_path = path.into();
        self
    }

    /// Set the permission checker for RBAC enforcement
    pub fn with_permission_checker(
        mut self,
        checker: Arc<dyn crate::rbac::PermissionChecker>,
    ) -> Self {
        self.handler = self.handler.with_permission_checker(checker);
        self
    }

    /// Set the session store for extracting user roles
    pub fn with_session_store<S: 'static + Send + Sync>(mut self, store: Arc<S>) -> Self {
        self.handler = self.handler.with_session_store(store);
        self
    }

    /// Set the cached schema spec for OpenAPI generation
    pub fn with_schema_spec(mut self, spec: crate::schema::ModelSpec) -> Self {
        self.cached_schema_spec = Some(spec);
        self
    }

    /// Set session store directly (consuming-self variant for callers that
    /// own the handler before it has been wrapped in `Arc` — currently used
    /// by `LithairServerBuilder::with_model_full`).
    ///
    /// At-runtime wiring (after the handler is already in `Arc<dyn
    /// ModelHandler>`) goes through the trait method
    /// [`ModelHandler::set_session_store_any`] instead.
    pub(crate) fn set_session_store_any_owned(
        mut self,
        store: Arc<dyn std::any::Any + Send + Sync>,
    ) -> Self {
        self.handler.session_store = Some(store);
        self
    }
}

#[async_trait::async_trait]
impl<T> ModelHandler for DeclarativeModelHandler<T>
where
    T: HttpExposable
        + LifecycleAware
        + ReplicatedModel
        + serde::Serialize
        + serde::de::DeserializeOwned
        + 'static,
{
    async fn handle_request(&self, req: Req, path_segments: &[&str]) -> Result<Resp, Infallible> {
        self.handler.handle_request(req, path_segments).await
    }

    async fn get_all_data_json(&self) -> serde_json::Value {
        let items = self.handler.get_all_items().await;
        serde_json::to_value(&items).unwrap_or(serde_json::json!([]))
    }

    async fn get_sample_data_json(&self, limit: usize) -> serde_json::Value {
        // Short-circuit at the storage layer so a model with 50k items
        // doesn't allocate 50k clones just for a 16-item RAM estimate
        // (Gemini PR #83 review). `DeclarativeHttpHandler` exposes a bounded
        // iterator over the in-memory `HashMap` that takes only `limit`
        // values under a read lock.
        let items = self.handler.get_sample_items(limit).await;
        serde_json::to_value(&items).unwrap_or(serde_json::json!([]))
    }

    async fn get_item_json(&self, id: &str) -> Option<serde_json::Value> {
        let items = self.handler.get_all_items().await;
        items
            .into_iter()
            .find(|item| item.get_primary_key() == id)
            .and_then(|item| serde_json::to_value(&item).ok())
    }

    async fn get_count(&self) -> usize {
        self.handler.get_all_items().await.len()
    }

    async fn export_json(&self) -> serde_json::Value {
        let items = self.handler.get_all_items().await;
        let count = items.len();
        serde_json::json!({
            "model": self.model_name,
            "base_path": self.base_path,
            "count": count,
            "exported_at": chrono::Utc::now().to_rfc3339(),
            "data": serde_json::to_value(&items).unwrap_or(serde_json::json!([]))
        })
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }

    fn base_path(&self) -> &str {
        &self.base_path
    }

    async fn get_entity_history(&self, id: &str) -> serde_json::Value {
        let events = self.handler.get_entity_history(id).await;
        let history: Vec<serde_json::Value> = events
            .into_iter()
            .map(|envelope| {
                serde_json::json!({
                    "event_type": envelope.event_type,
                    "event_id": envelope.event_id,
                    "timestamp": envelope.timestamp,
                    "timestamp_human": chrono::DateTime::from_timestamp(envelope.timestamp as i64, 0)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    "aggregate_id": envelope.aggregate_id,
                    // Include snapshot of data at this point (the payload)
                    "data": serde_json::from_str::<serde_json::Value>(&envelope.payload).ok()
                })
            })
            .collect();

        serde_json::json!({
            "entity_id": id,
            "model": self.model_name,
            "event_count": history.len(),
            "events": history
        })
    }

    async fn get_entity_event_count(&self, id: &str) -> usize {
        self.handler.get_entity_event_count(id).await
    }

    async fn submit_edit_event(
        &self,
        id: &str,
        changes: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let updated_item = self.handler.submit_admin_edit(id, changes).await?;
        serde_json::to_value(&updated_item)
            .map_err(|e| format!("Failed to serialize result: {}", e))
    }

    async fn apply_replicated_item_json(&self, item_json: serde_json::Value) -> Result<(), String> {
        // Deserialize JSON to typed item
        let item: T = serde_json::from_value(item_json)
            .map_err(|e| format!("Failed to deserialize replicated item: {}", e))?;

        // Apply using the typed method on the handler
        self.handler.apply_replicated_item(item).await
    }

    async fn apply_replicated_items_json(
        &self,
        items_json: Vec<serde_json::Value>,
    ) -> Result<usize, String> {
        // Deserialize each JSON to typed item
        let items: Vec<T> = items_json
            .into_iter()
            .map(|json| serde_json::from_value::<T>(json))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to deserialize replicated items: {}", e))?;

        // Apply using the typed method on the handler
        self.handler.apply_replicated_items(items).await
    }

    async fn apply_replicated_update_json(
        &self,
        id: &str,
        item_json: serde_json::Value,
    ) -> Result<(), String> {
        // Deserialize JSON to typed item
        let item: T = serde_json::from_value(item_json)
            .map_err(|e| format!("Failed to deserialize replicated update: {}", e))?;

        // Apply using the typed method on the handler
        self.handler.apply_replicated_update(id, item).await
    }

    async fn apply_replicated_delete_json(&self, id: &str) -> Result<bool, String> {
        // Apply using the typed method on the handler
        self.handler.apply_replicated_delete(id).await
    }

    fn schema_spec(&self) -> Option<crate::schema::ModelSpec> {
        self.cached_schema_spec.clone()
    }

    fn set_sse_broadcaster(&mut self, broadcaster: Arc<crate::http::sse::SseEventBroadcaster>) {
        self.handler.sse_broadcaster = Some(broadcaster);
    }

    fn set_require_session(&mut self, require: bool) {
        self.handler.set_require_session(require);
    }

    fn set_session_store_any(&mut self, store: Arc<dyn std::any::Any + Send + Sync>) {
        self.handler.session_store = Some(store);
    }

    fn event_store_arc(
        &self,
    ) -> Option<Arc<tokio::sync::RwLock<crate::engine::events::EventStore>>> {
        Some(Arc::clone(self.handler.get_event_store()))
    }
}
