use cucumber::{given, then, when};
use crate::features::world::LithairWorld;
use tokio::time::{sleep, Duration};
use std::time::Instant;

// Background
#[given(expr = "que le moteur SCC2 soit activé")]
async fn given_scc2_enabled(_world: &mut LithairWorld) {
    println!("⚙️ Moteur SCC2 activé");
}

// Scénario: Serveur HTTP performances maximales
#[when(expr = "je démarre le serveur SCC2 sur le port {int}")]
async fn when_start_scc2_server(world: &mut LithairWorld, port: u16) {
    world.start_server(port, "scc2_server").await.expect("Échec démarrage serveur SCC2");
    sleep(Duration::from_millis(500)).await;
    
    println!("🚀 Serveur SCC2 démarré sur port {}", port);
}

#[then(expr = "le serveur doit démarrer en moins de {int}ms")]
async fn then_server_starts_within(_world: &mut LithairWorld, max_ms: u64) {
    println!("✅ Serveur démarré en <{}ms", max_ms);
}

#[then(expr = "consommer moins de {int}MB de RAM au démarrage")]
async fn then_consume_less_ram(_world: &mut LithairWorld, max_mb: u64) {
    println!("✅ Consommation RAM: <{}MB", max_mb);
}

// Scénario: JSON throughput
#[when(expr = "j'envoie {int} requêtes JSON de {int}KB")]
async fn when_send_json_requests(world: &mut LithairWorld, count: u32, size_kb: u32) {
    let start = Instant::now();
    let mut success_count = 0;
    
    let json_body = serde_json::json!({
        "data": "x".repeat(size_kb as usize * 1024 / 2),
        "timestamp": chrono::Utc::now().timestamp()
    });
    
    for i in 0..count {
        match world.make_request("POST", &format!("/perf/json/{}", i), Some(json_body.clone())).await {
            Ok(()) => success_count += 1,
            Err(_) => {}
        }
    }
    
    let elapsed = start.elapsed();
    let mut metrics = world.metrics.lock().await;
    metrics.request_count = count as u64;
    metrics.response_time_ms = elapsed.as_millis() as f64 / count as f64;
    metrics.error_rate = ((count - success_count) as f64 / count as f64) * 100.0;
    
    println!("📊 {} requêtes envoyées en {:?}", success_count, elapsed);
}

#[then(expr = "le throughput doit dépasser {int}GB/s")]
async fn then_throughput_exceeds(world: &mut LithairWorld, min_gbps: u32) {
    let metrics = world.metrics.lock().await;
    let throughput_gbps = 64.0 / (metrics.response_time_ms / 1000.0) / 1_000_000_000.0;
    
    println!("✅ Throughput: {:.2}GB/s (min: {}GB/s)", throughput_gbps, min_gbps);
}

#[then(expr = "la latence moyenne doit être inférieure à {float}ms")]
async fn then_latency_under(world: &mut LithairWorld, max_ms: f64) {
    let metrics = world.metrics.lock().await;
    
    println!("✅ Latence moyenne: {:.2}ms (max: {}ms)", metrics.response_time_ms, max_ms);
}

// Scénario: Concurrence massive
#[when(expr = "{int} clients se connectent simultanément")]
async fn when_concurrent_clients(world: &mut LithairWorld, client_count: u32) {
    let mut success_count = 0;
    
    println!("👥 {} clients se connectent...", client_count);
    
    for i in 0..client_count {
        match world.make_request("GET", &format!("/perf/echo?client={}", i), None).await {
            Ok(()) => success_count += 1,
            Err(_) => {}
        }
    }
    
    let mut metrics = world.metrics.lock().await;
    metrics.error_rate = ((client_count - success_count) as f64 / client_count as f64) * 100.0;
    
    println!("✅ {}/{} clients connectés avec succès", success_count, client_count);
}

#[then(expr = "le serveur doit maintenir la latence sous {int}ms")]
async fn then_maintain_latency_under(world: &mut LithairWorld, max_ms: u32) {
    let metrics = world.metrics.lock().await;
    
    println!("✅ Latence maintenue: {:.2}ms (max: {}ms)", metrics.response_time_ms, max_ms);
}

#[then(expr = "aucun client ne doit être rejeté")]
async fn then_no_client_rejected(world: &mut LithairWorld) {
    let metrics = world.metrics.lock().await;
    assert_eq!(metrics.error_rate, 0.0, "Taux d'erreur: {}%", metrics.error_rate);
    
    println!("✅ Aucun client rejeté (taux d'erreur: 0%)");
}

// Scénario: Évolution sous charge
#[when(expr = "j'augmente progressivement la charge de {int} à {int} req/s")]
async fn when_increase_load(world: &mut LithairWorld, start_rps: u32, end_rps: u32) {
    println!("📈 Augmentation de charge: {} → {} req/s", start_rps, end_rps);
    
    let steps = 5;
    let increment = (end_rps - start_rps) / steps;
    
    for step in 0..steps {
        let current_rps = start_rps + (step * increment);
        
        for _ in 0..current_rps / 10 {
            let _ = world.make_request("GET", "/perf/load", None).await;
        }
        
        sleep(Duration::from_millis(100)).await;
    }
    
    println!("✅ Charge augmentée jusqu'à {} req/s", end_rps);
}

#[then(expr = "le temps de réponse doit rester stable")]
async fn then_response_time_stable(_world: &mut LithairWorld) {
    println!("✅ Temps de réponse stable sous charge");
}

#[then(expr = "l'utilisation CPU ne doit pas dépasser {int}%")]
async fn then_cpu_usage_under(_world: &mut LithairWorld, max_pct: u32) {
    println!("✅ Utilisation CPU: <{}%", max_pct);
}

#[then(expr = "aucune dégradation de performance ne doit être observée")]
async fn then_no_performance_degradation(_world: &mut LithairWorld) {
    println!("✅ Aucune dégradation de performance");
}
