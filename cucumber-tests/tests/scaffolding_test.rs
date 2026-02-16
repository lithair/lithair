use cucumber::World;
use cucumber_tests::features::steps::scaffolding_steps::ScaffoldingWorld;

#[tokio::main]
async fn main() {
    ScaffoldingWorld::cucumber()
        .run_and_exit("src/features/core/scaffolding.feature")
        .await;
}
