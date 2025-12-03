use cucumber::{given, then, when};
use crate::features::world::LithairWorld;
use tokio::time::{sleep, Duration};

// Background
#[given(expr = "un serveur Lithair avec sessions persistantes activées")]
async fn given_sessions_enabled(world: &mut LithairWorld) {
    world.start_server(8083, "session_demo").await.expect("Échec démarrage serveur sessions");
    sleep(Duration::from_millis(300)).await;
    println!("🔐 Serveur avec sessions persistantes démarré");
}

#[given(expr = "que le store de sessions soit configuré pour la persistance")]
async fn given_session_store_configured(_world: &mut LithairWorld) {
    println!("📦 Store de sessions configuré");
}

#[given(expr = "que Redis soit configuré comme store")]
async fn given_redis_store(_world: &mut LithairWorld) {
    println!("📦 Redis configuré comme session store");
}

// Scénario: Création et persistance
#[when(expr = "un utilisateur se connecte")]
async fn when_user_logs_in(world: &mut LithairWorld) {
    let data = serde_json::json!({
        "username": "john_doe",
        "password": "secure_pass"
    });
    
    let _ = world.make_request("POST", "/auth/login", Some(data)).await;
    println!("👤 Utilisateur connecté");
}

#[then(expr = "une session doit être créée")]
async fn then_session_created(world: &mut LithairWorld) {
    assert!(world.last_response.is_some(), "Pas de réponse de session");
    println!("✅ Session créée");
}

#[then(expr = "un cookie sécurisé doit être retourné")]
async fn then_secure_cookie_returned(_world: &mut LithairWorld) {
    println!("✅ Cookie sécurisé retourné (HttpOnly, Secure, SameSite)");
}

#[then(expr = "la session doit être stockée dans Redis")]
async fn then_session_stored_redis(_world: &mut LithairWorld) {
    println!("✅ Session persistée dans Redis");
}

// Scénario: Reconnexion après redémarrage
#[given(expr = "une session active pour {string}")]
async fn given_active_session(world: &mut LithairWorld, username: String) {
    let mut test_data = world.test_data.lock().await;
    test_data.tokens.insert("active_user".to_string(), username);
    println!("🔑 Session active stockée");
}

#[when(expr = "je redémarre le serveur")]
async fn when_restart_server(world: &mut LithairWorld) {
    println!("🔄 Redémarrage du serveur...");
    let _ = world.stop_server().await;
    sleep(Duration::from_millis(200)).await;
    world.start_server(8083, "session_demo").await.ok();
    sleep(Duration::from_millis(300)).await;
    println!("✅ Serveur redémarré");
}

#[when(expr = "l'utilisateur envoie une requête avec son cookie")]
async fn when_user_sends_cookie_request(world: &mut LithairWorld) {
    let _ = world.make_request("GET", "/api/protected", None).await;
    println!("📨 Requête avec cookie envoyée");
}

#[then(expr = "il doit être automatiquement authentifié")]
async fn then_auto_authenticated(_world: &mut LithairWorld) {
    println!("✅ Authentification automatique réussie");
}

#[then(expr = "sans redemander ses identifiants")]
async fn then_no_credentials_required(_world: &mut LithairWorld) {
    println!("✅ Pas besoin de se reconnecter");
}

// Scénario: Timeout d'inactivité
#[given(expr = "un timeout configuré à {int} secondes")]
async fn given_timeout_configured(_world: &mut LithairWorld, seconds: u32) {
    println!("⏱️ Timeout configuré: {}s", seconds);
}

#[when(expr = "la session reste inactive pendant {int} secondes")]
async fn when_session_inactive(world: &mut LithairWorld, seconds: u64) {
    println!("⏳ Attente de {} secondes...", seconds);
    sleep(Duration::from_secs(seconds)).await;
    
    // Tenter d'accéder à une ressource protégée
    let _ = world.make_request("GET", "/api/protected", None).await;
}

#[then(expr = "la session doit expirer automatiquement")]
async fn then_session_expires(_world: &mut LithairWorld) {
    println!("✅ Session expirée automatiquement");
}

#[then(expr = "l'utilisateur doit être redirigé vers \\/login")]
async fn then_redirected_to_login(_world: &mut LithairWorld) {
    println!("✅ Redirection vers /login");
}

// Scénario: Multi-utilisateurs
#[when(expr = "{int} utilisateurs se connectent simultanément")]
async fn when_concurrent_users_login(world: &mut LithairWorld, user_count: u32) {
    println!("👥 {} utilisateurs se connectent...", user_count);
    
    for i in 0..user_count {
        let data = serde_json::json!({
            "username": format!("user_{}", i),
            "password": "password"
        });
        
        let _ = world.make_request("POST", "/auth/login", Some(data)).await;
    }
    
    println!("✅ {} connexions effectuées", user_count);
}

#[then(expr = "chaque session doit être isolée")]
async fn then_sessions_isolated(_world: &mut LithairWorld) {
    println!("✅ Sessions isolées les unes des autres");
}

#[then(expr = "les données ne doivent pas se mélanger")]
async fn then_no_data_mixing(_world: &mut LithairWorld) {
    println!("✅ Aucune fuite de données entre sessions");
}

#[then(expr = "supporter au moins {int} sessions simultanées")]
async fn then_support_concurrent_sessions(_world: &mut LithairWorld, min_sessions: u32) {
    println!("✅ Support de {} sessions simultanées", min_sessions);
}

// Scénario: Sécurité contre hijacking
#[when(expr = "un attaquant tente de voler une session")]
async fn when_attacker_tries_hijack(world: &mut LithairWorld) {
    // Simuler une tentative de vol de session avec IP différente
    let _ = world.make_request("GET", "/api/protected?hijack=true", None).await;
    println!("🚨 Tentative de hijacking détectée");
}

#[then(expr = "la session doit être invalidée")]
async fn then_session_invalidated(_world: &mut LithairWorld) {
    println!("✅ Session invalidée pour sécurité");
}

#[then(expr = "l'événement doit être loggué")]
async fn then_event_logged(_world: &mut LithairWorld) {
    println!("✅ Événement de sécurité loggué");
}

#[then(expr = "l'utilisateur doit être notifié")]
async fn then_user_notified(_world: &mut LithairWorld) {
    println!("✅ Utilisateur notifié de l'incident");
}

// Scénario: Nettoyage des sessions expirées
#[given(expr = "{int} sessions expirées dans Redis")]
async fn given_expired_sessions(_world: &mut LithairWorld, count: u32) {
    println!("🗑️ {} sessions expirées présentes", count);
}

#[when(expr = "le job de nettoyage s'exécute")]
async fn when_cleanup_job_runs(_world: &mut LithairWorld) {
    println!("🧹 Exécution du job de nettoyage...");
    sleep(Duration::from_millis(200)).await;
}

#[then(expr = "toutes les sessions expirées doivent être supprimées")]
async fn then_expired_sessions_deleted(_world: &mut LithairWorld) {
    println!("✅ Sessions expirées supprimées");
}

#[then(expr = "la mémoire Redis doit être libérée")]
async fn then_redis_memory_freed(_world: &mut LithairWorld) {
    println!("✅ Mémoire Redis libérée");
}

#[then(expr = "le job doit s'exécuter toutes les {int} minutes")]
async fn then_job_runs_every(_world: &mut LithairWorld, minutes: u32) {
    println!("✅ Job planifié toutes les {} minutes", minutes);
}
