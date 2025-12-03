//! Test runner pour le stress test 100K snapshots
//!
//! Lance le scénario @medium (100K événements)

use cucumber::World;
use cucumber_tests::features::world::LithairWorld;

#[tokio::main]
async fn main() {
    LithairWorld::cucumber()
        .max_concurrent_scenarios(1)
        .filter_run("features/performance/stress_snapshot_1m.feature", |_, _, sc| {
            sc.tags.iter().any(|t| t == "medium")
        })
        .await;
}
