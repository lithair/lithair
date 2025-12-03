use lithair_core::engine::persistence::FileStorage;
use lithair_core::engine::{Event, StateEngine};
use lithair_core::http::{HttpResponse, Router, StatusCode};
/// Application de test Lithair pour tests E2E Cucumber
///
/// Cette application implémente un serveur HTTP complet avec :
/// - StateEngine pour l'état en mémoire
/// - FileStorage pour la persistance
/// - Routes HTTP pour CRUD articles
/// - Health check endpoint
use std::sync::Arc;
use tokio::sync::Mutex;

// Réutiliser les types de world.rs
use crate::features::world::{TestAppState, TestEvent};

/// Application de test minimale pour E2E
pub struct TestApp {
    pub engine: Arc<StateEngine<TestAppState>>,
    pub storage: Arc<Mutex<Option<FileStorage>>>,
}

impl TestApp {
    /// Crée une nouvelle application de test
    pub fn new(
        engine: Arc<StateEngine<TestAppState>>,
        storage: Arc<Mutex<Option<FileStorage>>>,
    ) -> Self {
        Self { engine, storage }
    }

    /// Construit le router avec toutes les routes
    pub fn router(&self) -> Router<()> {
        // Clone les références pour les closures
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
            // GET /api/articles - Lister tous les articles
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
            // GET /api/articles/count - Compter les articles
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
            // POST /api/articles - Créer un article
            .post("/api/articles", move |req, _params, _state| {
                let engine = engine_create.clone();
                let storage = storage_create.clone();

                // Parser le body JSON
                let body_str = std::str::from_utf8(req.body()).unwrap_or("{}");
                let article: serde_json::Value = serde_json::from_str(body_str)
                    .unwrap_or(serde_json::json!({}));

                // Générer un ID
                let id = uuid::Uuid::new_v4().to_string();

                // Créer l'événement
                let event = TestEvent::ArticleCreated {
                    id: id.clone(),
                    title: article.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    content: article.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                };

                // Appliquer au moteur
                engine.with_state_mut(|state| {
                    event.apply(state);
                }).ok();

                // TODO: Persister l'événement dans FileStorage
                // Pour l'instant juste log car FileStorage est async et handler est sync
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

        // Vérifier qu'on a des routes
        assert!(router.routes().len() > 0);
    }
}
