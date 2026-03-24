//! Test runner for snapshot durability tests
//!
//! This test verifies that snapshots work correctly
//! to accelerate recovery after restart.

use cucumber::World;
use cucumber_tests::features::world::LithairWorld;

#[tokio::main]
async fn main() {
    LithairWorld::cucumber()
        .max_concurrent_scenarios(1) // Sequential to avoid file conflicts
        .filter_run("features/performance/snapshot_durability.feature", |_, _, _| true)
        .await;
}
