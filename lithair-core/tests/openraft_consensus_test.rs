#![cfg(all(feature = "cluster", feature = "tls"))]

#[path = "../src/cluster/durable_log/mod.rs"]
#[allow(dead_code)]
mod durable_log;
#[path = "../src/cluster/peer_transport/mod.rs"]
#[allow(dead_code)]
mod peer_transport;

#[path = "support/openraft_processes.rs"]
mod processes;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cluster_child() {
    if std::env::var_os(processes::CHILD_ENV).is_some() {
        processes::child().await;
    }
}

#[tokio::test]
async fn acknowledged_writes_survive_leader_death_and_return() {
    let mut cluster = processes::Cluster::start().await;
    cluster.crash_leader().await;
    cluster.write(cluster.affected, "after-crash").await;
    cluster.restart().await;
    cluster.converge().await;
    // Repeat with the new leader to catch assumptions about the bootstrap node.
    cluster.crash_leader().await;
    cluster.write(cluster.affected, "second-election").await;
    cluster.restart().await;
    cluster.converge().await;
    cluster.cold_restart().await;
    cluster.write(None, "after-cold-restart").await;
    cluster.converge().await;
    cluster.stop().await;
}

#[tokio::test]
async fn minority_partition_rejects_writes_and_read_barriers() {
    let mut cluster = processes::Cluster::start().await;
    cluster.partition().await;
    cluster.minority_rejects().await;
    cluster.write(cluster.affected, "majority-write").await;
    cluster.heal();
    cluster.converge().await;
    cluster.stop().await;
}

#[path = "support/openraft_transport_cases.rs"]
mod transport_cases;

#[tokio::test]
async fn follower_installs_snapshot_after_compaction_and_survives_cold_restart() {
    let mut cluster = processes::Cluster::start().await;
    cluster.prepare_snapshot_catch_up().await;
    cluster.restart().await;
    cluster.converge().await;
    cluster.assert_snapshot_installed().await;
    cluster.cold_restart().await;
    cluster.converge().await;
    cluster.write(None, "after-snapshot-recovery").await;
    cluster.converge().await;
    cluster.stop().await;
}
