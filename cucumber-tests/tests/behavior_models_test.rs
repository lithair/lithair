use cucumber::World;
use cucumber_tests::features::world::LithairWorld;

#[tokio::main]
async fn main() {
    // Behavior specs for the realistic model suite (user uniqueness, order
    // FK notes, invoice Decimal-through-replay, >100KB documents under a
    // retention byte budget). Part of the per-PR CI gate.
    //
    // SERIAL like cucumber_tests: realistic_models_steps configure the
    // engine through process-global env vars (LT_<MODEL>_MEMORY_MAX_MB…);
    // concurrent scenarios would race them (#175).
    LithairWorld::cucumber()
        .max_concurrent_scenarios(Some(1))
        .run_and_exit("features/models/")
        .await;
}
