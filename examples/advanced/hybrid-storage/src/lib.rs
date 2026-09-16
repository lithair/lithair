//! Native tasks and a declarative SQL archive in one Lithair server.
use lithair_core::app::{LithairServer, LithairServerBuilder};
use lithair_core::http::RouteGuard;
use lithair_core::session::{MemorySessionStore, Session, SessionManager, SessionStore};
use lithair_core::DeclarativeModel;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
pub struct LiveTask {
    #[http(expose)]
    pub id: String,
    #[http(expose, validate = "non_empty")]
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, DeclarativeModel)]
#[storage(turso, collection = "archives_v1", filters("category"))]
pub struct Archive {
    #[http(expose)]
    #[permission(read = "ArchiveRead", write = "ArchiveWrite")]
    pub id: String,
    #[http(expose, validate = "non_empty")]
    pub title: String,
    #[http(expose)]
    pub category: String,
}

/// Seed a demo session. Real applications can use with_rbac_config/login.
/// Backend selection and all CRUD routes come from the model declarations.
pub async fn application(data: &Path, token: String) -> anyhow::Result<LithairServerBuilder> {
    anyhow::ensure!(!token.is_empty(), "ARCHIVE_TOKEN must not be empty");
    let sessions = MemorySessionStore::new();
    let mut session = Session::new(token, chrono::Utc::now() + chrono::Duration::hours(1));
    session.set("permissions", vec!["ArchiveRead", "ArchiveWrite"])?;
    sessions.set(session).await?;
    Ok(LithairServer::new()
        .with_data_dir(data.to_string_lossy().to_string())
        .with_sessions(SessionManager::new(sessions))
        .with_route_guard(
            "/api/archives/*",
            RouteGuard::RequireAuth { redirect_to: None, exclude: vec![] },
        )
        .with_model::<LiveTask>(data.join("tasks").to_string_lossy(), "/api/tasks")
        .with_model::<Archive>(data.join("archives").to_string_lossy(), "/api/archives"))
}
