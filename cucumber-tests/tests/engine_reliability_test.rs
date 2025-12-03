use cucumber::World;
use cucumber_tests::features::world::LithairWorld;

#[tokio::main]
async fn main() {
    // Exécuter les tests de FIABILITÉ du moteur
    // Recovery, Corruption, Concurrence, Durabilité
    LithairWorld::cucumber()
        .max_concurrent_scenarios(1) // Un seul scénario à la fois pour isolation
        .filter_run("features/performance/engine_reliability_test.feature", |_, _, _| {
            // Lancer tous les scénarios de test de fiabilité
            true
        })
        .await;
}
