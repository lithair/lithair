use cucumber::World;
use cucumber_tests::features::steps::sessions_steps::SessionsWorld;

#[tokio::main]
async fn main() {
    // Browser session journey (login cookie → gated route → logout → 401).
    // Own world, like scaffolding_test. Part of the per-PR CI gate.
    SessionsWorld::cucumber().run_and_exit("features/core/sessions.feature").await;
}
