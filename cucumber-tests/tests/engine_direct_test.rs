use cucumber::World;
use cucumber_tests::features::world::LithairWorld;

#[tokio::main]
async fn main() {
    // Run DIRECT engine tests (without HTTP)
    // Pure performance: 500K-1M ops/sec
    LithairWorld::cucumber()
        .max_concurrent_scenarios(1) // One scenario at a time
        .filter_run("features/performance/engine_direct_test.feature", |_, _, _| {
            // Run all direct test scenarios
            true
        })
        .await;
}
