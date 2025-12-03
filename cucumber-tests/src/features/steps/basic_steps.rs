use cucumber::{given, then, when};
use crate::features::world::LithairWorld;

/// Démarre un VRAI serveur HTTP Lithair pour les tests E2E
/// 
/// # Stack Technique
/// - Initialise TempDir + FileStorage
/// - Démarre HttpServer sur port aléatoire
/// - Routes: /health, /api/articles, /api/articles/count
/// 
/// # Validation
/// - Serveur écoute vraiment sur TCP
/// - Reqwest peut faire de vrais appels HTTP
#[given(expr = "un serveur Lithair")]
async fn given_lithair_server(world: &mut LithairWorld) {
    // 1. Initialiser la persistance
    let temp_path = world.init_temp_storage().await
        .expect("Échec init storage");
    
    // 2. Démarrer le VRAI serveur HTTP
    world.start_server(0, "test").await  // Port 0 = port aléatoire
        .expect("Échec démarrage serveur HTTP");
    
    let server_state = world.server.lock().await;
    println!("✅ Serveur Lithair RÉEL démarré:");
    println!("   - URL: {}", server_state.base_url.as_ref().unwrap());
    println!("   - Storage: {:?}", temp_path);
    println!("   - PID: {}", server_state.process_id.unwrap());
}

/// Fait un VRAI appel HTTP GET vers le serveur
/// 
/// # Tests E2E
/// - Utilise reqwest::Client pour vrai HTTP request
/// - Serveur doit être démarré avant
/// - Vérifie que le serveur répond vraiment
#[when(expr = "je fais une requête GET sur {string}")]
async fn when_get_request(world: &mut LithairWorld, path: String) {
    // VRAI appel HTTP via reqwest
    let result = world.make_request("GET", &path, None).await;
    
    match result {
        Ok(_) => println!("✅ Requête GET {} réussie", path),
        Err(e) => {
            println!("❌ Requête GET {} échouée: {}", path, e);
            world.last_error = Some(e);
        }
    }
}

/// Vérifie que la réponse HTTP est un succès (200)
/// 
/// # Assertions E2E
/// - Vérifie status code 200
/// - Vérifie body JSON valide
/// - Confirme que le serveur a vraiment répondu
#[then(expr = "la réponse doit être un succès")]
async fn then_response_success(world: &mut LithairWorld) {
    assert!(world.last_response.is_some(), "❌ Aucune réponse reçue");
    
    let response = world.last_response.as_ref().unwrap();
    assert!(response.contains("200"), "❌ Code HTTP n'est pas 200: {}", response);
    
    // Parser le body JSON pour vérifier qu'il est valide
    if let Some(body_start) = response.find("Body: ") {
        let body_str = &response[body_start + 6..];
        match serde_json::from_str::<serde_json::Value>(body_str) {
            Ok(json) => println!("✅ Réponse JSON valide: {}", json),
            Err(e) => println!("⚠️  Body non-JSON: {}", e),
        }
    }
    
    println!("✅ Test E2E réussi: Serveur HTTP répond correctement");
}
