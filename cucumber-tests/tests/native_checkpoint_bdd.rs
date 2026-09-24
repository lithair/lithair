use cucumber::{then, World};
#[path = "../../lithair-core/tests/support/native_checkpoint.rs"]
mod support;
#[derive(Debug, Default, World)]
struct CheckpointWorld;
#[then("SCC recovery applies only increments after the snapshot")]
async fn boundary(_: &mut CheckpointWorld) {
    support::snapshot_then_replay_applies_only_uncovered_increments().await;
}
#[then("the compacted SCC state survives reopening")]
async fn restore(_: &mut CheckpointWorld) {
    support::compacted_scc_snapshot_survives_reopen().await;
}
#[then("native truncation without a checkpoint preserves the journal and fails")]
async fn refuse(_: &mut CheckpointWorld) {
    support::truncation_requires_a_current_checkpoint();
}
#[then(
    "repeated native HTTP compaction preserves updates and deletes and rejects snapshot corruption"
)]
async fn http(_: &mut CheckpointWorld) {
    support::repeated_http_compaction_preserves_updates_and_deletes().await;
}
#[tokio::main]
async fn main() {
    CheckpointWorld::cucumber()
        .run_and_exit("features/core/native_checkpoint.feature")
        .await;
}
