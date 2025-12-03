//! Type-erased model handler for LithairServer

use crate::http::{DeclarativeHttpHandler, HttpExposable};
use crate::lifecycle::LifecycleAware;
use crate::consensus::ReplicatedModel;
use http_body_util::combinators::BoxBody;
use hyper::body::Incoming;
use hyper::{Request, Response};
use bytes::Bytes;
use std::convert::Infallible;
use std::sync::Arc;

type RespBody = BoxBody<Bytes, Infallible>;
type Req = Request<Incoming>;
type Resp = Response<RespBody>;

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

    /// Get single item by ID as JSON
    async fn get_item_json(&self, id: &str) -> Option<serde_json::Value>;

    /// Get total count of items
    async fn get_count(&self) -> usize;

    /// Export all data with metadata (for backup/external access)
    async fn export_json(&self) -> serde_json::Value;

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
    async fn submit_edit_event(&self, id: &str, changes: serde_json::Value) -> Result<serde_json::Value, String>;
}

/// Wrapper for DeclarativeHttpHandler that implements ModelHandler
pub struct DeclarativeModelHandler<T>
where
    T: HttpExposable + LifecycleAware + ReplicatedModel,
{
    handler: DeclarativeHttpHandler<T>,
    model_name: String,
    base_path: String,
}

impl<T> DeclarativeModelHandler<T>
where
    T: HttpExposable + LifecycleAware + ReplicatedModel,
{
    pub async fn new(data_path: String) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let handler = DeclarativeHttpHandler::<T>::new_with_replay(&data_path).await?;
        let model_name = std::any::type_name::<T>().split("::").last().unwrap_or("Unknown").to_string();
        let base_path = T::http_base_path().to_string();
        Ok(Self {
            handler,
            model_name,
            base_path,
        })
    }
    
    pub fn with_base_path(mut self, path: impl Into<String>) -> Self {
        self.base_path = path.into();
        self
    }
    
    /// Set the permission checker for RBAC enforcement
    pub fn with_permission_checker(mut self, checker: Arc<dyn crate::rbac::PermissionChecker>) -> Self {
        self.handler = self.handler.with_permission_checker(checker);
        self
    }
    
    /// Set the session store for extracting user roles
    pub fn with_session_store<S: 'static + Send + Sync>(mut self, store: Arc<S>) -> Self {
        self.handler = self.handler.with_session_store(store);
        self
    }
    
    /// Set session store directly (for type-erased Arc<dyn Any>)
    pub(crate) fn set_session_store_any(mut self, store: Arc<dyn std::any::Any + Send + Sync>) -> Self {
        self.handler.session_store = Some(store);
        self
    }
}

#[async_trait::async_trait]
impl<T> ModelHandler for DeclarativeModelHandler<T>
where
    T: HttpExposable + LifecycleAware + ReplicatedModel + serde::Serialize + serde::de::DeserializeOwned + 'static,
{
    async fn handle_request(&self, req: Req, path_segments: &[&str]) -> Result<Resp, Infallible> {
        self.handler.handle_request(req, path_segments).await
    }
    
    async fn get_all_data_json(&self) -> serde_json::Value {
        let items = self.handler.get_all_items().await;
        serde_json::to_value(&items).unwrap_or(serde_json::json!([]))
    }
    
    async fn get_item_json(&self, id: &str) -> Option<serde_json::Value> {
        let items = self.handler.get_all_items().await;
        items.into_iter()
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

    async fn submit_edit_event(&self, id: &str, changes: serde_json::Value) -> Result<serde_json::Value, String> {
        let updated_item = self.handler.submit_admin_edit(id, changes).await?;
        serde_json::to_value(&updated_item)
            .map_err(|e| format!("Failed to serialize result: {}", e))
    }
}
