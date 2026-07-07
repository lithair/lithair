use cucumber::World;
use cucumber_tests::features::world::LithairWorld;

#[tokio::main]
async fn main() {
    // Run all in-memory persistence feature scenarios.
    // Performance/cluster features have their own dedicated binaries.
    //
    // SERIAL on purpose: several step modules (retention, event-sourcing)
    // configure the engine through process-global env vars — the documented
    // deploy-time knobs (e.g. LT_<MODEL>_MEMORY_RETENTION) are exactly the
    // feature under test. Concurrent scenarios race those vars: a scenario
    // that set its limit to 10 could assert while a neighbour's limit of
    // 100 was in effect (flaked exactly so when this suite joined the CI
    // gate, #132). The suite executes in seconds; serialization is free.
    LithairWorld::cucumber()
        .max_concurrent_scenarios(Some(1))
        .run_and_exit("features/persistence/")
        .await;
}
