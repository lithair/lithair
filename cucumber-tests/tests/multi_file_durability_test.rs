//! Test runner for multi-file durability tests
//!
//! This test verifies that each data structure has its own file
//! with CRC32 validated for data integrity.

use cucumber::World;
use cucumber_tests::features::world::LithairWorld;

#[tokio::main]
async fn main() {
    LithairWorld::cucumber()
        .max_concurrent_scenarios(1) // Sequential to avoid file conflicts
        .filter_run("features/performance/multi_file_durability.feature", |_, _, _| true)
        .await;
}
