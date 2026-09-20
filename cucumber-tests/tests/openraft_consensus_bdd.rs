#[path = "../../lithair-core/src/cluster/durable_log/mod.rs"]
#[allow(dead_code)]
mod durable_log;
#[path = "../../lithair-core/src/cluster/peer_transport/mod.rs"]
#[allow(dead_code)]
mod peer_transport;
#[path = "../../lithair-core/tests/support/openraft_processes.rs"]
#[allow(dead_code)] // Identity-only scenarios use the other shared fixture methods.
mod processes;

use cucumber::{given, then, when, World};

#[derive(Debug, Default, World)]
struct ConsensusWorld {
    cluster: Option<processes::Cluster>,
}

#[given("three authenticated OpenRaft processes with durable logs")]
async fn start(world: &mut ConsensusWorld) {
    world.cluster = Some(processes::Cluster::start().await);
}
#[when("the leader acknowledges a command and is killed")]
async fn crash(world: &mut ConsensusWorld) {
    world.cluster.as_mut().unwrap().crash_leader().await;
}
#[then("the remaining majority accepts writes")]
async fn write(world: &mut ConsensusWorld) {
    let cluster = world.cluster.as_mut().unwrap();
    cluster.write(cluster.affected, "majority-write").await;
}
#[when("the old leader restarts from its existing log")]
#[when("the offline follower restarts from its existing log")]
async fn restart(world: &mut ConsensusWorld) {
    world.cluster.as_mut().unwrap().restart().await;
}
#[when("all three processes are killed after an acknowledged command and restarted")]
async fn cold_restart(world: &mut ConsensusWorld) {
    let cluster = world.cluster.as_mut().unwrap();
    cluster.write(None, "before-cold-restart").await;
    cluster.cold_restart().await;
}
#[then("all three processes recover the acknowledged commands")]
async fn converge(world: &mut ConsensusWorld) {
    let cluster = world.cluster.as_mut().unwrap();
    cluster.converge().await;
    cluster.stop().await;
}
#[when("the leader is isolated by cutting its TCP connections")]
async fn partition(world: &mut ConsensusWorld) {
    world.cluster.as_mut().unwrap().partition().await;
}
#[then("the isolated process rejects writes and read barriers")]
async fn reject(world: &mut ConsensusWorld) {
    world.cluster.as_mut().unwrap().minority_rejects().await;
}
#[when("the TCP partition is healed")]
async fn heal(world: &mut ConsensusWorld) {
    world.cluster.as_mut().unwrap().heal();
}

#[tokio::main]
async fn main() {
    if std::env::var_os(processes::CHILD_ENV).is_some() {
        processes::child().await;
        return;
    }
    ConsensusWorld::cucumber()
        .fail_on_skipped()
        .run_and_exit("features/core/openraft_consensus.feature")
        .await;
}

#[when("a follower misses writes until the leader snapshots and compacts them")]
async fn snapshot_catch_up(world: &mut ConsensusWorld) {
    world.cluster.as_mut().unwrap().prepare_snapshot_catch_up().await;
}
#[then("the returning follower installs a snapshot over TLS")]
async fn installed(world: &mut ConsensusWorld) {
    let cluster = world.cluster.as_mut().unwrap();
    cluster.converge().await;
    cluster.assert_snapshot_installed().await;
}
