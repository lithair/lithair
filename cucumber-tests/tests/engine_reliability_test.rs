use cucumber::World;
use cucumber_tests::features::world::LithairWorld;

#[tokio::main]
async fn main() {
    // Run engine RELIABILITY tests
    // Recovery, Corruption, Concurrency, Durability
    LithairWorld::cucumber()
        .max_concurrent_scenarios(1) // One scenario at a time for isolation
        .filter_run("features/performance/engine_reliability_test.feature", |_, _, _| {
            // Run all reliability test scenarios
            true
        })
        .await;
}
