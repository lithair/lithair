use cucumber::World;
use cucumber_tests::features::world::LithairWorld;

#[tokio::main]
async fn main() {
    // Exécuter le STRESS TEST 100K avec optimisations
    LithairWorld::cucumber()
        .max_concurrent_scenarios(1) // Un seul scénario à la fois
        .filter_run("features/performance/database_performance.feature", |_, _, scenario| {
            // Lancer le STRESS TEST
            scenario.name.contains("STRESS TEST")
        })
        .await;
}
