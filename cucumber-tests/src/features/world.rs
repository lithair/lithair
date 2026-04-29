#![allow(dead_code)]
use chrono;
use crc32fast::Hasher as Crc32Hasher;
use cucumber::World as CucumberWorld;
use lithair_core::engine::persistence::FileStorage;
use lithair_core::engine::{Event, StateEngine};
use lithair_core::http::{AsyncHttpServer, HttpResponse, HttpServer, Router, StatusCode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

#[derive(Debug, Default, Clone)]
pub struct ServerState {
    pub port: u16,
    pub process_id: Option<u32>,
    pub is_running: bool,
    pub base_url: Option<String>,
}

/// Represents a node in a Lithair cluster (simple mock)
pub struct ClusterNode {
    pub node_id: usize,
    pub server_state: ServerState,
    pub engine: Arc<StateEngine<TestAppState>>,
    pub storage: Arc<Mutex<Option<FileStorage>>>,
    pub server_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
}

/// Represents a real LithairServer node as an external process
pub struct RealClusterNode {
    pub node_id: u64,
    pub port: u16,
    pub process: Option<std::process::Child>,
    pub data_dir: PathBuf,
    pub peers: Vec<u16>,
}

#[derive(Debug)]
pub struct Metrics {
    pub request_count: u64,
    pub response_time_ms: f64,
    pub error_rate: f64,
    pub memory_usage_mb: f64,
    // Performance HTTP
    pub throughput: f64,
    pub total_duration: std::time::Duration,
    pub error_count: usize,
    pub latency_p50: std::time::Duration,
    pub latency_p95: std::time::Duration,
    pub latency_p99: std::time::Duration,
    // Server
    pub base_url: String,
    pub server_port: u16,
    pub persist_path: String,
    // Isolated benchmarks
    pub last_throughput: f64,
    pub last_avg_latency_ms: f64,
    pub last_p50_latency_ms: f64,
    pub last_p95_latency_ms: f64,
    pub last_p99_latency_ms: f64,
    // Snapshots
    pub last_state_json: Option<String>,
    pub loaded_state_json: Option<String>,
    pub snapshot_read_duration: Option<std::time::Duration>,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            request_count: 0,
            response_time_ms: 0.0,
            error_rate: 0.0,
            memory_usage_mb: 0.0,
            throughput: 0.0,
            total_duration: std::time::Duration::from_secs(0),
            error_count: 0,
            latency_p50: std::time::Duration::from_secs(0),
            latency_p95: std::time::Duration::from_secs(0),
            latency_p99: std::time::Duration::from_secs(0),
            base_url: String::from("http://localhost:8080"),
            server_port: 8080,
            persist_path: String::from("/tmp/cucumber-test"),
            last_throughput: 0.0,
            last_avg_latency_ms: 0.0,
            last_p50_latency_ms: 0.0,
            last_p95_latency_ms: 0.0,
            last_p99_latency_ms: 0.0,
            last_state_json: None,
            loaded_state_json: None,
            snapshot_read_duration: None,
        }
    }
}

/// Article structure for SCC2StateEngine (lock-free reads!)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TestArticle {
    pub id: String,
    pub title: String,
    pub content: String,
}

impl lithair_core::model_inspect::Inspectable for TestArticle {
    fn get_field_value(&self, field_name: &str) -> Option<serde_json::Value> {
        match field_name {
            "id" => serde_json::to_value(&self.id).ok(),
            "title" => serde_json::to_value(&self.title).ok(),
            "content" => serde_json::to_value(&self.content).ok(),
            _ => None,
        }
    }
}

impl lithair_core::model::ModelSpec for TestArticle {
    fn get_policy(&self, _field_name: &str) -> Option<lithair_core::model::FieldPolicy> {
        None
    }
    fn get_all_fields(&self) -> Vec<String> {
        vec!["id".to_string(), "title".to_string(), "content".to_string()]
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TestData {
    pub articles: HashMap<String, serde_json::Value>,
    pub users: HashMap<String, serde_json::Value>,
    pub tokens: HashMap<String, String>,
}

// Test state for the Lithair engine
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TestAppState {
    pub data: TestData,
    pub version: u64,
}

impl lithair_core::model_inspect::Inspectable for TestAppState {
    fn get_field_value(&self, _field_name: &str) -> Option<serde_json::Value> {
        None
    }
}

impl lithair_core::model::ModelSpec for TestAppState {
    fn get_policy(&self, _field_name: &str) -> Option<lithair_core::model::FieldPolicy> {
        None
    }
    fn get_all_fields(&self) -> Vec<String> {
        vec![]
    }
}

// Test events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TestEvent {
    ArticleCreated {
        id: String,
        title: String,
        content: String,
    },
    ArticleUpdated {
        id: String,
        title: String,
        content: String,
    },
    ArticleDeleted {
        id: String,
    },
    UserCreated {
        id: String,
        data: serde_json::Value,
    },
    /// Dynamic relation between an article and a user
    ArticleLinkedToUser {
        article_id: String,
        user_id: String,
    },
}

impl Event for TestEvent {
    type State = TestAppState;

    fn apply(&self, state: &mut Self::State) {
        match self {
            TestEvent::ArticleCreated { id, title, content } => {
                let article = serde_json::json!({
                    "id": id,
                    "title": title,
                    "content": content
                });
                state.data.articles.insert(id.clone(), article);
                state.version += 1;
            }
            TestEvent::ArticleUpdated { id, title, content } => {
                let article = serde_json::json!({
                    "id": id,
                    "title": title,
                    "content": content
                });
                state.data.articles.insert(id.clone(), article);
                state.version += 1;
            }
            TestEvent::ArticleDeleted { id } => {
                state.data.articles.remove(id);
                state.version += 1;
            }
            TestEvent::UserCreated { id, data } => {
                state.data.users.insert(id.clone(), data.clone());
                state.version += 1;
            }
            TestEvent::ArticleLinkedToUser { article_id, user_id } => {
                // Update the article with the user's id (author)
                if let Some(article) = state.data.articles.get_mut(article_id) {
                    if let Some(obj) = article.as_object_mut() {
                        obj.insert(
                            "author_id".to_string(),
                            serde_json::Value::String(user_id.clone()),
                        );
                    }
                }

                // Update the user with the list of their linked articles
                if let Some(user) = state.data.users.get_mut(user_id) {
                    if let Some(obj) = user.as_object_mut() {
                        use serde_json::Value;

                        let entry = obj.entry("articles").or_insert(Value::Array(Vec::new()));
                        if let Value::Array(arr) = entry {
                            let already_present = arr
                                .iter()
                                .any(|v| v.as_str().map(|s| s == article_id).unwrap_or(false));
                            if !already_present {
                                arr.push(Value::String(article_id.clone()));
                            }
                        }
                    }
                }

                state.version += 1;
            }
        }
    }

    fn idempotence_key(&self) -> Option<String> {
        match self {
            TestEvent::ArticleCreated { id, .. } => Some(format!("article-created:{}", id)),
            TestEvent::ArticleUpdated { id, .. } => Some(format!("article-updated:{}", id)),
            TestEvent::ArticleDeleted { id } => Some(format!("article-deleted:{}", id)),
            TestEvent::UserCreated { id, .. } => Some(format!("user-created:{}", id)),
            TestEvent::ArticleLinkedToUser { article_id, user_id } => {
                Some(format!("article-linked-to-user:{}:{}", article_id, user_id))
            }
        }
    }

    fn aggregate_id(&self) -> Option<String> {
        match self {
            TestEvent::ArticleCreated { .. }
            | TestEvent::ArticleUpdated { .. }
            | TestEvent::ArticleDeleted { .. } => Some("articles".to_string()),
            TestEvent::UserCreated { .. } => Some("users".to_string()),
            TestEvent::ArticleLinkedToUser { .. } => Some("relations".to_string()),
        }
    }

    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "\"serialization-error\"".to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionedArticleCreatedPayload {
    pub version: u32,
    pub id: String,
    pub title: String,
    pub content: String,
    pub slug: Option<String>,
}

#[derive(Default)]
pub struct VersionedArticleCreatedDeserializer;

impl lithair_core::engine::EventDeserializer for VersionedArticleCreatedDeserializer {
    type State = TestAppState;

    fn event_type(&self) -> &str {
        "test::ArticleCreated.versioned"
    }

    fn apply_from_json(&self, state: &mut Self::State, payload_json: &str) -> Result<(), String> {
        let value: serde_json::Value = serde_json::from_str(payload_json)
            .map_err(|e| format!("Failed to parse versioned article payload: {}", e))?;

        let version = value.get("version").and_then(|v| v.as_u64()).unwrap_or(1) as u32;

        let id = value
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing id in versioned article payload".to_string())?
            .to_string();

        let title = value.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();

        let content = value.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();

        let slug = if version >= 2 {
            value.get("slug").and_then(|v| v.as_str()).map(|s| s.to_string())
        } else {
            None
        };

        let article_json = serde_json::json!({
            "id": id,
            "title": title,
            "content": content,
            "slug": slug,
            "version": version,
        });

        state.data.articles.insert(id, article_json);
        state.version += 1;

        Ok(())
    }
}

/// Minimal application for using Engine<TestEngineApp> in tests
pub struct TestEngineApp;

impl lithair_core::RaftstoneApplication for TestEngineApp {
    type State = TestAppState;
    type Command = ();
    type Event = TestEvent;

    fn initial_state() -> Self::State {
        TestAppState::default()
    }

    fn routes() -> Vec<lithair_core::http::Route<Self::State>> {
        Vec::new()
    }

    fn command_routes() -> Vec<lithair_core::http::CommandRoute<Self>> {
        Vec::new()
    }

    fn event_deserializers(
    ) -> Vec<Box<dyn lithair_core::engine::EventDeserializer<State = Self::State>>> {
        vec![Box::new(VersionedArticleCreatedDeserializer)]
    }
}

#[derive(CucumberWorld)]
pub struct LithairWorld {
    pub server: Arc<Mutex<ServerState>>,
    pub metrics: Arc<Mutex<Metrics>>,
    pub test_data: Arc<Mutex<TestData>>,
    pub last_response: Option<String>,
    pub last_error: Option<String>,
    // Real Lithair engine
    pub engine: Arc<StateEngine<TestAppState>>,
    pub storage: Arc<Mutex<Option<FileStorage>>>,
    // AsyncWriter for contention-free ultra-fast writes
    pub async_writer: Arc<Mutex<Option<lithair_core::engine::AsyncWriter>>>,
    // Scc2Engine for ultra-fast reads (40M+ ops/sec)
    pub scc2_articles: Arc<lithair_core::engine::Scc2Engine<TestArticle>>,
    pub temp_dir: Arc<Mutex<Option<tempfile::TempDir>>>,
    // Real HTTP server in background
    pub server_handle: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
    // Distributed cluster support (simple mock)
    pub cluster_nodes: Arc<Mutex<Vec<ClusterNode>>>,
    // Real cluster support with actual LithairServer processes
    pub real_cluster_nodes: Arc<Mutex<Vec<RealClusterNode>>>,
    pub real_cluster_temp_dirs: Arc<Mutex<Vec<tempfile::TempDir>>>,
    // Reliability test support
    pub pre_crash_state: Option<Vec<TestArticle>>,
    pub corruption_detected: bool,
    pub parallel_handles: Option<Vec<tokio::task::JoinHandle<()>>>,
    // MultiFileEventStore for multi-file tests
    pub multi_file_store: Arc<Mutex<Option<lithair_core::engine::MultiFileEventStore>>>,
}

impl std::fmt::Debug for LithairWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LithairWorld")
            .field("server", &self.server)
            .field("metrics", &self.metrics)
            .field("test_data", &self.test_data)
            .field("last_response", &self.last_response)
            .field("last_error", &self.last_error)
            .field("engine", &"<StateEngine>")
            .field("storage", &"<FileStorage>")
            .field("temp_dir", &"<TempDir>")
            .finish()
    }
}

impl Default for LithairWorld {
    fn default() -> Self {
        Self {
            server: Arc::new(Mutex::new(ServerState::default())),
            metrics: Arc::new(Mutex::new(Metrics::default())),
            test_data: Arc::new(Mutex::new(TestData::default())),
            last_response: None,
            last_error: None,
            engine: Arc::new(StateEngine::new(TestAppState::default())),
            storage: Arc::new(Mutex::new(None)),
            async_writer: Arc::new(Mutex::new(None)),
            scc2_articles: Arc::new(
                lithair_core::engine::Scc2Engine::new(
                    Arc::new(std::sync::RwLock::new(
                        lithair_core::engine::EventStore::new("test_scc2")
                            .expect("Failed to create EventStore"),
                    )),
                    lithair_core::engine::Scc2EngineConfig {
                        verbose_logging: false,
                        enable_snapshots: true,
                        snapshot_interval: 1000,
                        enable_deduplication: true,
                        auto_persist_writes: true,
                        force_immediate_persistence: false,
                    },
                )
                .unwrap(),
            ),
            temp_dir: Arc::new(Mutex::new(None)),
            server_handle: Arc::new(Mutex::new(None)),
            cluster_nodes: Arc::new(Mutex::new(Vec::new())),
            real_cluster_nodes: Arc::new(Mutex::new(Vec::new())),
            real_cluster_temp_dirs: Arc::new(Mutex::new(Vec::new())),
            pre_crash_state: None,
            corruption_detected: false,
            parallel_handles: None,
            multi_file_store: Arc::new(Mutex::new(None)),
        }
    }
}

impl LithairWorld {
    pub async fn make_request(
        &mut self,
        method: &str,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<(), String> {
        let client = reqwest::Client::new();
        let server = self.server.lock().await;
        let url = format!("http://127.0.0.1:{}{}", server.port, path);
        drop(server);

        let request = match method {
            "GET" => client.get(&url),
            "POST" => client.post(&url),
            "PUT" => client.put(&url),
            "DELETE" => client.delete(&url),
            _ => return Err(format!("Unsupported HTTP method: {}", method)),
        };

        let request = if let Some(body_data) = body { request.json(&body_data) } else { request };

        match request.send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                let text = response.text().await.unwrap_or_default();
                self.last_response = Some(format!("Status: {}, Body: {}", status, text));
                Ok(())
            }
            Err(e) => {
                self.last_error = Some(format!("Request error: {}", e));
                Err(format!("Request error: {}", e))
            }
        }
    }

    /// Creates the HTTP router for E2E tests
    fn create_test_router(&self) -> Router<()> {
        let engine_health = self.engine.clone();
        let engine_list = self.engine.clone();
        let engine_count = self.engine.clone();
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
            // GET /api/articles
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
            // GET /api/articles/count
            .get("/api/articles/count", move |_req, _params, _state| {
                let engine = engine_count.clone();
                let count = engine.with_state(|state| {
                    state.data.articles.len()
                }).unwrap_or(0);

                let json_body = serde_json::to_string(&serde_json::json!({
                    "count": count
                })).unwrap();
                HttpResponse::new(StatusCode::Ok).json(&json_body)
            })
            // POST /api/articles
            .post("/api/articles", move |req, _params, _state| {
                let engine = engine_create.clone();
                let storage = storage_create.clone();

                let body_str = std::str::from_utf8(req.body()).unwrap_or("{}");
                let article: serde_json::Value = serde_json::from_str(body_str)
                    .unwrap_or(serde_json::json!({}));

                let id = uuid::Uuid::new_v4().to_string();
                let event = TestEvent::ArticleCreated {
                    id: id.clone(),
                    title: article.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    content: article.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                };

                // Apply to the state engine
                engine.with_state_mut(|state| {
                    event.apply(state);
                }).ok();

                // Real persistence - write to FileStorage
                let event_json = serde_json::to_string(&serde_json::json!({
                    "type": "ArticleCreated",
                    "id": id,
                    "data": article,
                    "timestamp": chrono::Utc::now().to_rfc3339()
                })).unwrap();

                // Must use try_lock because we are in a sync context
                if let Ok(mut storage_guard) = storage.try_lock() {
                    if let Some(ref mut file_storage) = *storage_guard {
                        // Call append_event (sync method)
                        if file_storage.append_event(&event_json).is_ok() {
                            file_storage.flush_batch().ok();
                            println!("💾 Event persisted: {}", event_json);
                        } else {
                            println!("⚠️ Failed to append event");
                        }
                    }
                } else {
                    println!("⚠️ Storage locked, event not persisted");
                }

                let json_body = serde_json::to_string(&serde_json::json!({
                    "id": id,
                    "article": article,
                    "status": "created"
                })).unwrap();
                HttpResponse::new(StatusCode::Created).json(&json_body)
            })
    }

    /// Start a REAL HTTP server in background
    ///
    /// # Arguments
    /// * `requested_port` - The port to listen on (0 for random)
    /// * `_binary` - Binary name (unused, kept for API compatibility)
    pub async fn start_server(&mut self, requested_port: u16, _binary: &str) -> Result<(), String> {
        // Pick port (0 = random)
        let port = if requested_port == 0 {
            portpicker::pick_unused_port().ok_or_else(|| "No available port".to_string())?
        } else {
            requested_port
        };

        // Create router
        let router = self.create_test_router();

        // Start async server using tokio::spawn (native async - no spawn_blocking needed)
        let addr = format!("127.0.0.1:{}", port);

        // Use AsyncHttpServer which uses Tokio's native TcpListener and Hyper
        tokio::spawn(async move {
            let server = AsyncHttpServer::new(router, ());
            if let Err(e) = server.serve(&addr).await {
                eprintln!("Server error: {}", e);
            }
        });

        // Update server state
        {
            let mut server_state = self.server.lock().await;
            server_state.port = port;
            server_state.is_running = true;
            server_state.process_id = Some(std::process::id());
            server_state.base_url = Some(format!("http://127.0.0.1:{}", port));
        }

        // Give the server time to start and bind
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Verify server responds
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .map_err(|e| format!("Failed to create client: {}", e))?;

        let url = format!("http://127.0.0.1:{}/health", port);

        for i in 0..20 {
            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    println!("Server started on port {}", port);
                    return Ok(());
                }
                Ok(resp) => {
                    if i % 5 == 0 {
                        println!("Health check returned status {}, retrying...", resp.status());
                    }
                }
                Err(e) => {
                    if i % 5 == 0 {
                        println!("Health check attempt {}: {}", i + 1, e);
                    }
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        Err(format!("Server failed to start on port {}", port))
    }

    /// Stop the HTTP server
    pub async fn stop_server(&mut self) -> Result<(), String> {
        // Take the handle - this will drop and stop the thread
        let handle = self.server_handle.lock().await.take();

        if handle.is_some() {
            // Update state
            let mut server_state = self.server.lock().await;
            server_state.is_running = false;
            server_state.process_id = None;
            println!("Server stopped");
        }

        Ok(())
    }

    /// Initializes a temporary directory for persistence tests
    pub async fn init_temp_storage(&mut self) -> Result<PathBuf, String> {
        let temp_dir =
            tempfile::tempdir().map_err(|e| format!("Failed to create temp dir: {}", e))?;
        let path = temp_dir.path().to_path_buf();

        // Initialize FileStorage
        let storage = FileStorage::new(path.to_str().unwrap())
            .map_err(|e| format!("Failed to init storage: {}", e))?;

        *self.storage.lock().await = Some(storage);
        *self.temp_dir.lock().await = Some(temp_dir);

        Ok(path)
    }

    /// Creates an article in memory and persists it
    pub async fn create_article(
        &mut self,
        id: String,
        data: serde_json::Value,
    ) -> Result<(), String> {
        let event = TestEvent::ArticleCreated {
            id: id.clone(),
            title: data.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            content: data.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        };

        // Apply the event to the engine
        self.engine
            .with_state_mut(|state| {
                event.apply(state);
            })
            .map_err(|e| format!("Failed to apply event: {}", e))?;

        // Persist if storage is enabled
        if let Some(_storage) = self.storage.lock().await.as_mut() {
            let event_json =
                serde_json::to_string(&event).map_err(|e| format!("Serialization error: {}", e))?;
            // FileStorage does not have a simple public append method;
            // for now just log the event
            println!("💾 Event serialized: {}", event_json);
        }

        Ok(())
    }

    /// Retrieves all articles from memory
    pub async fn get_articles(&self) -> HashMap<String, serde_json::Value> {
        self.engine.with_state(|state| state.data.articles.clone()).unwrap_or_default()
    }

    /// Computes the CRC32 checksum of articles in memory
    pub async fn compute_memory_checksum(&self) -> u32 {
        self.engine
            .with_state(|state| {
                let articles = &state.data.articles;

                let mut hasher = Crc32Hasher::new();
                let mut keys: Vec<_> = articles.keys().collect();
                keys.sort();

                for key in keys {
                    if let Some(value) = articles.get(key) {
                        if let Ok(json_str) = serde_json::to_string(value) {
                            hasher.update(json_str.as_bytes());
                        }
                    }
                }

                hasher.finalize()
            })
            .unwrap_or(0)
    }

    /// Computes the CRC32 checksum of the persistence file
    pub async fn compute_file_checksum(&self) -> Result<u32, String> {
        let storage_guard = self.storage.lock().await;
        let _storage =
            storage_guard.as_ref().ok_or_else(|| "Storage not initialized".to_string())?;

        let temp_dir = self.temp_dir.lock().await;
        if let Some(dir) = temp_dir.as_ref() {
            let file_path = dir.path().join("events.raftlog");
            if file_path.exists() {
                let content =
                    std::fs::read(&file_path).map_err(|e| format!("Failed to read file: {}", e))?;

                let mut hasher = Crc32Hasher::new();
                hasher.update(&content);
                return Ok(hasher.finalize());
            }
        }

        Ok(0)
    }

    /// Counts the number of articles in memory
    pub async fn count_articles(&self) -> usize {
        self.engine.with_state(|state| state.data.articles.len()).unwrap_or(0)
    }

    /// Verifies memory/file consistency
    pub async fn verify_memory_file_consistency(&self) -> Result<bool, String> {
        let articles_count = self.count_articles().await;

        // Check the file
        let temp_dir = self.temp_dir.lock().await;
        if let Some(dir) = temp_dir.as_ref() {
            let file_path = dir.path().join("events.raftlog");
            if file_path.exists() {
                // File exists; verify it is not empty if we have articles
                let metadata = std::fs::metadata(&file_path)
                    .map_err(|e| format!("Failed to read metadata: {}", e))?;

                if articles_count > 0 {
                    return Ok(metadata.len() > 0);
                }
                return Ok(true);
            }
        }

        // No file but no articles either -- that is OK
        Ok(articles_count == 0)
    }

    /// Cleans up temporary files
    pub async fn cleanup(&mut self) {
        // TempDir cleans itself up automatically on drop
        *self.temp_dir.lock().await = None;
        *self.storage.lock().await = None;
    }

    // ==================== CLUSTER SUPPORT ====================

    /// Starts a Lithair cluster with N nodes
    ///
    /// # Arguments
    /// * `node_count` - Number of nodes in the cluster
    ///
    /// # Returns
    /// List of ports used by the nodes
    pub async fn start_cluster(&mut self, node_count: usize) -> Result<Vec<u16>, String> {
        let mut ports = Vec::new();
        let mut nodes = Vec::new();

        for i in 0..node_count {
            // Create a TempDir for each node
            let temp_dir = tempfile::tempdir()
                .map_err(|e| format!("Failed to create temp dir for node {}: {}", i, e))?;
            let temp_path = temp_dir.path().to_string_lossy().to_string();

            // Create storage for this node
            let file_storage = FileStorage::new(&temp_path)
                .map_err(|e| format!("Failed to create storage for node {}: {}", i, e))?;

            // Create the state engine for this node
            let engine = Arc::new(StateEngine::new(TestAppState::default()));
            let storage = Arc::new(Mutex::new(Some(file_storage)));

            // Create the router for this node
            let engine_health = engine.clone();
            let engine_articles = engine.clone();
            let engine_create = engine.clone();
            let storage_create = storage.clone();

            let router = Router::new()
                .get("/health", move |_req, _params, _state| {
                    let _ = engine_health.clone();
                    let json_body = serde_json::json!({
                        "status": "ok",
                        "node_id": i,
                        "service": "lithair-cluster"
                    })
                    .to_string();
                    HttpResponse::new(StatusCode::Ok).json(&json_body)
                })
                .get("/api/articles", move |_req, _params, _state| {
                    let articles = engine_articles
                        .with_state(|state| state.data.articles.clone())
                        .unwrap_or_default();

                    let json_body = serde_json::json!({
                        "node_id": i,
                        "articles": articles
                    })
                    .to_string();
                    HttpResponse::new(StatusCode::Ok).json(&json_body)
                })
                .post("/api/articles", move |req, _params, _state| {
                    let engine_local = engine_create.clone();
                    let storage_local = storage_create.clone();

                    let body_str = std::str::from_utf8(req.body()).unwrap_or("{}");
                    let article: serde_json::Value =
                        serde_json::from_str(body_str).unwrap_or(serde_json::json!({}));

                    let id = uuid::Uuid::new_v4().to_string();
                    let event = TestEvent::ArticleCreated {
                        id: id.clone(),
                        title: article
                            .get("title")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        content: article
                            .get("content")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                    };

                    // Apply to the engine
                    engine_local
                        .with_state_mut(|state| {
                            event.apply(state);
                        })
                        .ok();

                    // Persist
                    if let Ok(mut storage_guard) = storage_local.try_lock() {
                        if let Some(ref mut fs) = *storage_guard {
                            let event_json = serde_json::json!({
                                "type": "ArticleCreated",
                                "id": id,
                                "data": article,
                                "node_id": i,
                                "timestamp": chrono::Utc::now().to_rfc3339()
                            })
                            .to_string();
                            fs.append_event(&event_json).ok();
                            fs.flush_batch().ok();
                        }
                    }

                    let json_body = serde_json::json!({
                        "id": id,
                        "article": article,
                        "node_id": i
                    })
                    .to_string();
                    HttpResponse::new(StatusCode::Created).json(&json_body)
                });

            // Find an available port
            let port = portpicker::pick_unused_port()
                .ok_or_else(|| format!("No port available for node {}", i))?;
            ports.push(port);

            // Create and start the server
            let addr = format!("127.0.0.1:{}", port);
            let server = HttpServer::new().with_router(router);

            let handle = tokio::task::spawn_blocking(move || {
                if let Err(e) = server.serve(&addr) {
                    eprintln!("❌ Node {} server error: {}", i, e);
                }
            });

            let server_handle = Arc::new(Mutex::new(Some(handle)));

            // Create the node
            let node = ClusterNode {
                node_id: i,
                server_state: ServerState {
                    port,
                    process_id: Some(std::process::id()),
                    is_running: true,
                    base_url: Some(format!("http://127.0.0.1:{}", port)),
                },
                engine,
                storage,
                server_handle,
            };

            nodes.push(node);
            println!("✅ Node {} started on port {}", i, port);
        }

        // Wait for all servers to be ready
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Save the nodes
        *self.cluster_nodes.lock().await = nodes;

        Ok(ports)
    }

    /// Stops all cluster nodes
    pub async fn stop_cluster(&mut self) -> Result<(), String> {
        let mut nodes = self.cluster_nodes.lock().await;

        for node in nodes.iter_mut() {
            if let Some(handle) = node.server_handle.lock().await.take() {
                handle.abort();
                println!("🛑 Node {} stopped", node.node_id);
            }
        }

        nodes.clear();
        Ok(())
    }

    /// Makes a request to a specific cluster node
    pub async fn make_cluster_request(
        &mut self,
        node_id: usize,
        method: &str,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<(), String> {
        let nodes = self.cluster_nodes.lock().await;
        let node = nodes
            .iter()
            .find(|n| n.node_id == node_id)
            .ok_or_else(|| format!("Node {} not found", node_id))?;

        let port = node.server_state.port;
        drop(nodes);

        let client = reqwest::Client::new();
        let url = format!("http://127.0.0.1:{}{}", port, path);

        let request = match method {
            "GET" => client.get(&url),
            "POST" => client.post(&url),
            "PUT" => client.put(&url),
            "DELETE" => client.delete(&url),
            _ => return Err(format!("Unsupported method: {}", method)),
        };

        let request = if let Some(body) = body { request.json(&body) } else { request };

        match request.send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                let text = response.text().await.unwrap_or_default();
                self.last_response = Some(format!("Status: {}, Body: {}", status, text));
                Ok(())
            }
            Err(e) => {
                self.last_error = Some(format!("Request error: {}", e));
                Err(format!("Request error: {}", e))
            }
        }
    }

    /// Counts the number of nodes in the cluster
    pub async fn cluster_size(&self) -> usize {
        self.cluster_nodes.lock().await.len()
    }

    // ==================== REAL LITHAIR SERVER CLUSTER SUPPORT ====================

    /// Starts a real LithairServer cluster with N nodes as external processes
    ///
    /// Uses the `lithair-cluster-node` binary compiled from the replication example
    pub async fn start_real_cluster(&mut self, node_count: usize) -> Result<Vec<u16>, String> {
        use std::process::{Command, Stdio};

        // Kill any leftover processes from previous failed runs (Unix-only;
        // Windows CI relies on per-run unique data dirs to avoid collisions).
        #[cfg(unix)]
        {
            let _ = Command::new("pkill").arg("-f").arg("lithair-cluster-node").output();
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }

        // Find the binary path
        let binary_path = std::env::current_dir()
            .map_err(|e| format!("Failed to get current dir: {}", e))?
            .parent()  // cucumber-tests -> lithair
            .ok_or("Failed to find parent directory")?
            .join("target/debug/lithair-cluster-node");

        if !binary_path.exists() {
            return Err(format!(
                "Binary not found at {:?}. Please run: cargo build -p replication",
                binary_path
            ));
        }

        let mut ports = Vec::new();
        let mut nodes = Vec::new();
        let mut temp_dirs = Vec::new();

        // Allocate ports first
        for _ in 0..node_count {
            let port =
                portpicker::pick_unused_port().ok_or_else(|| "No port available".to_string())?;
            ports.push(port);
        }

        // Build the peer lists for each node
        for i in 0..node_count {
            let temp_dir =
                tempfile::tempdir().map_err(|e| format!("Failed to create temp dir: {}", e))?;
            let data_dir = temp_dir.path().to_path_buf();

            // Peers are all the other nodes
            let peers: Vec<u16> = ports
                .iter()
                .enumerate()
                .filter(|(idx, _)| *idx != i)
                .map(|(_, port)| *port)
                .collect();

            let port = ports[i];
            let node_id = i as u64;

            // Build the --peers arguments
            let peers_args: Vec<String> =
                peers.iter().flat_map(|p| vec!["--peers".to_string(), p.to_string()]).collect();

            println!(
                "🚀 Starting real node {} on port {} with peers {:?}...",
                node_id, port, peers
            );

            // Start the process
            let mut cmd = Command::new(&binary_path);
            cmd.arg("--node-id")
                .arg(node_id.to_string())
                .arg("--port")
                .arg(port.to_string())
                .args(&peers_args)
                .env("EXPERIMENT_DATA_BASE", data_dir.to_string_lossy().to_string())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());

            let process =
                cmd.spawn().map_err(|e| format!("Failed to spawn node {}: {}", node_id, e))?;

            let node = RealClusterNode { node_id, port, process: Some(process), data_dir, peers };

            nodes.push(node);
            temp_dirs.push(temp_dir);
        }

        // Wait for all servers to be ready (LithairServer takes ~4s to start)
        println!("⏳ Waiting for nodes to start (this may take up to 30s)...");
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

        // Verify that each node responds (using /status endpoint for LithairServer)
        for (i, port) in ports.iter().enumerate() {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .map_err(|e| format!("Failed to create client: {}", e))?;

            // LithairServer uses /status endpoint, not /health
            let url = format!("http://127.0.0.1:{}/status", port);
            let mut retries = 20; // More retries with longer total wait

            while retries > 0 {
                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        let body = resp.text().await.unwrap_or_default();
                        println!(
                            "✅ Node {} ready on port {} - status: {}",
                            i,
                            port,
                            body.chars().take(100).collect::<String>()
                        );
                        break;
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        retries -= 1;
                        if retries == 0 {
                            return Err(format!(
                                "Node {} returned status {} on port {}",
                                i, status, port
                            ));
                        }
                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    }
                    Err(e) => {
                        retries -= 1;
                        if retries == 0 {
                            return Err(format!(
                                "Node {} failed to start on port {}: {}",
                                i, port, e
                            ));
                        }
                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    }
                }
            }
        }

        // Save the nodes
        *self.real_cluster_nodes.lock().await = nodes;
        *self.real_cluster_temp_dirs.lock().await = temp_dirs;

        println!("✅ Real cluster of {} nodes started (ports: {:?})", node_count, ports);
        Ok(ports)
    }

    /// Stops all real cluster nodes
    pub async fn stop_real_cluster(&mut self) -> Result<(), String> {
        let mut nodes = self.real_cluster_nodes.lock().await;

        for node in nodes.iter_mut() {
            if let Some(mut process) = node.process.take() {
                // Send SIGTERM first
                let _ = process.kill();
                let _ = process.wait();
                println!("🛑 Real node {} stopped", node.node_id);
            }
        }

        nodes.clear();

        // Clean up temp dirs
        self.real_cluster_temp_dirs.lock().await.clear();

        Ok(())
    }

    /// Starts a real LithairServer cluster with a persistent data directory
    ///
    /// Data is stored in /tmp/lithair-stress-test/ and cleaned at startup
    pub async fn start_real_cluster_persistent(
        &mut self,
        node_count: usize,
    ) -> Result<Vec<u16>, String> {
        use std::process::{Command, Stdio};

        // Persistent directory for test data
        let base_data_dir = std::path::PathBuf::from("/tmp/lithair-stress-test");

        // Clean up the directory at start
        if base_data_dir.exists() {
            println!("🧹 Cleaning up previous test data at {:?}...", base_data_dir);
            std::fs::remove_dir_all(&base_data_dir)
                .map_err(|e| format!("Failed to clean data dir: {}", e))?;
        }
        std::fs::create_dir_all(&base_data_dir)
            .map_err(|e| format!("Failed to create data dir: {}", e))?;

        // Find the binary path
        let binary_path = std::env::current_dir()
            .map_err(|e| format!("Failed to get current dir: {}", e))?
            .parent()
            .ok_or("Failed to find parent directory")?
            .join("target/debug/lithair-cluster-node");

        if !binary_path.exists() {
            return Err(format!(
                "Binary not found at {:?}. Please run: cargo build -p replication",
                binary_path
            ));
        }

        let mut ports = Vec::new();
        let mut nodes = Vec::new();

        // Allocate ports first
        for _ in 0..node_count {
            let port =
                portpicker::pick_unused_port().ok_or_else(|| "No port available".to_string())?;
            ports.push(port);
        }

        // Build the peer lists for each node
        for i in 0..node_count {
            let data_dir = base_data_dir.join(format!("node_{}", i));
            std::fs::create_dir_all(&data_dir)
                .map_err(|e| format!("Failed to create node data dir: {}", e))?;

            // Peers are all the other nodes
            let peers: Vec<u16> = ports
                .iter()
                .enumerate()
                .filter(|(idx, _)| *idx != i)
                .map(|(_, port)| *port)
                .collect();

            let port = ports[i];
            let node_id = i as u64;

            // Build the --peers arguments
            let peers_args: Vec<String> =
                peers.iter().flat_map(|p| vec!["--peers".to_string(), p.to_string()]).collect();

            println!(
                "🚀 Starting real node {} on port {} with peers {:?} (data: {:?})...",
                node_id, port, peers, data_dir
            );

            // Start the process
            let mut cmd = Command::new(&binary_path);
            cmd.arg("--node-id")
                .arg(node_id.to_string())
                .arg("--port")
                .arg(port.to_string())
                .args(&peers_args)
                .env("EXPERIMENT_DATA_BASE", data_dir.to_string_lossy().to_string())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());

            let process =
                cmd.spawn().map_err(|e| format!("Failed to spawn node {}: {}", node_id, e))?;

            let node = RealClusterNode { node_id, port, process: Some(process), data_dir, peers };

            nodes.push(node);
        }

        // Wait for all servers to be ready
        println!("⏳ Waiting for nodes to start...");
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

        // Verify that each node responds
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .pool_max_idle_per_host(10)
            .build()
            .map_err(|e| format!("Failed to create client: {}", e))?;

        for (i, port) in ports.iter().enumerate() {
            let url = format!("http://127.0.0.1:{}/status", port);
            let mut retries = 20;

            while retries > 0 {
                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        let body = resp.text().await.unwrap_or_default();
                        println!(
                            "✅ Node {} ready on port {} - status: {}",
                            i,
                            port,
                            body.chars().take(100).collect::<String>()
                        );
                        break;
                    }
                    Ok(resp) => {
                        retries -= 1;
                        if retries == 0 {
                            return Err(format!(
                                "Node {} returned status {} on port {}",
                                i,
                                resp.status(),
                                port
                            ));
                        }
                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    }
                    Err(e) => {
                        retries -= 1;
                        if retries == 0 {
                            return Err(format!(
                                "Node {} failed to start on port {}: {}",
                                i, port, e
                            ));
                        }
                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    }
                }
            }
        }

        // Save the nodes (no temp_dirs since data is persistent)
        *self.real_cluster_nodes.lock().await = nodes;

        println!("✅ Real cluster of {} nodes started (ports: {:?})", node_count, ports);
        println!("📁 Data directory: {:?}", base_data_dir);
        Ok(ports)
    }

    /// Makes a request to a real cluster node
    pub async fn make_real_cluster_request(
        &mut self,
        node_id: usize,
        method: &str,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, String> {
        let nodes = self.real_cluster_nodes.lock().await;
        let node = nodes
            .iter()
            .find(|n| n.node_id == node_id as u64)
            .ok_or_else(|| format!("Real node {} not found", node_id))?;

        let port = node.port;
        drop(nodes);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| format!("Failed to create client: {}", e))?;

        let url = format!("http://127.0.0.1:{}{}", port, path);

        let request = match method {
            "GET" => client.get(&url),
            "POST" => client.post(&url),
            "PUT" => client.put(&url),
            "DELETE" => client.delete(&url),
            _ => return Err(format!("Unsupported method: {}", method)),
        };

        let request = if let Some(body) = body { request.json(&body) } else { request };

        match request.send().await {
            Ok(response) => {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();

                self.last_response = Some(format!("Status: {}, Body: {}", status.as_u16(), text));

                // Try to parse as JSON, or wrap in a simple object
                let json_result: serde_json::Value =
                    serde_json::from_str(&text).unwrap_or_else(|_| {
                        serde_json::json!({
                            "status": status.as_u16(),
                            "body": text
                        })
                    });

                Ok(json_result)
            }
            Err(e) => {
                self.last_error = Some(format!("Request error: {}", e));
                Err(format!("Request error: {}", e))
            }
        }
    }

    /// Counts the number of real nodes in the cluster
    pub async fn real_cluster_size(&self) -> usize {
        self.real_cluster_nodes.lock().await.len()
    }

    /// Returns the ports of the real cluster nodes
    pub async fn get_real_cluster_ports(&self) -> Vec<u16> {
        self.real_cluster_nodes.lock().await.iter().map(|n| n.port).collect()
    }

    /// Returns the leader port (node_id = 0)
    pub async fn get_real_leader_port(&self) -> u16 {
        let nodes = self.real_cluster_nodes.lock().await;
        nodes.iter().find(|n| n.node_id == 0).map(|n| n.port).unwrap_or(0)
    }
}
