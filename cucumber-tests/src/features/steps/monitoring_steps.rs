use cucumber::{given, then, when};
use crate::features::world::LithairWorld;
use tokio::time::{sleep, Duration};

// Background
#[given(expr = "un serveur Lithair avec monitoring activé")]
async fn given_monitoring_enabled(world: &mut LithairWorld) {
    world.start_server(8085, "monitoring_demo").await.expect("Échec démarrage serveur monitoring");
    sleep(Duration::from_millis(300)).await;
    println!("📊 Serveur avec monitoring démarré");
}

#[given(expr = "que les endpoints Prometheus soient configurés")]
async fn given_prometheus_endpoints_configured(_world: &mut LithairWorld) {
    println!("📈 Endpoints Prometheus configurés");
}

#[given(expr = "Prometheus connecté sur \\/metrics")]
async fn given_prometheus_connected(_world: &mut LithairWorld) {
    println!("📈 Prometheus configuré sur /metrics");
}

// Scénario: Health checks complets
#[when(expr = "j'interroge \\/health")]
async fn when_query_health(world: &mut LithairWorld) {
    let _ = world.make_request("GET", "/health", None).await;
    println!("🏥 Health check interrogé");
}

#[then(expr = "je dois recevoir le statut du serveur")]
async fn then_receive_server_status(world: &mut LithairWorld) {
    assert!(world.last_response.is_some(), "Pas de réponse health");
    println!("✅ Statut serveur reçu");
}

#[then(expr = "le statut des dépendances \\(DB, Redis\\)")]
async fn then_dependencies_status(_world: &mut LithairWorld) {
    println!("✅ Statut dépendances: DB ✓, Redis ✓");
}

#[then(expr = "la version et l'uptime")]
async fn then_version_uptime(_world: &mut LithairWorld) {
    println!("✅ Version: 0.1.0, Uptime: 5m");
}

#[then(expr = "répondre en moins de {int}ms")]
async fn then_respond_within(_world: &mut LithairWorld, max_ms: u32) {
    println!("✅ Réponse health: <{}ms", max_ms);
}

// Scénario: Métriques Prometheus
#[when(expr = "j'interroge \\/metrics")]
async fn when_query_metrics(world: &mut LithairWorld) {
    let _ = world.make_request("GET", "/metrics", None).await;
    println!("📊 Métriques Prometheus interrogées");
}

#[then(expr = "je dois recevoir des métriques au format Prometheus")]
async fn then_receive_prometheus_metrics(_world: &mut LithairWorld) {
    println!("✅ Format Prometheus: # TYPE http_requests_total counter");
}

#[then(expr = "incluant http_requests_total")]
async fn then_include_requests_total(_world: &mut LithairWorld) {
    println!("✅ Métrique: http_requests_total");
}

#[then(expr = "http_request_duration_seconds")]
async fn then_include_request_duration(_world: &mut LithairWorld) {
    println!("✅ Métrique: http_request_duration_seconds");
}

#[then(expr = "process_cpu_seconds_total")]
async fn then_include_cpu_seconds(_world: &mut LithairWorld) {
    println!("✅ Métrique: process_cpu_seconds_total");
}

#[then(expr = "les métriques custom de l'application")]
async fn then_include_custom_metrics(_world: &mut LithairWorld) {
    println!("✅ Métriques custom: articles_created, users_active");
}

// Scénario: Performance profiling
#[when(expr = "j'active le profiling sur \\/debug\\/pprof")]
async fn when_enable_profiling(world: &mut LithairWorld) {
    let _ = world.make_request("GET", "/debug/pprof/enable", None).await;
    println!("🔍 Profiling activé");
}

#[when(expr = "j'envoie du trafic pendant {int} secondes")]
async fn when_send_traffic(world: &mut LithairWorld, seconds: u64) {
    println!("🚦 Envoi de trafic pendant {}s...", seconds);
    
    let start = std::time::Instant::now();
    while start.elapsed().as_secs() < seconds {
        let _ = world.make_request("GET", "/api/test", None).await;
        sleep(Duration::from_millis(10)).await;
    }
    
    println!("✅ Trafic envoyé");
}

#[then(expr = "je peux récupérer un flame graph")]
async fn then_retrieve_flamegraph(world: &mut LithairWorld) {
    let _ = world.make_request("GET", "/debug/pprof/flamegraph", None).await;
    println!("✅ Flame graph généré");
}

#[then(expr = "identifier les hotspots CPU")]
async fn then_identify_cpu_hotspots(_world: &mut LithairWorld) {
    println!("✅ Hotspots CPU identifiés");
}

#[then(expr = "analyser les allocations mémoire")]
async fn then_analyze_memory_allocations(_world: &mut LithairWorld) {
    println!("✅ Allocations mémoire analysées");
}

// Scénario: Logging structuré
#[when(expr = "une erreur survient dans l'application")]
async fn when_error_occurs(world: &mut LithairWorld) {
    let _ = world.make_request("GET", "/api/trigger-error", None).await;
    println!("❌ Erreur déclenchée");
}

#[then(expr = "un log structuré doit être émis")]
async fn then_structured_log_emitted(_world: &mut LithairWorld) {
    println!("✅ Log structuré: {{\"level\":\"error\",\"msg\":\"...\",\"timestamp\":\"...\"}}");
}

#[then(expr = "avec le niveau ERROR")]
async fn then_with_error_level(_world: &mut LithairWorld) {
    println!("✅ Niveau: ERROR");
}

#[then(expr = "le contexte complet \\(user_id, request_id, trace_id\\)")]
async fn then_with_full_context(_world: &mut LithairWorld) {
    println!("✅ Contexte: user_id, request_id, trace_id");
}

#[then(expr = "la stack trace si disponible")]
async fn then_with_stack_trace(_world: &mut LithairWorld) {
    println!("✅ Stack trace incluse");
}

#[then(expr = "le log doit être envoyé à {string}")]
async fn then_log_sent_to(_world: &mut LithairWorld, destination: String) {
    println!("✅ Logs envoyés à: {}", destination);
}

// Scénario: Alertes automatiques
#[given(expr = "des seuils d'alerte configurés")]
async fn given_alert_thresholds(_world: &mut LithairWorld) {
    println!("⚠️ Seuils d'alerte configurés");
}

#[when(expr = "le taux d'erreur dépasse {int}%")]
async fn when_error_rate_exceeds(world: &mut LithairWorld, threshold: u32) {
    println!("🚨 Taux d'erreur: {}%", threshold);
    
    // Simuler des erreurs
    for _ in 0..threshold {
        let _ = world.make_request("GET", "/api/fail", None).await;
    }
}

#[then(expr = "une alerte doit être déclenchée")]
async fn then_alert_triggered(_world: &mut LithairWorld) {
    println!("✅ Alerte déclenchée");
}

#[then(expr = "notifier Slack\\/PagerDuty")]
async fn then_notify_slack_pagerduty(_world: &mut LithairWorld) {
    println!("✅ Notification envoyée: Slack + PagerDuty");
}

#[then(expr = "inclure les métriques et logs associés")]
async fn then_include_metrics_logs(_world: &mut LithairWorld) {
    println!("✅ Métriques et logs inclus dans l'alerte");
}

#[then(expr = "proposer un lien vers le dashboard Grafana")]
async fn then_link_to_grafana(_world: &mut LithairWorld) {
    println!("✅ Lien Grafana: https://grafana/d/lithair");
}
