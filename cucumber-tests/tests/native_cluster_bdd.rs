#[path = "../../lithair-core/tests/support/native_cluster_cases.rs"]
mod cases;
use cucumber::{then, World};
#[derive(Debug, Default, World)]
struct NativeWorld;
#[then("three native nodes preserve concurrent PATCH fields, uniqueness and retry results")]
async fn crud(_: &mut NativeWorld) {
    cases::crud_and_retry().await;
}
#[then("acknowledged native records and request results survive failover and cold restart")]
async fn recovery(_: &mut NativeWorld) {
    cases::failover_and_cold_recovery().await;
}
#[tokio::main]
async fn main() {
    NativeWorld::cucumber()
        .fail_on_skipped()
        .run_and_exit("features/core/native_cluster.feature")
        .await;
}

#[then("a declarative Turso model is refused by native consensus")]
fn turso(_: &mut NativeWorld) {
    assert!(lithair_core::cluster::native::Model::of::<hybrid_storage::Archive>(
        "site.archive",
        "/api/archive"
    )
    .is_err());
}
