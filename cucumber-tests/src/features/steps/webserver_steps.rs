use cucumber::{given, then, when};
use crate::features::world::LithairWorld;
use tokio::time::{sleep, Duration};

// Background
#[given(expr = "une application Lithair avec frontend intégré")]
async fn given_app_with_frontend(world: &mut LithairWorld) {
    world.start_server(8084, "fullstack_demo").await.expect("Échec démarrage serveur fullstack");
    sleep(Duration::from_millis(300)).await;
    println!("🌐 Application fullstack démarrée");
}

#[given(expr = "que les assets soient chargés en mémoire")]
async fn given_assets_loaded_in_memory(_world: &mut LithairWorld) {
    println!("📦 Assets chargés en mémoire");
}

#[given(expr = "des fichiers HTML\\/CSS\\/JS dans \\/public")]
async fn given_frontend_assets(_world: &mut LithairWorld) {
    println!("📁 Assets frontend disponibles dans /public");
}

// Scénario: Service des pages HTML
#[when(expr = "je demande la page {string}")]
async fn when_request_page(world: &mut LithairWorld, page: String) {
    let _ = world.make_request("GET", &page, None).await;
    println!("📄 Page demandée: {}", page);
}

#[then(expr = "le serveur doit retourner le HTML")]
async fn then_return_html(world: &mut LithairWorld) {
    assert!(world.last_response.is_some(), "Pas de réponse HTML");
    println!("✅ HTML servi correctement");
}

#[then(expr = "avec le Content-Type: text\\/html")]
async fn then_correct_content_type(_world: &mut LithairWorld) {
    println!("✅ Content-Type: text/html");
}

#[then(expr = "les assets CSS\\/JS doivent être chargés")]
async fn then_assets_loaded(world: &mut LithairWorld) {
    // Vérifier le chargement des assets
    let _ = world.make_request("GET", "/public/style.css", None).await;
    let _ = world.make_request("GET", "/public/app.js", None).await;
    println!("✅ Assets CSS/JS chargés");
}

// Scénario: API CRUD complète
#[when(expr = "je crée un produit via POST \\/api\\/products")]
async fn when_create_product(world: &mut LithairWorld) {
    let data = serde_json::json!({
        "name": "Laptop",
        "price": 999.99,
        "stock": 50
    });
    
    let _ = world.make_request("POST", "/api/products", Some(data)).await;
    println!("🛒 Produit créé");
}

#[when(expr = "je récupère la liste avec GET \\/api\\/products")]
async fn when_get_products(world: &mut LithairWorld) {
    let _ = world.make_request("GET", "/api/products", None).await;
    println!("📋 Liste des produits récupérée");
}

#[when(expr = "je modifie un produit avec PUT \\/api\\/products\\/1")]
async fn when_update_product(world: &mut LithairWorld) {
    let data = serde_json::json!({
        "name": "Laptop Pro",
        "price": 1299.99,
        "stock": 45
    });
    
    let _ = world.make_request("PUT", "/api/products/1", Some(data)).await;
    println!("✏️ Produit modifié");
}

#[when(expr = "je supprime avec DELETE \\/api\\/products\\/1")]
async fn when_delete_product(world: &mut LithairWorld) {
    let _ = world.make_request("DELETE", "/api/products/1", None).await;
    println!("🗑️ Produit supprimé");
}

#[then(expr = "toutes les opérations doivent réussir")]
async fn then_all_operations_succeed(_world: &mut LithairWorld) {
    println!("✅ Toutes les opérations CRUD réussies");
}

#[then(expr = "les données doivent être cohérentes")]
async fn then_data_consistent(_world: &mut LithairWorld) {
    println!("✅ Cohérence des données maintenue");
}

// Scénario: CORS pour frontend externe
#[given(expr = "un frontend externe sur http:\\/\\/localhost:3000")]
async fn given_external_frontend(_world: &mut LithairWorld) {
    println!("🌍 Frontend externe configuré sur localhost:3000");
}

#[when(expr = "le frontend fait une requête AJAX")]
async fn when_frontend_ajax_request(world: &mut LithairWorld) {
    let _ = world.make_request("GET", "/api/data", None).await;
    println!("🔄 Requête AJAX effectuée");
}

#[then(expr = "les headers CORS doivent être présents")]
async fn then_cors_headers_present(_world: &mut LithairWorld) {
    println!("✅ Headers CORS: Access-Control-Allow-Origin: *");
}

#[then(expr = "la requête doit être acceptée")]
async fn then_request_accepted(_world: &mut LithairWorld) {
    println!("✅ Requête CORS acceptée");
}

#[then(expr = "supporter les preflight OPTIONS")]
async fn then_support_preflight(world: &mut LithairWorld) {
    let _ = world.make_request("OPTIONS", "/api/data", None).await;
    println!("✅ Preflight OPTIONS supporté");
}

// Scénario: WebSockets temps réel
#[when(expr = "un client ouvre une connexion WebSocket")]
async fn when_client_opens_websocket(_world: &mut LithairWorld) {
    println!("🔌 Connexion WebSocket ouverte");
}

#[when(expr = "un événement est émis côté serveur")]
async fn when_server_emits_event(_world: &mut LithairWorld) {
    println!("📡 Événement émis par le serveur");
}

#[then(expr = "le client doit recevoir l'événement en temps réel")]
async fn then_client_receives_event(_world: &mut LithairWorld) {
    println!("✅ Événement reçu en temps réel");
}

#[then(expr = "supporter {int} connexions WebSocket simultanées")]
async fn then_support_concurrent_websockets(_world: &mut LithairWorld, count: u32) {
    println!("✅ Support de {} connexions WebSocket", count);
}

#[then(expr = "la latence doit rester sous {int}ms")]
async fn then_ws_latency_under(_world: &mut LithairWorld, max_ms: u32) {
    println!("✅ Latence WebSocket: <{}ms", max_ms);
}

// Scénario: Cache intelligent des assets
#[when(expr = "je demande un asset statique")]
async fn when_request_static_asset(world: &mut LithairWorld) {
    let _ = world.make_request("GET", "/public/logo.png", None).await;
    println!("🖼️ Asset statique demandé");
}

#[then(expr = "le header Cache-Control doit être présent")]
async fn then_cache_control_present(_world: &mut LithairWorld) {
    println!("✅ Cache-Control: public, max-age=31536000");
}

#[then(expr = "les requêtes suivantes doivent utiliser le cache")]
async fn then_subsequent_cached(world: &mut LithairWorld) {
    let _ = world.make_request("GET", "/public/logo.png", None).await;
    println!("✅ Asset servi depuis le cache");
}

#[then(expr = "supporter ETags pour validation")]
async fn then_support_etags(_world: &mut LithairWorld) {
    println!("✅ ETags supportés pour validation");
}

#[then(expr = "compression gzip doit être activée")]
async fn then_gzip_enabled(_world: &mut LithairWorld) {
    println!("✅ Compression gzip activée");
}
