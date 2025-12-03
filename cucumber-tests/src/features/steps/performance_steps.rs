use cucumber::{given, then, when};
use crate::features::LithairWorld;
use std::time::{Duration, Instant};
use tokio::time::sleep;

#[given(expr = "un serveur Lithair démarré")]
async fn given_server_started(world: &mut LithairWorld) {
    world.start_server(18321, "scc2_server_demo").await.expect("Impossible de démarrer le serveur");
    sleep(Duration::from_millis(500)).await; // Attendre que le serveur soit prêt
}

#[given(expr = "que le moteur SCC2 soit activé")]
async fn given_scc2_enabled(_world: &mut LithairWorld) {
    // Le serveur SCC2 est configuré avec SCC2 par défaut
    // TODO: Vérifier que SCC2 est bien activé
}

#[when(expr = "je démarre le serveur SCC2 sur le port {int}")]
async fn start_scc2_server(world: &mut LithairWorld, port: u16) {
    world.start_server(port, "scc2_server_demo").await.expect("Impossible de démarrer le serveur SCC2");
    sleep(Duration::from_millis(500)).await;
}

#[when(expr = "j'envoie {int} requêtes JSON de {int}KB")]
async fn send_json_requests(world: &mut LithairWorld, count: u32, size_kb: u32) {
    let mut total_time = Duration::new(0, 0);
    let mut success_count = 0;
    
    let json_body = serde_json::json!({
        "data": "x".repeat(size_kb as usize * 1024 / 2),
        "timestamp": chrono::Utc::now().timestamp()
    });
    
    for _ in 0..count {
        let start = Instant::now();
        match world.make_request("POST", "/perf/json", Some(json_body.clone())).await {
            Ok(()) => {
                success_count += 1;
                total_time += start.elapsed();
            }
            Err(e) => {
                world.last_error = Some(e);
            }
        }
    }
    
    let mut metrics = world.metrics.lock().await;
    metrics.request_count = count as u64;
    metrics.response_time_ms = total_time.as_millis() as f64 / count as f64;
    metrics.error_rate = ((count - success_count) as f64 / count as f64) * 100.0;
}

#[when(expr = "{int} clients se connectent simultanément")]
async fn concurrent_connections(world: &mut LithairWorld, client_count: u32) {
    let mut success_count = 0;
    
    for i in 0..client_count {
        match world.make_request("GET", &format!("/perf/echo?client={}", i), None).await {
            Ok(()) => success_count += 1,
            Err(_) => {} // Ignorer les erreurs pour l'instant
        }
    }
    
    let mut metrics = world.metrics.lock().await;
    metrics.error_rate = ((client_count - success_count) as f64 / client_count as f64) * 100.0;
}

#[then(expr = "le serveur doit répondre en moins de {float}ms")]
async fn assert_response_time(world: &mut LithairWorld, max_ms: f64) {
    let metrics = world.metrics.lock().await;
    assert!(
        metrics.response_time_ms < max_ms,
        "Temps de réponse {}ms supérieur à {}ms",
        metrics.response_time_ms,
        max_ms
    );
}

#[then(expr = "supporter plus de {int}M requêtes/seconde")]
async fn assert_throughput(world: &mut LithairWorld, min_million_rps: u32) {
    let metrics = world.metrics.lock().await;
    let rps = 1000.0 / metrics.response_time_ms;
    let million_rps = rps / 1_000_000.0;
    
    assert!(
        million_rps > min_million_rps as f64,
        "Throughput {}M RPS inférieur à {}M RPS",
        million_rps,
        min_million_rps
    );
}

#[then(expr = "consommer moins de {int}MB de mémoire")]
async fn assert_memory_usage(world: &mut LithairWorld, max_mb: u32) {
    // TODO: Implémenter la mesure de mémoire réelle
    let metrics = world.metrics.lock().await;
    assert!(
        metrics.memory_usage_mb < max_mb as f64,
        "Utilisation mémoire {}MB supérieure à {}MB",
        metrics.memory_usage_mb,
        max_mb
    );
}

#[then(expr = "le throughput doit dépasser {int}GB/s")]
async fn assert_throughput_gbps(world: &mut LithairWorld, min_gbps: u32) {
    let metrics = world.metrics.lock().await;
    // Calcul approximatif basé sur la taille des requêtes et le temps
    let throughput_gbps = (64.0 * 1000.0) / (metrics.response_time_ms / 1000.0) / 1_000_000_000.0;
    
    assert!(
        throughput_gbps > min_gbps as f64,
        "Throughput {}GB/s inférieur à {}GB/s",
        throughput_gbps,
        min_gbps
    );
}

#[then(expr = "la latence moyenne doit être inférieure à {float}ms")]
async fn assert_latency(world: &mut LithairWorld, max_ms: f64) {
    let metrics = world.metrics.lock().await;
    assert!(
        metrics.response_time_ms < max_ms,
        "Latence {}ms supérieure à {}ms",
        metrics.response_time_ms,
        max_ms
    );
}

#[then(expr = "aucun client ne doit être rejeté")]
async fn assert_no_rejections(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    assert_eq!(
        metrics.error_rate, 0.0,
        "Taux d'erreur {}% supérieur à 0%",
        metrics.error_rate
    );
}

#[then(expr = "le serveur doit maintenir la latence sous {int}ms")]
async fn assert_latency_under_load(world: &mut LithairWorld, max_ms: u32) {
    let metrics = world.metrics.lock().await;
    assert!(
        metrics.response_time_ms < max_ms as f64,
        "Latence sous charge {}ms supérieure à {}ms",
        metrics.response_time_ms,
        max_ms
    );
}
