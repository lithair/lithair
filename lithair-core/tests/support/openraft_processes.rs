//! Shared real-process fixture for the integration target and Gherkin runner.
//! Test-only state restores a durable snapshot and replays the retained suffix.
use crate::{
    durable_log::DurableLog,
    peer_transport::{fingerprint, Peer},
};
use openraft::{storage::Adaptor, Config, Raft, SnapshotPolicy};
use openraft_memstore::{ClientRequest, TypeConfig};
#[path = "openraft_snapshot_machine.rs"]
mod snapshot_machine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap, net::SocketAddr, path::PathBuf, process::Stdio, sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    net::{TcpListener, TcpStream},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::{watch, RwLock},
    task::JoinSet,
    time::{timeout, Instant},
};

const DEADLINE: Duration = Duration::from_secs(30);
pub const CHILD_ENV: &str = "LITHAIR_OPENRAFT_TEST_CHILD";

#[path = "openraft_credentials.rs"]
mod credentials;
pub use credentials::Credentials;

#[derive(Serialize, Deserialize)]
struct Setup {
    id: u64,
    cluster_id: String,
    directory: PathBuf,
    create: bool,
    peers: BTreeMap<u64, Peer>,
    credentials: Credentials,
}

async fn next_json<R: tokio::io::AsyncBufRead + Unpin>(lines: &mut Lines<R>) -> Value {
    timeout(DEADLINE, async {
        loop {
            let line = lines.next_line().await.unwrap().expect("child closed stdout");
            if line.starts_with('{') {
                return serde_json::from_str(&line).unwrap();
            }
        }
    })
    .await
    .expect("child response deadline")
}
fn reply(value: Value) {
    println!("{value}");
}

pub async fn child() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    reply(json!({"address":listener.local_addr().unwrap()}));
    let mut input = BufReader::new(tokio::io::stdin()).lines();
    let setup: Setup = serde_json::from_value(next_json(&mut input).await).unwrap();
    let network = setup.credentials.transport(setup.id, &setup.cluster_id, setup.peers);
    let identity = network.node_identity().unwrap();
    let log = if setup.create {
        DurableLog::<TypeConfig>::create_for_node(&setup.directory, identity).await
    } else {
        DurableLog::<TypeConfig>::open_for_node(&setup.directory, identity).await
    };
    let log = match log {
        Ok(log) => log,
        Err(error) => {
            reply(json!({"ready":false,"error":error.to_string()}));
            return;
        }
    };
    let snapshot_machine = snapshot_machine::SnapshotMachine::restore(log.clone()).await.unwrap();
    let memory = snapshot_machine.memory.clone();
    let installs = snapshot_machine.installs.clone();
    // OpenRaft can submit purge before snapshot installation completes.
    // Separate adapter locks let the state-machine worker publish while purge waits.
    let (log_store, _) = Adaptor::new(snapshot_machine.clone());
    let (_, machine) = Adaptor::new(snapshot_machine);
    let config = Config {
        heartbeat_interval: 100,
        election_timeout_min: 400,
        election_timeout_max: 800,
        // A durable snapshot must exist before OpenRaft can purge its prefix.
        snapshot_policy: SnapshotPolicy::LogsSinceLast(8),
        replication_lag_threshold: 8,
        max_in_snapshot_log_to_keep: 2,
        purge_batch_size: 1,
        max_payload_entries: 64,
        snapshot_max_chunk_size: 64 * 1024,
        ..Config::default()
    }
    .validate()
    .unwrap();
    let raft = Raft::new(setup.id, Arc::new(config), network.clone(), log_store, machine)
        .await
        .unwrap();
    let operator_network = network.clone();
    let peer_raft = raft.clone();
    let server = tokio::spawn(async move {
        network.serve(listener, peer_raft, std::future::pending()).await.unwrap()
    });
    reply(json!({"ready":true}));
    while let Some(line) = input.next_line().await.unwrap() {
        let request: Value = serde_json::from_str(&line).unwrap();
        let response = match request["op"].as_str().unwrap() {
            "initialize" => {
                json!({"ok":operator_network.bootstrap(&log, &raft).await.is_ok()})
            }
            "write" => {
                let write = raft.client_write(ClientRequest {
                    client: request["key"].as_str().unwrap().into(),
                    serial: 1,
                    status: request["value"].as_str().unwrap().into(),
                });
                json!({"ok":matches!(timeout(Duration::from_secs(2), write).await, Ok(Ok(_)))})
            }
            "barrier" => {
                json!({"ok":matches!(timeout(Duration::from_secs(2), raft.ensure_linearizable()).await, Ok(Ok(_)))})
            }
            "snapshot" => json!({"ok":raft.trigger().snapshot().await.is_ok()}),
            "state" => {
                let metrics = raft.metrics().borrow().clone();
                json!({"leader":metrics.current_leader,"term":metrics.current_term,"state":memory.get_state_machine().await.client_status,"running":metrics.running_state.is_ok(),"error":format!("{:?}",metrics.running_state),"purged":metrics.purged.map(|id|id.index),"installed":installs.load(std::sync::atomic::Ordering::Relaxed),"initialized":raft.is_initialized().await.unwrap()})
            }
            _ => panic!("unknown fixture operation"),
        };
        reply(response);
    }
    server.abort();
    let _ = server.await;
    raft.shutdown().await.unwrap();
}

struct Process {
    child: Child,
    input: ChildStdin,
    output: Lines<BufReader<ChildStdout>>,
}
impl Process {
    async fn launch() -> (Self, SocketAddr) {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "cluster_child", "--nocapture"])
            .env(CHILD_ENV, "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap()).lines();
        let ready = next_json(&mut output).await;
        let address = serde_json::from_value(ready["address"].clone()).unwrap();
        (Self { child, input, output }, address)
    }
    async fn call(&mut self, request: Value) -> Value {
        let mut bytes = serde_json::to_vec(&request).unwrap();
        bytes.push(b'\n');
        timeout(DEADLINE, self.input.write_all(&bytes)).await.unwrap().unwrap();
        next_json(&mut self.output).await
    }
}

// A directed TCP proxy per edge. Changing `enabled` closes established streams
// as well as refusing new ones, so a partition cuts actual Raft traffic.
struct Edge {
    address: SocketAddr,
    enabled: watch::Sender<bool>,
}
pub struct Cluster {
    processes: BTreeMap<u64, Process>,
    addresses: BTreeMap<u64, Arc<RwLock<SocketAddr>>>,
    edges: BTreeMap<(u64, u64), Edge>,
    proxies: JoinSet<()>,
    credentials: Credentials,
    directory: tempfile::TempDir,
    pub affected: Option<u64>,
    acknowledged: BTreeMap<String, String>,
}
impl std::fmt::Debug for Cluster {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cluster")
            .field("affected", &self.affected)
            .finish_non_exhaustive()
    }
}
impl Cluster {
    pub async fn start() -> Self {
        let mut cluster = Self::provision().await;
        cluster.assert_uninitialized().await;
        cluster.reject_follower_bootstrap().await;
        cluster.bootstrap().await;
        cluster.reject_rebootstrap().await;
        cluster.leader(None).await;
        cluster
    }
    pub async fn provision() -> Self {
        let mut cluster = Self {
            processes: BTreeMap::new(),
            addresses: BTreeMap::new(),
            edges: BTreeMap::new(),
            proxies: JoinSet::new(),
            credentials: Credentials::generate(),
            directory: tempfile::tempdir().unwrap(),
            affected: None,
            acknowledged: BTreeMap::new(),
        };
        for id in 1..=3 {
            let (process, address) = Process::launch().await;
            cluster.processes.insert(id, process);
            cluster.addresses.insert(id, Arc::new(RwLock::new(address)));
            std::fs::create_dir(cluster.directory.path().join(id.to_string())).unwrap();
        }
        for from in 1..=3 {
            for to in 1..=3 {
                if from != to {
                    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                    let address = listener.local_addr().unwrap();
                    let (enabled, receiver) = watch::channel(true);
                    let target = cluster.addresses[&to].clone();
                    cluster.proxies.spawn(proxy(listener, target, receiver));
                    cluster.edges.insert((from, to), Edge { address, enabled });
                }
            }
        }
        for id in 1..=3 {
            cluster.configure(id, true).await;
        }
        cluster
    }
    async fn setup(&self, id: u64, create: bool) -> Setup {
        let mut addresses = BTreeMap::new();
        for to in 1..=3 {
            let address = if to == id {
                *self.addresses[&id].read().await
            } else {
                self.edges[&(id, to)].address
            };
            addresses.insert(to, address);
        }
        Setup {
            id,
            cluster_id: "process-test".into(),
            directory: self.directory.path().join(id.to_string()),
            create,
            peers: self.credentials.peers(&addresses),
            credentials: self.credentials.clone(),
        }
    }
    async fn configure(&mut self, id: u64, create: bool) {
        let setup = self.setup(id, create).await;
        assert_eq!(self.call(id, serde_json::to_value(setup).unwrap()).await["ready"], true);
    }
    pub async fn assert_uninitialized(&mut self) {
        for id in 1..=3 {
            let state = self.call(id, json!({"op":"state"})).await;
            assert_eq!(state["initialized"], false);
            assert!(state["leader"].is_null());
        }
    }
    pub async fn reject_follower_bootstrap(&mut self) {
        for id in [2, 3] {
            assert_eq!(self.call(id, json!({"op":"initialize"})).await["ok"], false);
        }
    }
    pub async fn bootstrap(&mut self) {
        assert_eq!(self.call(1, json!({"op":"initialize"})).await["ok"], true);
    }
    pub async fn reject_unavailable_bootstrap(&mut self) {
        for ((from, to), edge) in &self.edges {
            if *from == 3 || *to == 3 {
                edge.enabled.send_replace(false);
            }
        }
        assert_eq!(self.call(1, json!({"op":"initialize"})).await["ok"], false);
        self.assert_uninitialized().await;
    }
    pub async fn reject_rebootstrap(&mut self) {
        for id in 1..=3 {
            assert_eq!(self.call(id, json!({"op":"initialize"})).await["ok"], false);
        }
    }

    pub async fn reject_wrong_identity(&mut self) {
        let id = 2;
        self.write(None, "before-identity-mismatch").await;
        let mut process = self.processes.remove(&id).unwrap();
        process.child.kill().await.unwrap();
        process.child.wait().await.unwrap();
        for mode in ["node", "cluster", "certificate"] {
            let (mut rejected, _) = Process::launch().await;
            let mut setup = self.setup(id, false).await;
            match mode {
                "cluster" => setup.cluster_id = "other-cluster".into(),
                "node" => setup.id = 3,
                _ => {
                    let cert = setup.credentials.certs[&4].clone();
                    setup.peers.get_mut(&id).unwrap().certificate_sha256 = fingerprint(&cert);
                    setup.credentials.certs.insert(id, cert);
                    let key = setup.credentials.keys[&4].clone();
                    setup.credentials.keys.insert(id, key);
                }
            }
            let response = rejected.call(serde_json::to_value(setup).unwrap()).await;
            assert_eq!(response["ready"], false, "{response}");
            assert!(response["error"].as_str().unwrap().contains("identity mismatch"));
            assert!(timeout(DEADLINE, rejected.child.wait()).await.unwrap().unwrap().success());
        }
        self.affected = Some(id);
        self.restart().await;
        self.converge().await;
    }
    pub async fn call(&mut self, id: u64, request: Value) -> Value {
        self.processes.get_mut(&id).unwrap().call(request).await
    }
    pub async fn leader(&mut self, exclude: Option<u64>) -> u64 {
        let deadline = Instant::now() + DEADLINE;
        let mut tick = tokio::time::interval(Duration::from_millis(40));
        loop {
            for id in self.processes.keys().copied().collect::<Vec<_>>() {
                if Some(id) == exclude {
                    continue;
                }
                let state = self.call(id, json!({"op":"state"})).await;
                assert_eq!(state["running"], true, "node {id}: {state}");
                if state["leader"] == id
                    && self.call(id, json!({"op":"barrier"})).await["ok"] == true
                {
                    return id;
                }
            }
            assert!(Instant::now() < deadline, "no leader before deadline");
            tick.tick().await;
        }
    }
    pub async fn write(&mut self, exclude: Option<u64>, key: &str) -> u64 {
        let deadline = Instant::now() + DEADLINE;
        loop {
            let leader = self.leader(exclude).await;
            if self.call(leader, json!({"op":"write","key":key,"value":key})).await["ok"] == true {
                self.acknowledged.insert(key.into(), key.into());
                return leader;
            }
            assert!(Instant::now() < deadline, "write not acknowledged");
        }
    }
    pub async fn crash_leader(&mut self) {
        let leader = self.write(None, "before-crash").await;
        self.affected = Some(leader);
        let mut process = self.processes.remove(&leader).unwrap();
        process.child.kill().await.unwrap();
        assert!(!process.child.wait().await.unwrap().success());
    }
    pub async fn restart(&mut self) {
        let id = self.affected.unwrap();
        let (process, address) = Process::launch().await;
        *self.addresses[&id].write().await = address;
        self.processes.insert(id, process);
        self.configure(id, false).await;
        assert_eq!(self.call(id, json!({"op":"initialize"})).await["ok"], false);
    }
    pub async fn partition(&mut self) {
        let id = self.write(None, "before-partition").await;
        self.affected = Some(id);
        for ((from, to), edge) in &self.edges {
            if *from == id || *to == id {
                edge.enabled.send_replace(false);
            }
        }
        // A successful majority barrier proves that a replacement leader is active.
        self.leader(Some(id)).await;
    }
    pub async fn minority_rejects(&mut self) {
        let id = self.affected.unwrap();
        assert_eq!(
            self.call(id, json!({"op":"write","key":"uncertain","value":"uncertain"})).await["ok"],
            false
        );
        assert_eq!(self.call(id, json!({"op":"barrier"})).await["ok"], false);
    }
    pub fn heal(&mut self) {
        for edge in self.edges.values() {
            edge.enabled.send_replace(true);
        }
    }
    pub async fn converge(&mut self) {
        let deadline = Instant::now() + DEADLINE;
        let mut tick = tokio::time::interval(Duration::from_millis(40));
        loop {
            let mut complete = true;
            let mut states = Vec::new();
            for id in 1..=3 {
                let state = self.call(id, json!({"op":"state"})).await;
                assert_eq!(state["running"], true, "node {id}: {state}");
                for (key, value) in &self.acknowledged {
                    complete &= state["state"][key] == value.as_str();
                }
                states.push(state["state"].clone());
            }
            if complete && states.windows(2).all(|s| s[0] == s[1]) {
                return;
            }
            assert!(Instant::now() < deadline, "replicas did not converge: {states:?}");
            tick.tick().await;
        }
    }
    pub async fn stop(&mut self) {
        for process in self.processes.values_mut() {
            process.child.kill().await.unwrap();
            let _ = process.child.wait().await.unwrap();
        }
        self.processes.clear();
        self.proxies.abort_all();
        while self.proxies.join_next().await.is_some() {}
    }

    pub async fn prepare_snapshot_catch_up(&mut self) {
        let leader = self.write(None, "before-snapshot").await;
        let follower = (1..=3).find(|id| *id != leader).unwrap();
        self.affected = Some(follower);
        let mut process = self.processes.remove(&follower).unwrap();
        process.child.kill().await.unwrap();
        let _ = process.child.wait().await.unwrap();
        for index in 0..24 {
            self.write(Some(follower), &format!("snapshot-{index}")).await;
        }
        let leader = self.leader(Some(follower)).await;
        assert_eq!(self.call(leader, json!({"op":"snapshot"})).await["ok"], true);
        let deadline = Instant::now() + DEADLINE;
        let mut tick = tokio::time::interval(Duration::from_millis(40));
        loop {
            let state = self.call(leader, json!({"op":"state"})).await;
            if state["purged"].as_u64().is_some_and(|index| index >= 20) {
                break;
            }
            assert!(Instant::now() < deadline, "leader did not purge snapshot prefix: {state}");
            tick.tick().await;
        }
    }
    pub async fn assert_snapshot_installed(&mut self) {
        let state = self.call(self.affected.unwrap(), json!({"op":"state"})).await;
        assert!(
            state["installed"].as_u64().unwrap() > 0,
            "follower must install a snapshot over TLS: {state}"
        );
    }

    pub async fn cold_restart(&mut self) {
        self.restart_all().await;
        self.leader(None).await;
        self.reject_rebootstrap().await;
    }
    pub async fn restart_uninitialized(&mut self) {
        self.restart_all().await;
        self.assert_uninitialized().await;
    }
    async fn restart_all(&mut self) {
        for process in self.processes.values_mut() {
            process.child.kill().await.unwrap();
            let _ = process.child.wait().await.unwrap();
        }
        self.processes.clear();
        for id in 1..=3 {
            let (process, address) = Process::launch().await;
            *self.addresses[&id].write().await = address;
            self.processes.insert(id, process);
        }
        for id in 1..=3 {
            self.configure(id, false).await;
        }
    }
}

async fn proxy(
    listener: TcpListener,
    target: Arc<RwLock<SocketAddr>>,
    mut enabled: watch::Receiver<bool>,
) {
    let mut streams = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (mut source,_) = accepted.unwrap();
                if !*enabled.borrow() { continue; }
                let target = target.clone();
                let mut enabled = enabled.clone();
                streams.spawn(async move {
                    tokio::select! {
                        _ = enabled.changed() => {},
                        _ = async {
                            if let Ok(mut destination) = TcpStream::connect(*target.read().await).await {
                                let _ = tokio::io::copy_bidirectional(&mut source, &mut destination).await;
                            }
                        } => {}
                    }
                });
            }
            _ = enabled.changed() => { if !*enabled.borrow() { streams.abort_all(); } },
            Some(_) = streams.join_next(), if !streams.is_empty() => {},
        }
    }
}
