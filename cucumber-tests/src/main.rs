use cucumber::{cli, World};
mod features;

#[tokio::main]
async fn main() {
    // Run Cucumber tests with concurrency=1.
    // Cluster scenarios spawn external lithair-cluster-node processes and
    // allocate ports dynamically. Running them concurrently causes port
    // collisions, zombie processes, and resource exhaustion.
    features::LithairWorld::cucumber()
        .max_concurrent_scenarios(Some(1))
        .with_cli::<()>(cli::Opts::parsed())
        .run_and_exit("features/")
        .await;
}
