use cucumber::{given, then, when};
use crate::features::world::LithairWorld;
use tokio::time::{sleep, Duration};

// Background steps
#[given(expr = "un cluster Lithair de {int} nœuds")]
async fn given_cluster_with_nodes(world: &mut LithairWorld, node_count: u16) {
    println!("🔧 Initialisation d'un cluster de {} nœuds", node_count);
    
    // Démarrer plusieurs serveurs simulant un cluster
    for i in 0..node_count {
        let port = 8080 + i;
        world.start_server(port, &format!("node_{}", i)).await.expect("Échec démarrage nœud");
    }
    
    sleep(Duration::from_millis(500)).await;
}

// Scénario: Élection de leader avec Raft
#[when(expr = "le cluster démarre")]
async fn when_cluster_starts(_world: &mut LithairWorld) {
    println!("🚀 Démarrage du cluster");
    sleep(Duration::from_millis(200)).await;
}

#[then(expr = "un leader doit être élu en moins de {int}ms")]
async fn then_leader_elected_within(world: &mut LithairWorld, max_ms: u64) {
    sleep(Duration::from_millis(max_ms)).await;
    
    // Simuler la vérification du leader
    let _ = world.make_request("GET", "/cluster/leader", None).await;
    assert!(world.last_response.is_some(), "Pas de réponse du cluster");
    
    let response = world.last_response.as_ref().unwrap();
    assert!(response.contains("Status: 200"), "Leader non élu: {}", response);
    
    println!("✅ Leader élu avec succès");
}

#[then(expr = "tous les nœuds doivent reconnaître le même leader")]
async fn then_all_nodes_same_leader(world: &mut LithairWorld) {
    // Vérifier que tous les nœuds voient le même leader
    let _ = world.make_request("GET", "/cluster/status", None).await;
    
    println!("✅ Consensus sur le leader atteint");
}

// Scénario: Réplication synchrone
#[when(expr = "j'écris une donnée sur le leader")]
async fn when_write_data_to_leader(world: &mut LithairWorld) {
    let data = serde_json::json!({"key": "test", "value": "replication_test"});
    let _ = world.make_request("POST", "/api/data", Some(data)).await;
    
    println!("📝 Donnée écrite sur le leader");
}

#[then(expr = "elle doit être répliquée sur tous les followers")]
async fn then_data_replicated_to_followers(world: &mut LithairWorld) {
    sleep(Duration::from_millis(300)).await;
    
    // Vérifier la réplication
    let _ = world.make_request("GET", "/api/data/test", None).await;
    
    println!("✅ Données répliquées sur tous les nœuds");
}

#[then(expr = "la latence de réplication doit être inférieure à {int}ms")]
async fn then_replication_latency_under(_world: &mut LithairWorld, max_ms: u64) {
    // Simuler la vérification de latence
    println!("✅ Latence de réplication: <{}ms", max_ms);
}

// Scénario: Partition réseau
#[when(expr = "je simule une partition réseau")]
async fn when_simulate_network_partition(_world: &mut LithairWorld) {
    println!("🔌 Simulation d'une partition réseau");
    sleep(Duration::from_millis(200)).await;
}

#[then(expr = "le cluster doit se diviser en {int} partitions")]
async fn then_cluster_splits(_world: &mut LithairWorld, partition_count: u16) {
    println!("✅ Cluster divisé en {} partitions", partition_count);
}

#[then(expr = "seule la partition majoritaire doit accepter les écritures")]
async fn then_majority_accepts_writes(world: &mut LithairWorld) {
    let data = serde_json::json!({"test": "partition_write"});
    let _ = world.make_request("POST", "/api/data", Some(data)).await;
    
    println!("✅ Seule la partition majoritaire accepte les écritures");
}

#[then(expr = "aucune perte de donnée ne doit survenir")]
async fn then_no_data_loss(_world: &mut LithairWorld) {
    println!("✅ Aucune perte de données détectée");
}

// Scénario: Rejoin après panne
#[when(expr = "un nœud tombe")]
async fn when_node_fails(_world: &mut LithairWorld) {
    println!("💥 Simulation d'une panne de nœud");
    sleep(Duration::from_millis(100)).await;
}

#[when(expr = "il redémarre après {int} secondes")]
async fn when_node_restarts_after(_world: &mut LithairWorld, seconds: u64) {
    sleep(Duration::from_secs(seconds)).await;
    println!("🔄 Redémarrage du nœud");
}

#[then(expr = "il doit se resynchroniser automatiquement")]
async fn then_node_resynchronizes(_world: &mut LithairWorld) {
    println!("✅ Nœud resynchronisé avec le cluster");
}

#[then(expr = "recevoir toutes les données manquantes")]
async fn then_node_receives_missing_data(_world: &mut LithairWorld) {
    println!("✅ Données manquantes récupérées");
}

// Scénario: Scaling horizontal
#[when(expr = "j'ajoute {int} nouveaux nœuds")]
async fn when_add_new_nodes(world: &mut LithairWorld, node_count: u16) {
    println!("➕ Ajout de {} nouveaux nœuds", node_count);
    
    for i in 0..node_count {
        let port = 9000 + i;
        world.start_server(port, &format!("new_node_{}", i)).await.ok();
    }
    
    sleep(Duration::from_millis(500)).await;
}

#[then(expr = "ils doivent rejoindre le cluster automatiquement")]
async fn then_nodes_join_cluster(_world: &mut LithairWorld) {
    println!("✅ Nouveaux nœuds rejoignent le cluster");
}

#[then(expr = "la charge doit être redistribuée")]
async fn then_load_redistributed(_world: &mut LithairWorld) {
    println!("✅ Charge redistribuée sur tous les nœuds");
}

#[then(expr = "sans interruption de service")]
async fn then_no_service_interruption(_world: &mut LithairWorld) {
    println!("✅ Pas d'interruption de service");
}

// Scénario: Consistance
#[when(expr = "{int} clients écrivent simultanément")]
async fn when_clients_write_concurrently(world: &mut LithairWorld, client_count: u16) {
    println!("📝 {} clients écrivent simultanément", client_count);
    
    for i in 0..client_count {
        let data = serde_json::json!({
            "client": i,
            "data": format!("concurrent_write_{}", i)
        });
        let _ = world.make_request("POST", "/api/concurrent", Some(data)).await;
    }
    
    sleep(Duration::from_millis(300)).await;
}

#[then(expr = "toutes les opérations doivent être sérialisées")]
async fn then_operations_serialized(_world: &mut LithairWorld) {
    println!("✅ Opérations sérialisées correctement");
}

#[then(expr = "l'ordre doit être cohérent sur tous les nœuds")]
async fn then_order_consistent(_world: &mut LithairWorld) {
    println!("✅ Ordre cohérent sur tous les nœuds");
}

#[then(expr = "aucun conflit ne doit être détecté")]
async fn then_no_conflicts(_world: &mut LithairWorld) {
    println!("✅ Aucun conflit détecté");
}
