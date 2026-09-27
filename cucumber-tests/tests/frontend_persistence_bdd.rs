#[path = "../../lithair-core/tests/support/frontend_persistence.rs"]
mod cases;
use cucumber::{then, World};
#[derive(Debug, Default, World)]
struct FrontendWorld;
#[then("unchanged frontend loads, concurrent reloads and restarts do not append copies")]
async fn unchanged(_: &mut FrontendWorld) {
    cases::unchanged_loads_reloads_and_updates_do_not_append().await;
}
#[then("legacy frontend history compacts while preserving content, MIME and deletions")]
async fn legacy(_: &mut FrontendWorld) {
    cases::legacy_history_is_compacted_without_losing_deletes_or_mime().await;
}
#[then("frontend updates stay bounded and offline removals survive reopening in isolation")]
async fn changes(_: &mut FrontendWorld) {
    cases::changed_assets_and_removals_stay_bounded_across_restart().await;
}
#[then("corrupt frontend journals are rejected without discarding their evidence")]
async fn corruption(_: &mut FrontendWorld) {
    cases::corrupt_history_is_rejected_without_compacting_away_evidence().await;
}
#[tokio::main]
async fn main() {
    FrontendWorld::cucumber()
        .fail_on_skipped()
        .run_and_exit("features/persistence/frontend.feature")
        .await;
}
