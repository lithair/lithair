use cucumber::World;
use cucumber_tests::features::world::LithairWorld;

#[tokio::main]
async fn main() {
    // Exécuter BENCH 2 - Écriture pure
    LithairWorld::cucumber()
        .max_concurrent_scenarios(1)
        .filter_run("features/performance/bench_isolated.feature", |_, _, scenario| {
            // Lancer BENCH 2 pour mesurer écriture disque
            scenario.name.contains("BENCH 2")
        })
        .await;
}
