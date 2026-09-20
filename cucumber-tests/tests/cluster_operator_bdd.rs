#[path = "../../lithair-core/src/cluster/durable_log/mod.rs"]
#[allow(dead_code)]
mod durable_log;
#[path = "../../lithair-core/tests/support/operator_files.rs"]
mod fixture;
#[path = "../../lithair-core/src/cluster/peer_transport/mod.rs"]
#[allow(dead_code)]
mod peer_transport;
#[path = "../../lithair-core/tests/support/openraft_processes.rs"]
#[allow(dead_code)]
mod processes;

use cucumber::{given, then, when, World};
use lithair_core::cluster::operator::{run, OperatorCommand};
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Default, World)]
struct OperatorWorld {
    files: Option<fixture::OperatorFiles>,
    report: serde_json::Value,
    cluster: Option<processes::Cluster>,
}
fn contents(path: &Path) -> BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(path)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (entry.file_name().to_str().unwrap().into(), std::fs::read(entry.path()).unwrap())
        })
        .collect()
}
#[given("an operator configuration for three authenticated voters")]
async fn configuration(world: &mut OperatorWorld) {
    world.files = Some(fixture::OperatorFiles::new());
}
#[when("I check the configuration and provision its local store")]
async fn provision(world: &mut OperatorWorld) {
    let files = world.files.as_ref().unwrap();
    let checked = run(OperatorCommand::Check, &files.configs[&1]).await.unwrap();
    assert!(!files.data(1).exists());
    world.report = run(OperatorCommand::Provision, &files.configs[&1]).await.unwrap();
    assert_eq!(checked["plan_sha256"], world.report["plan_sha256"]);
}
#[then("the store retains the configured identity without bootstrapping")]
async fn identity(world: &mut OperatorWorld) {
    let files = world.files.as_ref().unwrap();
    let inspected = run(OperatorCommand::Inspect, &files.configs[&1]).await.unwrap();
    assert_eq!(inspected["node_id"], 1);
    assert_eq!(inspected["plan_sha256"], world.report["plan_sha256"]);
    assert_eq!(inspected["store"]["pristine"], true);
    assert_eq!(inspected["store"]["bootstrap_claimed"], false);
}
#[then("offline inspection leaves the store unchanged")]
async fn unchanged(world: &mut OperatorWorld) {
    let files = world.files.as_ref().unwrap();
    let before = contents(&files.data(1));
    run(OperatorCommand::Inspect, &files.configs[&1]).await.unwrap();
    assert_eq!(contents(&files.data(1)), before);
}
#[given("three uninitialized peers with a shared bootstrap plan")]
async fn peers(world: &mut OperatorWorld) {
    world.cluster = Some(processes::Cluster::provision().await);
}
#[when("one peer is unreachable during bootstrap preflight")]
async fn unavailable(world: &mut OperatorWorld) {
    world.cluster.as_mut().unwrap().reject_unavailable_bootstrap().await;
}
#[then("bootstrap is refused without consuming its permission")]
async fn refused(world: &mut OperatorWorld) {
    world.cluster.as_mut().unwrap().assert_uninitialized().await;
    // The subsequent successful retry verifies the one-shot permission survived.
}
#[when("the peer returns and the operator retries bootstrap")]
async fn retry(world: &mut OperatorWorld) {
    let cluster = world.cluster.as_mut().unwrap();
    cluster.heal();
    cluster.bootstrap().await;
}
#[then("the three nodes accept writes and recover after restart")]
async fn restart(world: &mut OperatorWorld) {
    let cluster = world.cluster.as_mut().unwrap();
    cluster.write(None, "preflight-retry").await;
    cluster.cold_restart().await;
    cluster.converge().await;
    cluster.stop().await;
}
#[tokio::main]
async fn main() {
    if std::env::var_os(processes::CHILD_ENV).is_some() {
        processes::child().await;
        return;
    }
    OperatorWorld::cucumber()
        .fail_on_skipped()
        .run_and_exit("features/core/cluster_operator.feature")
        .await;
}
