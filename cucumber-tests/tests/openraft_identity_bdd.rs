#[path = "../../lithair-core/src/cluster/durable_log/mod.rs"]
#[allow(dead_code)]
mod durable_log;
#[path = "../../lithair-core/src/cluster/peer_transport/mod.rs"]
#[allow(dead_code)]
mod peer_transport;
#[path = "../../lithair-core/tests/support/openraft_processes.rs"]
#[allow(dead_code)]
mod processes;

use cucumber::{given, then, when, World};
use durable_log::{DurableLog, NodeIdentity};
use openraft_memstore::TypeConfig;

#[derive(Debug, Default, World)]
struct IdentityWorld {
    directory: Option<tempfile::TempDir>,
    identity: Option<NodeIdentity>,
    rejected: bool,
    cluster: Option<processes::Cluster>,
}
#[given("a provisioned consensus directory for node one")]
async fn provision(world: &mut IdentityWorld) {
    let directory = tempfile::tempdir().unwrap();
    let identity = NodeIdentity {
        cluster_id: "bdd".into(),
        node_id: 1,
        bootstrap_node: 1,
        voters: [(1, [1; 32]), (2, [2; 32]), (3, [3; 32])].into(),
    };
    let store = DurableLog::<TypeConfig>::create_for_node(directory.path(), identity.clone())
        .await
        .unwrap();
    drop(store);
    world.directory = Some(directory);
    world.identity = Some(identity);
}
#[when("another node tries to recover that directory")]
async fn wrong_node(world: &mut IdentityWorld) {
    let mut wrong = world.identity.clone().unwrap();
    wrong.node_id = 2;
    world.rejected =
        DurableLog::<TypeConfig>::open_for_node(world.directory.as_ref().unwrap().path(), wrong)
            .await
            .is_err();
}
#[then("the identity mismatch is rejected")]
async fn rejected(world: &mut IdentityWorld) {
    assert!(world.rejected);
}
#[then("the original identity can still recover the directory")]
async fn original(world: &mut IdentityWorld) {
    let store = DurableLog::<TypeConfig>::open_for_node(
        world.directory.as_ref().unwrap().path(),
        world.identity.clone().unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(store.node_identity().await.unwrap(), world.identity.clone().unwrap());
}
#[given("three provisioned but uninitialized consensus processes")]
async fn provision_processes(world: &mut IdentityWorld) {
    world.cluster = Some(processes::Cluster::provision().await);
}
#[then("no process has initialized the consensus group")]
async fn empty(world: &mut IdentityWorld) {
    let cluster = world.cluster.as_mut().unwrap();
    cluster.assert_uninitialized().await;
    cluster.restart_uninitialized().await;
}
#[then("followers cannot bootstrap the group")]
async fn followers(world: &mut IdentityWorld) {
    world.cluster.as_mut().unwrap().reject_follower_bootstrap().await;
}
#[when("the designated node explicitly bootstraps the group")]
async fn bootstrap(world: &mut IdentityWorld) {
    world.cluster.as_mut().unwrap().bootstrap().await;
}
#[then("the group accepts writes and survives a cold restart")]
async fn recover(world: &mut IdentityWorld) {
    let cluster = world.cluster.as_mut().unwrap();
    cluster.write(None, "operator-bootstrap").await;
    cluster.reject_wrong_identity().await;
    cluster.cold_restart().await;
    cluster.converge().await;
}
#[then("no process can bootstrap the group again")]
async fn no_rebootstrap(world: &mut IdentityWorld) {
    let cluster = world.cluster.as_mut().unwrap();
    cluster.reject_rebootstrap().await;
    cluster.stop().await;
}
#[tokio::main]
async fn main() {
    if std::env::var_os(processes::CHILD_ENV).is_some() {
        processes::child().await;
        return;
    }
    IdentityWorld::cucumber()
        .fail_on_skipped()
        .run_and_exit("features/core/openraft_identity.feature")
        .await;
}
