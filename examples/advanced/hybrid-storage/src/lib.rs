//! A public native model and an authenticated SQL archive in one Lithair server.
use http_body_util::{BodyExt, Limited};
use lithair_core::app::{
    response, LithairServer, LithairServerBuilder, Method, RouteRequest, RouteResponse, StatusCode,
};
use lithair_core::DeclarativeModel;
use lithair_turso::{Database, Equal, Error, Page, SqlModel, Store, MAX_DOCUMENT_BYTES};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::Path, sync::Arc};

#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
pub struct LiveTask {
    #[http(expose)]
    pub id: String,
    #[http(expose, validate = "non_empty")]
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
pub struct Archive {
    #[http(expose)]
    #[permission(read = "ArchiveRead", write = "ArchiveWrite")]
    pub id: String,
    #[http(expose, validate = "non_empty")]
    pub title: String,
    #[http(expose)]
    pub category: String,
}
impl SqlModel for Archive {
    const COLLECTION: &'static str = "archives_v1";
    const FILTER_FIELDS: &'static [&'static str] = &["category"];
}

/// The example's archive routes use an explicit bearer credential. Production
/// applications should derive permission strings from their own authenticated user.
pub async fn application(data: &Path, token: String) -> anyhow::Result<LithairServerBuilder> {
    anyhow::ensure!(!token.is_empty(), "ARCHIVE_TOKEN must not be empty");
    std::fs::create_dir_all(data)?;
    let path = data.join("archive.db");
    let database =
        Database::open(path.to_str().ok_or_else(|| anyhow::anyhow!("non-UTF8 database path"))?)
            .await?;
    let archive = database.store::<Archive>("example")?;
    let token = Arc::new(format!("Bearer {token}"));
    let mut builder = LithairServer::new()
        .with_data_dir(data.to_string_lossy().to_string())
        .with_model::<LiveTask>(data.join("tasks").to_string_lossy(), "/api/tasks");
    for method in [Method::GET, Method::POST, Method::PUT, Method::DELETE] {
        let archive = archive.clone();
        let token = token.clone();
        builder = builder.with_route_async(method, "/api/archives", move |req| {
            let archive = archive.clone();
            let token = token.clone();
            async move { Ok(handle(req, archive, &token).await) }
        });
    }
    Ok(builder)
}

fn error(status: StatusCode, message: &str) -> RouteResponse {
    response::json_value(status, &serde_json::json!({"error": message}))
}
fn storage_error(error_value: Error) -> RouteResponse {
    match error_value {
        Error::Forbidden => error(StatusCode::FORBIDDEN, "Permission denied"),
        Error::NotFound => error(StatusCode::NOT_FOUND, "Not found"),
        Error::Conflict => error(StatusCode::CONFLICT, "Primary key already exists"),
        Error::InvalidInput(_) | Error::Validation(_) => {
            error(StatusCode::BAD_REQUEST, &error_value.to_string())
        }
        _ => error(StatusCode::INTERNAL_SERVER_ERROR, "Storage operation failed"),
    }
}

async fn handle(req: RouteRequest, archive: Store<Archive>, token: &str) -> RouteResponse {
    if req.headers().get("authorization").and_then(|v| v.to_str().ok()) != Some(token) {
        return error(StatusCode::UNAUTHORIZED, "Valid archive bearer token required");
    }
    let permissions = vec!["ArchiveRead".into(), "ArchiveWrite".into()];
    let query: HashMap<String, String> =
        url::form_urlencoded::parse(req.uri().query().unwrap_or_default().as_bytes())
            .into_owned()
            .collect();
    if query
        .keys()
        .any(|key| !["id", "category", "limit", "offset"].contains(&key.as_str()))
    {
        return error(StatusCode::BAD_REQUEST, "Unknown query parameter");
    }
    let id = query.get("id");
    match *req.method() {
        Method::GET => {
            if let Some(id) = id {
                return match archive.get(id, &permissions).await {
                    Ok(Some(item)) => {
                        response::json_value(StatusCode::OK, &serde_json::json!({"data": item}))
                    }
                    Ok(None) => error(StatusCode::NOT_FOUND, "Not found"),
                    Err(err) => storage_error(err),
                };
            }
            let parse =
                |key: &str, default: u32| query.get(key).map_or(Ok(default), |v| v.parse::<u32>());
            let (Ok(limit), Ok(offset)) = (parse("limit", 50), parse("offset", 0)) else {
                return error(StatusCode::BAD_REQUEST, "Invalid pagination");
            };
            let filter = query.get("category").map(|value| Equal { field: "category", value });
            match archive.list(Page { limit, offset }, filter, &permissions).await {
                Ok(items) => {
                    response::json_value(StatusCode::OK, &serde_json::json!({"data": items}))
                }
                Err(err) => storage_error(err),
            }
        }
        Method::POST | Method::PUT => {
            let create = *req.method() == Method::POST;
            if !create && id.is_none() {
                return error(StatusCode::BAD_REQUEST, "Update requires id");
            }
            let body = match Limited::new(req.into_body(), MAX_DOCUMENT_BYTES).collect().await {
                Ok(body) => body.to_bytes(),
                Err(_) => {
                    return error(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "Cannot read body within 1 MiB limit",
                    )
                }
            };
            let value: Archive = match serde_json::from_slice(&body) {
                Ok(value) => value,
                Err(_) => return error(StatusCode::BAD_REQUEST, "Invalid archive JSON"),
            };
            let result = if create {
                archive.create(value, &permissions).await
            } else {
                archive
                    .update(id.map(String::as_str).unwrap_or_default(), value, &permissions)
                    .await
            };
            match result {
                Ok(()) => response::json_value(
                    if create { StatusCode::CREATED } else { StatusCode::OK },
                    &serde_json::json!({"saved": true}),
                ),
                Err(err) => storage_error(err),
            }
        }
        Method::DELETE => {
            let Some(id) = id else {
                return error(StatusCode::BAD_REQUEST, "Delete requires id");
            };
            match archive.delete(id, &permissions).await {
                Ok(()) => {
                    response::json_value(StatusCode::OK, &serde_json::json!({"deleted": true}))
                }
                Err(err) => storage_error(err),
            }
        }
        _ => error(StatusCode::METHOD_NOT_ALLOWED, "Unsupported method"),
    }
}
