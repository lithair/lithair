use cucumber::{then, World};

#[path = "../../lithair-core/tests/support/native_http.rs"]
mod support;

#[derive(Debug, Default, World)]
struct NativeHttpWorld;

#[then("failed POST PUT PATCH and DELETE operations preserve the previous state and stop further writes")]
async fn failed_http(_: &mut NativeHttpWorld) {
    support::failed_http_mutations().await;
}

#[then("HTTP create edit and delete acknowledgements survive reopening without a timer flush")]
async fn reopen(_: &mut NativeHttpWorld) {
    support::acknowledged_mutations_reopen().await;
}

#[then("concurrent HTTP patches retain every acknowledged field after reopening")]
async fn concurrent(_: &mut NativeHttpWorld) {
    support::concurrent_partial_edits().await;
}

#[then("failed replicated and admin mutations neither publish state nor emit notifications")]
async fn failed_apply(_: &mut NativeHttpWorld) {
    support::failed_replicated_mutations().await;
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    NativeHttpWorld::cucumber()
        .run_and_exit("features/core/native_http_commit.feature")
        .await;
}
