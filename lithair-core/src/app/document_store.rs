//! One HTTP implementation of the generated model routes for models whose
//! records live in an external document store (`lithair-turso`,
//! `lithair-postgres`, RFC 296 phase 3). Adapters implement [`DocumentStore`];
//! routes, sessions, permissions, pagination and status codes are shared, so the
//! backends cannot drift apart.
use super::{response, Method, ModelHandler, ModelStats, RouteRequest, RouteResponse, StatusCode};
use crate::http::HttpExposable;
use http_body_util::{BodyExt, Full, Limited};
use serde_json::{json, Value};
use std::{any::Any, convert::Infallible, sync::Arc};

/// A page of SQL candidates, filtered by read permissions. `next_offset`
/// counts candidates: a short or empty `data` page can still continue.
pub struct ListedPage<T> {
    pub data: Vec<T>,
    pub next_offset: Option<u32>,
}

/// How a store failure maps to an HTTP status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentErrorKind {
    Forbidden,
    NotFound,
    Conflict,
    /// Client input or model validation: 400 with the error's message.
    InvalidInput,
    /// Anything else: 500 without details.
    Internal,
}

/// The repository operations behind the generated routes. Implemented by the
/// external storage adapters; permissions are always enforced by the store.
#[async_trait::async_trait]
pub trait DocumentStore: Send + Sync + 'static {
    type Model: HttpExposable;
    type Error: std::fmt::Display + Send;
    /// Backend name used in diagnostics, e.g. `"PostgreSQL"`.
    const BACKEND: &'static str;
    /// Largest accepted request body (an encoded document).
    const MAX_DOCUMENT_BYTES: usize;

    /// Top-level string fields allowed as `?field=value` equality filters.
    fn filter_fields(&self) -> &'static [&'static str];
    /// Permission names resolved through the configured RBAC checker.
    fn permissions(&self) -> &'static [&'static str];
    fn error_kind(error: &Self::Error) -> DocumentErrorKind;
    /// L1 copy counters when the model declares a cache (RFC 304).
    fn cache_stats(&self) -> Option<super::CacheStats> {
        None
    }

    async fn list_page(
        &self,
        limit: u32,
        offset: u32,
        filter: Option<(&str, &str)>,
        permissions: &[String],
    ) -> Result<ListedPage<Self::Model>, Self::Error>;
    async fn get(
        &self,
        id: &str,
        permissions: &[String],
    ) -> Result<Option<Self::Model>, Self::Error>;
    async fn create(&self, value: Self::Model, permissions: &[String]) -> Result<(), Self::Error>;
    async fn update(
        &self,
        id: &str,
        value: Self::Model,
        permissions: &[String],
    ) -> Result<(), Self::Error>;
    async fn patch(
        &self,
        id: &str,
        changes: Value,
        permissions: &[String],
    ) -> Result<(), Self::Error>;
    async fn delete(&self, id: &str, permissions: &[String]) -> Result<(), Self::Error>;
}

/// The model handler serving `store` on its model's base path. Returns a fresh
/// `Arc`, as storage factories must.
pub fn document_handler<S: DocumentStore>(store: S) -> Arc<dyn ModelHandler> {
    Arc::new(DocumentHandler { store, require_session: false, sessions: None, checker: None })
}

struct DocumentHandler<S: DocumentStore> {
    store: S,
    require_session: bool,
    sessions: Option<Arc<dyn Any + Send + Sync>>,
    checker: Option<Arc<dyn crate::rbac::PermissionChecker>>,
}

fn error(status: StatusCode, message: &str) -> RouteResponse {
    response::json_value(status, &json!({"error": message}))
}
fn storage_error<S: DocumentStore>(value: S::Error) -> RouteResponse {
    match S::error_kind(&value) {
        DocumentErrorKind::Forbidden => error(StatusCode::FORBIDDEN, "Permission denied"),
        DocumentErrorKind::NotFound => error(StatusCode::NOT_FOUND, "Not found"),
        DocumentErrorKind::Conflict => error(StatusCode::CONFLICT, "Primary key already exists"),
        DocumentErrorKind::InvalidInput => error(StatusCode::BAD_REQUEST, &value.to_string()),
        DocumentErrorKind::Internal => {
            error(StatusCode::INTERNAL_SERVER_ERROR, "Storage operation failed")
        }
    }
}
fn native_only<S: DocumentStore>() -> String {
    format!(
        "Native admin, history, replication and backup are unavailable for {} models",
        S::BACKEND
    )
}
const ALLOW: &str = "GET, HEAD, POST, PUT, PATCH, DELETE, OPTIONS";

impl<S: DocumentStore> DocumentHandler<S> {
    async fn dispatch(&self, req: RouteRequest, segments: &[&str]) -> RouteResponse {
        let method = req.method().clone();
        if method == Method::OPTIONS {
            let mut resp = response::empty(StatusCode::NO_CONTENT);
            resp.headers_mut().insert("allow", ALLOW.parse().expect("static allow header"));
            return resp;
        }
        let session = match crate::session::session_for_request(&req, self.sessions.as_ref()).await
        {
            Ok(session) => session,
            Err(message) => return error(StatusCode::FORBIDDEN, message),
        };
        if self.require_session && session.is_none() {
            return error(StatusCode::UNAUTHORIZED, "Authentication required");
        }
        let mut permissions: Vec<String> =
            session.as_ref().and_then(|s| s.get("permissions")).unwrap_or_default();
        if let Some(checker) = &self.checker {
            let Some(role) = session.as_ref().and_then(|s| s.get::<String>("role")) else {
                return error(StatusCode::UNAUTHORIZED, "Authentication required");
            };
            // The configured checker is authoritative, not arbitrary role text
            // or a permission list stored alongside that role.
            permissions = self
                .store
                .permissions()
                .iter()
                .filter(|p| checker.has_permission(&role, p))
                .map(|p| (*p).to_owned())
                .collect();
            let operation =
                if method == Method::GET || method == Method::HEAD { "Read" } else { "Write" };
            if !checker.has_permission(&role, &format!("{}{operation}", self.model_name()))
                && !checker.has_permission(&role, operation)
                && permissions.is_empty()
            {
                return error(StatusCode::FORBIDDEN, "Permission denied");
            }
        }
        // Unlike the native optional extractor, SQL always invokes can_read /
        // can_write, even with no session, preserving public_if model policies.
        let mut query = std::collections::HashMap::new();
        let pairs = req.uri().query().unwrap_or_default().split('&').filter(|p| !p.is_empty());
        for (key, value) in pairs.map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (
                crate::http::query::percent_decode(key),
                crate::http::query::percent_decode(value),
            )
        }) {
            if query.insert(key, value).is_some() {
                return error(StatusCode::BAD_REQUEST, "Duplicate query parameter");
            }
        }
        if !query.is_empty()
            && !(segments.is_empty() && (method == Method::GET || method == Method::HEAD))
        {
            return error(StatusCode::BAD_REQUEST, "Query parameters are only supported on lists");
        }
        let id = segments.first().map(|id| crate::http::query::percent_decode(id));
        match (method, segments.len()) {
            (Method::GET | Method::HEAD, 0) => {
                if query.keys().any(|key| {
                    key != "limit"
                        && key != "offset"
                        && !self.store.filter_fields().contains(&key.as_str())
                }) {
                    return error(StatusCode::BAD_REQUEST, "Unknown query parameter");
                }
                let parse = |key: &str, default: u32| {
                    query.get(key).map_or(Ok(default), |v| v.parse::<u32>())
                };
                let (Ok(limit), Ok(offset)) = (parse("limit", 50), parse("offset", 0)) else {
                    return error(StatusCode::BAD_REQUEST, "Invalid pagination");
                };
                let mut filters =
                    query.iter().filter(|(k, _)| self.store.filter_fields().contains(&k.as_str()));
                let filter = filters.next().map(|(field, value)| (field.as_str(), value.as_str()));
                if filters.next().is_some() {
                    return error(StatusCode::BAD_REQUEST, "Only one equality filter is supported");
                }
                match self.store.list_page(limit, offset, filter, &permissions).await {
                    Ok(page) => match serde_json::to_value(page.data) {
                        Ok(data) => response::json_value(
                            StatusCode::OK,
                            &json!({
                                "data": data,
                                "has_more": page.next_offset.is_some(),
                                "next_offset": page.next_offset,
                            }),
                        ),
                        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "Serialization failed"),
                    },
                    Err(e) => storage_error::<S>(e),
                }
            }
            (Method::GET | Method::HEAD, 1) => {
                match self.store.get(id.as_deref().unwrap_or_default(), &permissions).await {
                    Ok(Some(item)) => response::json_serialize(StatusCode::OK, &item)
                        .unwrap_or_else(|_| {
                            error(StatusCode::INTERNAL_SERVER_ERROR, "Serialization failed")
                        }),
                    Ok(None) => error(StatusCode::NOT_FOUND, "Not found"),
                    Err(e) => storage_error::<S>(e),
                }
            }
            (method @ Method::POST, 0) | (method @ (Method::PUT | Method::PATCH), 1) => {
                let is_json =
                    req.headers().get("content-type").and_then(|v| v.to_str().ok()).is_some_and(
                        |v| {
                            v.split(';')
                                .next()
                                .is_some_and(|v| v.trim().eq_ignore_ascii_case("application/json"))
                        },
                    );
                if !is_json {
                    return error(
                        StatusCode::UNSUPPORTED_MEDIA_TYPE,
                        "Content-Type must be application/json",
                    );
                }
                let bytes =
                    match Limited::new(req.into_body(), S::MAX_DOCUMENT_BYTES).collect().await {
                        Ok(body) => body.to_bytes(),
                        Err(_) => {
                            return error(
                                StatusCode::PAYLOAD_TOO_LARGE,
                                "Cannot read body within 1 MiB limit",
                            )
                        }
                    };
                let result = if method == Method::PATCH {
                    let changes = match serde_json::from_slice(&bytes) {
                        Ok(value) => value,
                        Err(_) => return error(StatusCode::BAD_REQUEST, "Invalid JSON"),
                    };
                    self.store.patch(id.as_deref().unwrap_or_default(), changes, &permissions).await
                } else {
                    let value = match serde_json::from_slice(&bytes) {
                        Ok(value) => value,
                        Err(_) => return error(StatusCode::BAD_REQUEST, "Invalid model JSON"),
                    };
                    if method == Method::POST {
                        self.store.create(value, &permissions).await
                    } else {
                        self.store
                            .update(id.as_deref().unwrap_or_default(), value, &permissions)
                            .await
                    }
                };
                match result {
                    Ok(()) => response::json_value(
                        if method == Method::POST { StatusCode::CREATED } else { StatusCode::OK },
                        &json!({"saved": true}),
                    ),
                    Err(e) => storage_error::<S>(e),
                }
            }
            (Method::DELETE, 1) => {
                match self.store.delete(id.as_deref().unwrap_or_default(), &permissions).await {
                    Ok(()) => response::empty(StatusCode::NO_CONTENT),
                    Err(e) => storage_error::<S>(e),
                }
            }
            _ => {
                let mut resp = error(StatusCode::METHOD_NOT_ALLOWED, "Unsupported method or path");
                resp.headers_mut().insert("allow", ALLOW.parse().expect("static allow header"));
                resp
            }
        }
    }
}

#[async_trait::async_trait]
impl<S: DocumentStore> ModelHandler for DocumentHandler<S> {
    fn uses_native_storage(&self) -> bool {
        false
    }
    async fn handle_request(
        &self,
        req: RouteRequest,
        segments: &[&str],
    ) -> Result<RouteResponse, Infallible> {
        let head = req.method() == Method::HEAD;
        let mut resp = self.dispatch(req, segments).await;
        if head {
            *resp.body_mut() = Full::new(bytes::Bytes::new()).boxed();
        }
        Ok(resp)
    }
    fn model_name(&self) -> &str {
        std::any::type_name::<S::Model>().rsplit("::").next().unwrap_or("Unknown")
    }
    fn base_path(&self) -> &str {
        <S::Model as HttpExposable>::http_base_path()
    }
    fn set_require_session(&mut self, require: bool) {
        self.require_session = require;
    }
    fn set_session_store_any(&mut self, store: Arc<dyn Any + Send + Sync>) {
        self.sessions = Some(store);
    }
    fn set_permission_checker(&mut self, checker: Arc<dyn crate::rbac::PermissionChecker>) {
        if self.checker.is_none() {
            self.checker = Some(checker);
        }
    }
    // These interfaces describe native snapshots/events, not SQL data. Server
    // startup rejects native admin/cluster configuration for this handler.
    async fn get_all_data_json(&self) -> Value {
        json!({"error": native_only::<S>()})
    }
    async fn get_sample_data_json(&self, _limit: usize) -> Value {
        json!({"error": native_only::<S>()})
    }
    async fn get_item_json(&self, _id: &str) -> Option<Value> {
        None
    }
    async fn get_count(&self) -> usize {
        0
    }
    async fn export_json(&self) -> Value {
        json!({"error": native_only::<S>()})
    }
    async fn get_entity_history(&self, _id: &str) -> Value {
        json!({"error": native_only::<S>()})
    }
    async fn get_entity_event_count(&self, _id: &str) -> usize {
        0
    }
    async fn submit_edit_event(&self, _id: &str, _changes: Value) -> Result<Value, String> {
        Err(native_only::<S>())
    }
    async fn apply_replicated_item_json(&self, _item: Value) -> Result<(), String> {
        Err(native_only::<S>())
    }
    async fn apply_replicated_items_json(&self, _items: Vec<Value>) -> Result<usize, String> {
        Err(native_only::<S>())
    }
    async fn apply_replicated_update_json(&self, _id: &str, _item: Value) -> Result<(), String> {
        Err(native_only::<S>())
    }
    async fn apply_replicated_delete_json(&self, _id: &str) -> Result<bool, String> {
        Err(native_only::<S>())
    }
    // Native metrics measure what is held in RAM; never scan the database.
    async fn get_stats(&self, _data_path: &str) -> ModelStats {
        // Only the L1 copies (RFC 304) live in RAM; the authority is external.
        let cache = self.store.cache_stats().unwrap_or_default();
        ModelStats {
            model: self.model_name().into(),
            item_count: cache.items,
            approx_ram_bytes: cache.bytes as u64,
            raftlog_size_bytes: 0,
            events_since_last_compaction: None,
            last_compaction_at: None,
        }
    }
}
