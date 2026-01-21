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
    features::world::LithairWorld::cucumber()
        .run_and_exit("features/performance/serialization_modes.feature")
        .await;
}
