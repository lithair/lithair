use lithair_core::engine::persistence::FileStorage;
use lithair_core::engine::{Event, StateEngine};
use lithair_core::http::{HttpResponse, Router, StatusCode};
/// Lithair test application for E2E Cucumber tests
///
/// This application implements a complete HTTP server with:
/// - StateEngine for in-memory state
/// - FileStorage for persistence
/// - HTTP routes for CRUD articles
/// - Health check endpoint
use std::sync::Arc;
use tokio::sync::Mutex;

// Reuse types from world.rs
use crate::features::world::{TestAppState, TestEvent};

/// Minimal test application for E2E
pub struct TestApp {
    pub engine: Arc<StateEngine<TestAppState>>,
    pub storage: Arc<Mutex<Option<FileStorage>>>,
}

impl TestApp {
    /// Creates a new test application
    pub fn new(
        engine: Arc<StateEngine<TestAppState>>,
        storage: Arc<Mutex<Option<FileStorage>>>,
    ) -> Self {
        Self { engine, storage }
    }

    /// Builds the router with all routes
    pub fn router(&self) -> Router<()> {
        // Clone references for closures
        let engine_health = self.engine.clone();
        let engine_list = self.engine.clone();
        let engine_get = self.engine.clone();
        let engine_create = self.engine.clone();
        let storage_create = self.storage.clone();

        Router::new()
            // Health check
            .get("/health", move |_req, _params, _state| {
                let _ = engine_health.clone();
                let json_body = serde_json::to_string(&serde_json::json!({
                    "status": "ok",
                    "service": "lithair-test"
                })).unwrap();

                HttpResponse::new(StatusCode::Ok).json(&json_body)
            })
            // GET /api/articles - List all articles
            .get("/api/articles", move |_req, _params, _state| {
                let engine = engine_list.clone();
                let articles = engine.with_state(|state| {
                    state.data.articles.clone()
                }).unwrap_or_default();

                let json_body = serde_json::to_string(&serde_json::json!({
                    "articles": articles,
                    "count": articles.len()
                })).unwrap();

                HttpResponse::new(StatusCode::Ok).json(&json_body)
            })
            // GET /api/articles/count - Count articles
            .get("/api/articles/count", move |_req, _params, _state| {
                let engine = engine_get.clone();
                let count = engine.with_state(|state| {
                    state.data.articles.len()
                }).unwrap_or(0);

                let json_body = serde_json::to_string(&serde_json::json!({
                    "count": count
                })).unwrap();

                HttpResponse::new(StatusCode::Ok).json(&json_body)
            })
            // POST /api/articles - Create an article
            .post("/api/articles", move |req, _params, _state| {
                let engine = engine_create.clone();
                let storage = storage_create.clone();

                // Parse the JSON body
                let body_str = std::str::from_utf8(req.body()).unwrap_or("{}");
                let article: serde_json::Value = serde_json::from_str(body_str)
                    .unwrap_or(serde_json::json!({}));

                // Generate an ID
                let id = uuid::Uuid::new_v4().to_string();

                // Create the event
                let event = TestEvent::ArticleCreated {
                    id: id.clone(),
                    title: article.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    content: article.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                };

                // Apply to the engine
                engine.with_state_mut(|state| {
                    event.apply(state);
                }).ok();

                // FileStorage persistence is not yet wired here because FileStorage
                // is async and this handler is sync; for now just log the event
                let event_json = serde_json::to_string(&event).unwrap_or_default();
                println!("💾 Event to persist: {}", event_json);
                let _ = storage;

                let json_body = serde_json::to_string(&serde_json::json!({
                    "id": id,
                    "article": article,
                    "status": "created"
                })).unwrap();

                HttpResponse::new(StatusCode::Created).json(&json_body)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_creation() {
        let engine = Arc::new(StateEngine::new(TestAppState::default()));
        let storage = Arc::new(Mutex::new(None));
        let app = TestApp::new(engine, storage);
        let router = app.router();

        // Verify that routes exist
        assert!(!router.routes().is_empty());
    }
}
