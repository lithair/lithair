use cucumber::World;
use cucumber_tests::features::world::LithairWorld;

#[tokio::main]
async fn main() {
    // Exécuter les tests de durabilité
    LithairWorld::cucumber()
        .max_concurrent_scenarios(1) // Un seul scénario à la fois
        .filter_run("features/performance/durability_test.feature", |_, _, _| {
            // Lancer tous les scénarios de durabilité
            true
        })
        .await;
}
