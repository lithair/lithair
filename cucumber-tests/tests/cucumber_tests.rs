use cucumber::World;
use cucumber_tests::features::world::LithairWorld;

#[tokio::main]
async fn main() {
    // Exécuter les tests Cucumber avec la nouvelle API
    LithairWorld::cucumber()
        .run_and_exit("features/")
        .await;
}
