use cucumber::World;
use cucumber_tests::features::world::LithairWorld;

#[tokio::main]
async fn main() {
    // Run the 1M STRESS TESTS with full verification
    LithairWorld::cucumber()
        .max_concurrent_scenarios(1) // One scenario at a time to avoid port conflicts
        .filter_run("features/performance/stress_1m_test.feature", |_, _, _| {
            // Run all stress test scenarios
            true
        })
        .await;
}
