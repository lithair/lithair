use cucumber::World;
use cucumber_tests::features::world::LithairWorld;

#[tokio::main]
async fn main() {
    // Run durability tests
    LithairWorld::cucumber()
        .max_concurrent_scenarios(1) // One scenario at a time
        .filter_run("features/performance/durability_test.feature", |_, _, _| {
            // Run all durability scenarios
            true
        })
        .await;
}
