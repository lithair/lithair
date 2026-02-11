//! # Tests d'Intégration Build - Organisation Propre
//!
//! Ce fichier contient les tests du binaire final, organisés en modules.
//! Les helpers réutilisables sont dans `tests/helpers/mod.rs`

#![allow(unused_imports)]
#![allow(dead_code)]

mod helpers;

use helpers::{CliTester, LoadTester, PersistenceChecker, TestServer};

// ==================== MODULE 1 : COMPILATION ====================

mod compilation {
    use std::path::Path;
    use std::process::Command;

    #[test]
    fn test_binary_compiles_successfully() {
        println!("🔨 Test : Compilation du binaire release...");

        let output = Command::new("cargo")
            .args(["build", "--release", "--bin", "lithair"])
            .current_dir("../")
            .output()
            .expect("Failed to execute cargo build");

        assert!(
            output.status.success(),
            "❌ Compilation failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );

        let binary_path = Path::new("../target/release/lithair");
        assert!(binary_path.exists(), "❌ Binary not found");

        println!("✅ Compilation réussie !");
    }
}

// ==================== MODULE 2 : DÉMARRAGE ====================

mod startup {
    use super::*;

    #[tokio::test]
    async fn test_server_starts_with_default_config() {
        println!("🚀 Test : Démarrage avec config par défaut...");

        let mut server = TestServer::start_default(19100).await.expect("Failed to start server");

        // Vérifier que le serveur répond
        server.health_check().await.expect("Health check failed");

        server.stop().ok();
        println!("✅ Serveur démarre et répond !");
    }

    #[tokio::test]
    async fn test_server_starts_with_custom_config() {
        println!("🚀 Test : Démarrage avec config custom...");

        let config = r#"
[server]
port = 19101

[persistence]
enabled = true
path = "/tmp/lithair-custom-test"

[logging]
level = "info"
"#;

        let mut server = TestServer::start(19101, config).await.expect("Failed to start server");

        server.health_check().await.expect("Health check failed");
        server.stop().ok();

        println!("✅ Serveur démarre avec config custom !");
    }
}

// ==================== MODULE 3 : API ====================

mod api_tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_api_get_endpoint() {
        println!("📡 Test : GET endpoint...");

        let mut server = TestServer::start_default(19102).await.expect("Failed to start server");

        // Faire une requête GET
        let response = server.get("/api/status").await.expect("GET request failed");

        assert!(!response.is_empty(), "Response is empty");

        server.stop().ok();
        println!("✅ GET endpoint fonctionne !");
    }

    #[tokio::test]
    async fn test_api_post_endpoint() {
        println!("📡 Test : POST endpoint...");

        let mut server = TestServer::start_default(19103).await.expect("Failed to start server");

        // Faire une requête POST
        let data = json!({
            "title": "Test Article",
            "content": "Test content"
        });

        let response = server.post("/api/articles", data).await.expect("POST request failed");

        assert!(!response.is_empty(), "Response is empty");

        server.stop().ok();
        println!("✅ POST endpoint fonctionne !");
    }
}

// ==================== MODULE 4 : PERFORMANCE ====================

mod performance_tests {
    use super::*;

    #[tokio::test]
    async fn test_server_handles_concurrent_requests() {
        println!("⚡ Test : Requêtes concurrentes...");

        let mut server = TestServer::start_default(19104).await.expect("Failed to start server");

        // Attendre que le serveur soit prêt
        server.health_check().await.expect("Server not ready");

        // Test de charge
        let tester = LoadTester::new();
        let url = format!("http://127.0.0.1:{}/health", server.port);

        let result = tester.run_concurrent_requests(&url, 50).await.expect("Load test failed");

        println!(
            "📊 Résultats : {}/{} réussies ({:.1}%)",
            result.success,
            result.total,
            result.success_rate()
        );
        println!("📊 Performance : {:.1} req/s", result.requests_per_second());

        assert!(
            result.success_rate() >= 90.0,
            "❌ Success rate trop faible : {:.1}%",
            result.success_rate()
        );

        server.stop().ok();
        println!("✅ Serveur gère bien la charge !");
    }
}

// ==================== MODULE 5 : PERSISTENCE ====================

mod persistence_tests {
    use super::*;

    #[tokio::test]
    async fn test_persistence_creates_files() {
        println!("💾 Test : Persistence crée les fichiers...");

        let persistence_path = "/tmp/lithair-persist-test";
        std::fs::create_dir_all(persistence_path).ok();

        let config = format!(
            r#"
[server]
port = 19105

[persistence]
enabled = true
path = "{}"
"#,
            persistence_path
        );

        let mut server = TestServer::start(19105, &config).await.expect("Failed to start server");

        // Faire quelques requêtes pour générer des events
        server.post("/api/test", serde_json::json!({"data": "test"})).await.ok();

        // Vérifier que les fichiers existent
        let events_file = format!("{}/events.raftlog", persistence_path);

        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        let file_exists = PersistenceChecker::check_file_exists(&events_file);

        server.stop().ok();
        std::fs::remove_dir_all(persistence_path).ok();

        assert!(file_exists, "❌ Events file not created");
        println!("✅ Fichiers de persistence créés !");
    }
}

// ==================== MODULE 6 : CLI ====================

mod cli_tests {
    use super::*;

    #[test]
    fn test_help_command() {
        println!("❓ Test : Commande --help...");

        let help = CliTester::test_help().expect("--help failed");

        assert!(!help.is_empty(), "Help is empty");
        assert!(
            help.contains("Lithair") || help.contains("Usage"),
            "Help doesn't contain expected text"
        );

        println!("✅ Commande --help OK !");
    }

    #[test]
    fn test_version_command() {
        println!("📌 Test : Commande --version...");

        let version = CliTester::test_version().expect("--version failed");

        assert!(!version.is_empty(), "Version is empty");

        println!("✅ Version : {}", version.trim());
    }
}

// ==================== MODULE 7 : DOCUMENTATION ====================

mod documentation_tests {
    use std::path::Path;

    #[test]
    fn test_all_docs_exist() {
        println!("📚 Test : Documentation complète...");

        let docs = vec![
            "../README.md",
            "../cucumber-tests/GUIDE_PRATIQUE_UTILISATION.md",
            "../cucumber-tests/POURQUOI_TESTER_BUILDS.md",
        ];

        for doc in docs {
            assert!(Path::new(doc).exists(), "❌ Doc manquante : {}", doc);
        }

        println!("✅ Toute la documentation existe !");
    }
}
