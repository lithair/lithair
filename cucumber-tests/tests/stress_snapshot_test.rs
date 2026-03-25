//! Test runner for snapshot stress tests
//!
//! This test verifies snapshot performance at large scale
//! with 10K, 100K, 500K and 1M events.

use cucumber::World;
use cucumber_tests::features::world::LithairWorld;

#[tokio::main]
async fn main() {
    // By default, only run quick tests (@quick)
    // To run full tests, use: cargo test --test stress_snapshot_test -- --tags @1m
    LithairWorld::cucumber()
        .max_concurrent_scenarios(1) // Sequential to avoid file conflicts
        .filter_run("features/performance/stress_snapshot_1m.feature", |_, _, sc| {
            // By default, only run @quick tests
            // Other tests can be run manually
            sc.tags.iter().any(|t| t == "quick")
        })
        .await;
}
