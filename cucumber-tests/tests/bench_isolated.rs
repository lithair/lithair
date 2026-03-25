use cucumber::World;
use cucumber_tests::features::world::LithairWorld;

#[tokio::main]
async fn main() {
    // Run BENCH 2 - Pure write
    LithairWorld::cucumber()
        .max_concurrent_scenarios(1)
        .filter_run("features/performance/bench_isolated.feature", |_, _, scenario| {
            // Run BENCH 2 to measure disk write
            scenario.name.contains("BENCH 2")
        })
        .await;
}
