use cucumber::World;
use cucumber_tests::features::steps::scaffolding_steps::ScaffoldingWorld;

#[tokio::main]
async fn main() {
    ScaffoldingWorld::cucumber()
        .run_and_exit("features/core/scaffolding.feature")
        .await;
}
