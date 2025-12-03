use cucumber::World;
use cucumber_tests::features::world::LithairWorld;

#[tokio::main]
async fn main() {
    // Exécuter les tests DIRECTS du moteur (sans HTTP)
    // Performance pure : 500K-1M ops/sec
    LithairWorld::cucumber()
        .max_concurrent_scenarios(1) // Un seul scénario à la fois
        .filter_run("features/performance/engine_direct_test.feature", |_, _, _| {
            // Lancer tous les scénarios de test direct
            true
        })
        .await;
}
