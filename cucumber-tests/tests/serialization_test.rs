//! Cucumber tests for dual-mode serialization (JSON + rkyv)

use cucumber::World;

mod features {
    pub mod steps {
        #[allow(unused_imports)]
        pub use cucumber_tests::features::steps::serialization_steps::*;
    }
    pub mod world {
        pub use cucumber_tests::features::world::*;
    }
}

#[tokio::main]
async fn main() {
    // Run serialization tests, filtering out scenarios that need
    // a running HTTP server or are benchmarks.
    features::world::LithairWorld::cucumber()
        .filter_run(
            "features/performance/serialization_modes.feature",
            |_feature, _rule, scenario| {
                let skip_tags: &[&str] = &["http", "benchmark"];
                !scenario.tags.iter().any(|t| skip_tags.contains(&t.as_str()))
            },
        )
        .await;
}
