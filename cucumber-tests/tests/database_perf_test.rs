use cucumber::World;
use cucumber_tests::features::world::LithairWorld;

#[tokio::main]
async fn main() {
    // Run the STRESS TEST 100K with optimizations
    LithairWorld::cucumber()
        .max_concurrent_scenarios(1) // One scenario at a time
        .filter_run("features/performance/database_performance.feature", |_, _, scenario| {
            // Run the STRESS TEST
            scenario.name.contains("STRESS TEST")
        })
        .await;
}
