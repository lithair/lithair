use cucumber::World;
use cucumber_tests::features::world::LithairWorld;

#[tokio::main]
async fn main() {
    LithairWorld::cucumber()
        .fail_on_skipped()
        .with_default_cli()
        .run("features/performance/cluster_stress_multi_model.feature")
        .await;
}
