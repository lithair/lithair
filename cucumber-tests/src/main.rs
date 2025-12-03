use cucumber::{World, cli};
mod features;

#[tokio::main]
async fn main() {
    // Exécuter les tests Cucumber
    features::LithairWorld::cucumber()
        .with_cli::<()>(cli::Opts::parsed())
        .run_and_exit("features/")
        .await;
}
