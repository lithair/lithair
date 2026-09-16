//! Generated model routes backed by SQL, with Lithair's shared session transport.
use crate::{Database, Equal, Error, Page, SqlModel, Store, MAX_DOCUMENT_BYTES};
use http_body_util::{BodyExt, Full, Limited};
use lithair_core::app::{
    response, Method, ModelFactory, ModelHandler, ModelStats, RouteRequest, RouteResponse,
    StatusCode,
};
use serde_json::{json, Value};
use std::{any::Any, convert::Infallible, sync::Arc};

/// Factory used by `DeclarativeModel`. `data_path` is a model directory, just
/// like native storage; it is created on startup and contains `model.db`.
/// Give each registration its own directory. Namespace/collection are stable
/// document keys, independent of the HTTP mount path.
pub fn model_factory<T: SqlModel>() -> ModelFactory {
    Arc::new(|data_path| {
        Box::pin(async move {
            tokio::fs::create_dir_all(&data_path).await?;
            for native_file in ["events.raftlog", "state.raftsnap", "meta.raftmeta"] {
                anyhow::ensure!(
                    !tokio::fs::try_exists(std::path::Path::new(&data_path).join(native_file)).await?,
                    "Native data exists in this model directory; migrate explicitly before selecting Turso"
                );
            }
            let path = std::path::Path::new(&data_path).join("model.db");
            let database = Database::open(
                path.to_str().ok_or_else(|| anyhow::anyhow!("database path must be UTF-8"))?,
            )
            .await?;
            let store = database.store::<T>(T::NAMESPACE)?;
            store.prepare().await?;
            Ok(
                Arc::new(SqlHandler {
                    store,
                    require_session: false,
                    sessions: None,
                    checker: None,
                }) as Arc<dyn ModelHandler>,
            )
        })
    })
}

struct SqlHandler<T: SqlModel> {
    store: Store<T>,
    require_session: bool,
    sessions: Option<Arc<dyn Any + Send + Sync>>,
    checker: Option<Arc<dyn lithair_core::rbac::PermissionChecker>>,
}
fn error(status: StatusCode, message: &str) -> RouteResponse {
    response::json_value(status, &json!({"error": message}))
}
fn storage_error(value: Error) -> RouteResponse {
    match value {
        Error::Forbidden => error(StatusCode::FORBIDDEN, "Permission denied"),
        Error::NotFound => error(StatusCode::NOT_FOUND, "Not found"),
        Error::Conflict => error(StatusCode::CONFLICT, "Primary key already exists"),
        Error::InvalidInput(_) | Error::Validation(_) => {
            error(StatusCode::BAD_REQUEST, &value.to_string())
        }
        _ => error(StatusCode::INTERNAL_SERVER_ERROR, "Storage operation failed"),
    }
}
const ALLOW: &str = "GET, HEAD, POST, PUT, PATCH, DELETE, OPTIONS";
const NATIVE_ONLY: &str =
    "Native admin, history, replication and backup are unavailable for Turso models";

impl<T: SqlModel> SqlHandler<T> {
    async fn dispatch(&self, req: RouteRequest, segments: &[&str]) -> RouteResponse {
        let method = req.method().clone();
        if method == Method::OPTIONS {
            let mut resp = response::empty(StatusCode::NO_CONTENT);
            resp.headers_mut().insert("allow", ALLOW.parse().expect("static allow header"));
            return resp;
        }
        let session =
            match lithair_core::session::session_for_request(&req, self.sessions.as_ref()).await {
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
            permissions = T::PERMISSIONS
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
        for (key, value) in
            url::form_urlencoded::parse(req.uri().query().unwrap_or_default().as_bytes())
                .into_owned()
        {
            if query.insert(key, value).is_some() {
                return error(StatusCode::BAD_REQUEST, "Duplicate query parameter");
            }
        }
        if !query.is_empty()
            && !(segments.is_empty() && (method == Method::GET || method == Method::HEAD))
        {
            return error(StatusCode::BAD_REQUEST, "Query parameters are only supported on lists");
        }
        let id = segments.first().map(|id| lithair_core::http::query::percent_decode(id));
        match (method, segments.len()) {
            (Method::GET | Method::HEAD, 0) => {
                if query.keys().any(|key| {
                    key != "limit" && key != "offset" && !T::FILTER_FIELDS.contains(&key.as_str())
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
                    query.iter().filter(|(k, _)| T::FILTER_FIELDS.contains(&k.as_str()));
                let filter = filters.next().map(|(field, value)| Equal { field, value });
                if filters.next().is_some() {
                    return error(StatusCode::BAD_REQUEST, "Only one equality filter is supported");
                }
                match self.store.list_page(Page { limit, offset }, filter, &permissions).await {
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
                    Err(e) => storage_error(e),
                }
            }
            (Method::GET | Method::HEAD, 1) => {
                match self.store.get(id.as_deref().unwrap_or_default(), &permissions).await {
                    Ok(Some(item)) => response::json_serialize(StatusCode::OK, &item)
                        .unwrap_or_else(|_| {
                            error(StatusCode::INTERNAL_SERVER_ERROR, "Serialization failed")
                        }),
                    Ok(None) => error(StatusCode::NOT_FOUND, "Not found"),
                    Err(e) => storage_error(e),
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
                let bytes = match Limited::new(req.into_body(), MAX_DOCUMENT_BYTES).collect().await
                {
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
                    Err(e) => storage_error(e),
                }
            }
            (Method::DELETE, 1) => {
                match self.store.delete(id.as_deref().unwrap_or_default(), &permissions).await {
                    Ok(()) => response::json_value(StatusCode::OK, &json!({"deleted": true})),
                    Err(e) => storage_error(e),
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
impl<T: SqlModel> ModelHandler for SqlHandler<T> {
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
        std::any::type_name::<T>().rsplit("::").next().unwrap_or("Unknown")
    }
    fn base_path(&self) -> &str {
        T::http_base_path()
    }
    fn set_require_session(&mut self, require: bool) {
        self.require_session = require;
    }
    fn set_session_store_any(&mut self, store: Arc<dyn Any + Send + Sync>) {
        self.sessions = Some(store);
    }
    fn set_permission_checker(&mut self, checker: Arc<dyn lithair_core::rbac::PermissionChecker>) {
        if self.checker.is_none() {
            self.checker = Some(checker);
        }
    }
    // These interfaces describe native snapshots/events, not SQL data. Server
    // startup rejects native admin/cluster configuration for this handler.
    async fn get_all_data_json(&self) -> Value {
        json!({"error": NATIVE_ONLY})
    }
    async fn get_sample_data_json(&self, _limit: usize) -> Value {
        json!({"error": NATIVE_ONLY})
    }
    async fn get_item_json(&self, _id: &str) -> Option<Value> {
        None
    }
    async fn get_count(&self) -> usize {
        0
    }
    async fn export_json(&self) -> Value {
        json!({"error": NATIVE_ONLY})
    }
    async fn get_entity_history(&self, _id: &str) -> Value {
        json!({"error": NATIVE_ONLY})
    }
    async fn get_entity_event_count(&self, _id: &str) -> usize {
        0
    }
    async fn submit_edit_event(&self, _id: &str, _changes: Value) -> Result<Value, String> {
        Err(NATIVE_ONLY.into())
    }
    async fn apply_replicated_item_json(&self, _item: Value) -> Result<(), String> {
        Err(NATIVE_ONLY.into())
    }
    async fn apply_replicated_items_json(&self, _items: Vec<Value>) -> Result<usize, String> {
        Err(NATIVE_ONLY.into())
    }
    async fn apply_replicated_update_json(&self, _id: &str, _item: Value) -> Result<(), String> {
        Err(NATIVE_ONLY.into())
    }
    async fn apply_replicated_delete_json(&self, _id: &str) -> Result<bool, String> {
        Err(NATIVE_ONLY.into())
    }
    // Existing metrics explicitly measure items held in RAM and native logs.
    // SQL models hold neither; never scan the database for native metrics.
    async fn get_stats(&self, _data_path: &str) -> ModelStats {
        ModelStats {
            model: self.model_name().into(),
            item_count: 0,
            approx_ram_bytes: 0,
            raftlog_size_bytes: 0,
            events_since_last_compaction: None,
            last_compaction_at: None,
        }
    }
}
