#![allow(dead_code)]
use std::path::Path;
/// # Helpers pour Tests de Build
///
/// Ce module fournit des utilitaires réutilisables pour tester le binaire final.
/// C'est l'équivalent des "steps" Cucumber mais pour les tests de build.
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tokio::time::sleep;

/// Structure pour gérer un serveur Lithair de test
pub struct TestServer {
    pub process: Child,
    pub port: u16,
    pub config_path: String,
}

impl TestServer {
    /// Démarre un serveur Lithair avec une configuration
    pub async fn start(port: u16, config_toml: &str) -> Result<Self, String> {
        // 1. Écrire la config
        let config_path = format!("/tmp/lithair-test-{}.toml", port);
        std::fs::write(&config_path, config_toml)
            .map_err(|e| format!("Failed to write config: {}", e))?;

        // 2. Compiler si nécessaire
        Self::ensure_binary_exists()?;

        // 3. Lancer le serveur
        let process = Command::new("../target/release/lithair")
            .args(["--config", &config_path])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to start server: {}", e))?;

        // 4. Attendre qu'il démarre
        sleep(Duration::from_secs(2)).await;

        Ok(TestServer { process, port, config_path })
    }

    /// Démarre avec config par défaut
    pub async fn start_default(port: u16) -> Result<Self, String> {
        let config = format!(
            r#"
[server]
port = {}

[persistence]
enabled = true
path = "/tmp/lithair-test-{}"
"#,
            port, port
        );

        Self::start(port, &config).await
    }

    /// Vérifie que le binaire existe, sinon le compile
    fn ensure_binary_exists() -> Result<(), String> {
        let binary_path = Path::new("../target/release/lithair");

        if !binary_path.exists() {
            println!("⚠️ Binary not found, compiling...");
            let status = Command::new("cargo")
                .args(["build", "--release", "--bin", "lithair"])
                .current_dir("../")
                .status()
                .map_err(|e| format!("Failed to compile: {}", e))?;

            if !status.success() {
                return Err("Compilation failed".to_string());
            }
        }

        Ok(())
    }

    /// Fait une requête GET au serveur
    pub async fn get(&self, path: &str) -> Result<String, String> {
        let url = format!("http://127.0.0.1:{}{}", self.port, path);
        let response =
            reqwest::get(&url).await.map_err(|e| format!("GET {} failed: {}", url, e))?;

        response.text().await.map_err(|e| format!("Failed to read response: {}", e))
    }

    /// Fait une requête POST au serveur
    pub async fn post(&self, path: &str, body: serde_json::Value) -> Result<String, String> {
        let url = format!("http://127.0.0.1:{}{}", self.port, path);
        let client = reqwest::Client::new();
        let response = client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("POST {} failed: {}", url, e))?;

        response.text().await.map_err(|e| format!("Failed to read response: {}", e))
    }

    /// Vérifie que le serveur répond
    pub async fn health_check(&self) -> Result<(), String> {
        let response = self.get("/health").await?;

        if response.contains("ok") || response.contains("200") {
            Ok(())
        } else {
            Err(format!("Health check failed: {}", response))
        }
    }

    /// Arrête le serveur proprement
    pub fn stop(&mut self) -> Result<(), String> {
        self.process.kill().map_err(|e| format!("Failed to stop server: {}", e))?;

        // Nettoyer la config
        std::fs::remove_file(&self.config_path).ok();

        Ok(())
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop().ok();
    }
}

/// Helper pour tester la performance
pub struct LoadTester {
    client: reqwest::Client,
}

impl LoadTester {
    pub fn new() -> Self {
        LoadTester { client: reqwest::Client::new() }
    }

    /// Lance N requêtes concurrentes
    pub async fn run_concurrent_requests(
        &self,
        url: &str,
        count: usize,
    ) -> Result<LoadTestResult, String> {
        let start = std::time::Instant::now();

        let mut tasks = vec![];
        for _ in 0..count {
            let client = self.client.clone();
            let url = url.to_string();
            tasks.push(tokio::spawn(async move { client.get(&url).send().await }));
        }

        let results = futures::future::join_all(tasks).await;
        let duration = start.elapsed();

        let success_count =
            results.iter().filter(|r| r.is_ok() && r.as_ref().unwrap().is_ok()).count();

        Ok(LoadTestResult {
            total: count,
            success: success_count,
            failed: count - success_count,
            duration,
        })
    }
}

pub struct LoadTestResult {
    pub total: usize,
    pub success: usize,
    pub failed: usize,
    pub duration: Duration,
}

impl LoadTestResult {
    pub fn success_rate(&self) -> f64 {
        (self.success as f64 / self.total as f64) * 100.0
    }

    pub fn requests_per_second(&self) -> f64 {
        self.total as f64 / self.duration.as_secs_f64()
    }
}

/// Helper pour vérifier la persistence
pub struct PersistenceChecker;

impl PersistenceChecker {
    /// Vérifie qu'un fichier de persistence existe
    pub fn check_file_exists(path: &str) -> bool {
        Path::new(path).exists()
    }

    /// Vérifie le contenu d'un fichier de persistence
    pub fn check_file_contains(path: &str, text: &str) -> Result<bool, String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("Failed to read {}: {}", path, e))?;

        Ok(content.contains(text))
    }

    /// Compte le nombre d'événements dans le log
    pub fn count_events(path: &str) -> Result<usize, String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("Failed to read {}: {}", path, e))?;

        Ok(content.lines().count())
    }
}

/// Helper pour tester les options CLI
pub struct CliTester;

impl CliTester {
    /// Teste la commande --help
    pub fn test_help() -> Result<String, String> {
        let output = Command::new("../target/release/lithair")
            .arg("--help")
            .output()
            .map_err(|e| format!("Failed to run --help: {}", e))?;

        if !output.status.success() {
            return Err("--help command failed".to_string());
        }

        String::from_utf8(output.stdout).map_err(|e| format!("Invalid UTF-8: {}", e))
    }

    /// Teste la commande --version
    pub fn test_version() -> Result<String, String> {
        let output = Command::new("../target/release/lithair")
            .arg("--version")
            .output()
            .map_err(|e| format!("Failed to run --version: {}", e))?;

        if !output.status.success() {
            return Err("--version command failed".to_string());
        }

        String::from_utf8(output.stdout).map_err(|e| format!("Invalid UTF-8: {}", e))
    }
}
