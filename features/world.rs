use cucumber::World as _;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Default)]
pub struct ServerState {
    pub port: u16,
    pub process_id: Option<u32>,
    pub is_running: bool,
}

#[derive(Debug, Default)]
pub struct Metrics {
    pub request_count: u64,
    pub response_time_ms: f64,
    pub error_rate: f64,
    pub memory_usage_mb: f64,
}

#[derive(Debug, Default)]
pub struct TestData {
    pub articles: HashMap<String, serde_json::Value>,
    pub users: HashMap<String, serde_json::Value>,
    pub tokens: HashMap<String, String>,
}

#[derive(Debug, World)]
pub struct LithairWorld {
    pub server: Arc<Mutex<ServerState>>,
    pub metrics: Arc<Mutex<Metrics>>,
    pub test_data: Arc<Mutex<TestData>>,
    pub last_response: Option<reqwest::Response>,
    pub last_error: Option<String>,
}

impl Default for LithairWorld {
    fn default() -> Self {
        Self {
            server: Arc::new(Mutex::new(ServerState::default())),
            metrics: Arc::new(Mutex::new(Metrics::default())),
            test_data: Arc::new(Mutex::new(TestData::default())),
            last_response: None,
            last_error: None,
        }
    }
}

impl LithairWorld {
    pub async fn start_server(&mut self, port: u16, binary: &str) -> Result<(), String> {
        let mut server = self.server.lock().await;
        
        // Logique pour démarrer le serveur Lithair
        // TODO: Implémenter le démarrage réel du binaire
        
        server.port = port;
        server.is_running = true;
        
        Ok(())
    }
    
    pub async fn stop_server(&mut self) -> Result<(), String> {
        let mut server = self.server.lock().await;
        
        if let Some(pid) = server.process_id {
            // TODO: Tuer le processus proprement
        }
        
        server.is_running = false;
        server.process_id = None;
        
        Ok(())
    }
    
    pub async fn make_request(&mut self, method: &str, path: &str, body: Option<serde_json::Value>) -> Result<reqwest::Response, String> {
        let server = self.server.lock().await;
        let url = format!("http://127.0.0.1:{}{}", server.port, path);
        
        let client = reqwest::Client::new();
        let mut request = match method {
            "GET" => client.get(&url),
            "POST" => client.post(&url),
            "PUT" => client.put(&url),
            "DELETE" => client.delete(&url),
            _ => return Err(format!("Méthode HTTP non supportée: {}", method)),
        };
        
        if let Some(body) = body {
            request = request.json(&body);
        }
        
        request
            .send()
            .await
            .map_err(|e| format!("Erreur requête: {}", e))
    }
}
