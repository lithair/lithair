use cucumber::{cli, World};
mod features;

#[tokio::main]
async fn main() {
    // Run Cucumber tests
    features::LithairWorld::cucumber()
        .with_cli::<()>(cli::Opts::parsed())
        .run_and_exit("features/")
        .await;
}
