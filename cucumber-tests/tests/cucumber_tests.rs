use cucumber::World;
use cucumber_tests::features::world::LithairWorld;

#[tokio::main]
async fn main() {
    // Run all in-memory persistence feature scenarios.
    // Performance/cluster features have their own dedicated binaries.
    LithairWorld::cucumber().run_and_exit("features/persistence/").await;
}
