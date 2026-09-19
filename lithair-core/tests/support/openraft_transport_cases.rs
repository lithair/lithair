use crate::{
    peer_transport::{PeerTransport, MAX_BODY},
    processes::Credentials,
};
use openraft::{
    network::{RPCOption, RaftNetwork, RaftNetworkFactory},
    raft::{AppendEntriesRequest, InstallSnapshotRequest, VoteRequest},
    storage::Adaptor,
    Config, Raft, Vote,
};
use openraft_memstore::{MemStore, TypeConfig};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use serde_json::{json, Value};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::{net::TcpListener, task::JoinHandle};

struct Fixture {
    credentials: Credentials,
    peers: BTreeMap<u64, crate::peer_transport::Peer>,
    raft: Raft<TypeConfig>,
    task: JoinHandle<()>,
}
impl Fixture {
    async fn new() -> Self {
        let credentials = Credentials::generate();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let peers = credentials.peers(&[(1, address), (2, address), (3, address)].into());
        let network = credentials.transport(2, "test", peers.clone());
        let (log, sm) = Adaptor::new(MemStore::new_async().await);
        let config = Config { enable_elect: false, enable_heartbeat: false, ..Config::default() }
            .validate()
            .unwrap();
        let raft = Raft::new(2, Arc::new(config), network.clone(), log, sm).await.unwrap();
        let remote = raft.clone();
        let task = tokio::spawn(async move {
            network.serve(listener, remote, std::future::pending()).await.unwrap();
        });
        Self { credentials, peers, raft, task }
    }
    fn client(&self) -> PeerTransport<TypeConfig> {
        self.credentials.transport(1, "test", self.peers.clone())
    }
    async fn stop(self) {
        self.task.abort();
        let _ = self.task.await;
        self.raft.shutdown().await.unwrap();
    }
}
fn vote_body() -> Value {
    json!({"version":1,"cluster":"test","sender":1,"recipient":2,
        "payload":VoteRequest::new(Vote::new(5,1),None)})
}
fn option() -> RPCOption {
    RPCOption::new(Duration::from_secs(2))
}

#[tokio::test]
async fn authenticated_vote_append_and_snapshot_rpc_round_trips() {
    let fixture = Fixture::new().await;
    let mut network = fixture.client();
    let mut client = network.new_client(2, &()).await;
    let response = client.vote(VoteRequest::new(Vote::new(5, 1), None), option()).await.unwrap();
    assert!(response.vote_granted);
    let append = AppendEntriesRequest::<TypeConfig> {
        vote: Vote::new_committed(5, 1),
        prev_log_id: None,
        entries: vec![],
        leader_commit: None,
    };
    assert!(client.append_entries(append, option()).await.is_ok());
    // A stale snapshot request is rejected by Raft with the newer vote, proving
    // typed snapshot RPC routing without claiming durable snapshot installation.
    let snapshot = InstallSnapshotRequest::<TypeConfig> {
        vote: Vote::new_committed(1, 1),
        meta: Default::default(),
        offset: 0,
        data: vec![],
        done: true,
    };
    let response = client.install_snapshot(snapshot, option()).await.unwrap();
    assert_eq!(response.vote, Vote::new_committed(5, 1));
    fixture.raft.shutdown().await.unwrap();
    assert!(matches!(
        client.vote(VoteRequest::new(Vote::new(6, 1), None), option()).await,
        Err(openraft::error::RPCError::RemoteError(_))
    ));
    fixture.task.abort();
    let _ = fixture.task.await;
}

#[tokio::test]
async fn invalid_envelopes_are_rejected_before_raft_dispatch() {
    let fixture = Fixture::new().await;
    let client = fixture.client();
    let mut cases = Vec::new();
    for (key, value) in [
        ("version", json!(2)),
        ("cluster", json!("another")),
        ("sender", json!(3)),
        ("recipient", json!(3)),
        ("extra", json!(true)),
    ] {
        let mut body = vote_body();
        body[key] = value;
        cases.push(serde_json::to_vec(&body).unwrap());
    }
    let mut impersonated = vote_body();
    impersonated["payload"] =
        serde_json::to_value(VoteRequest::new(Vote::new(5, 3), None)).unwrap();
    cases.push(serde_json::to_vec(&impersonated).unwrap());
    cases.push(b"malformed".to_vec());
    cases.push(vec![b' '; MAX_BODY + 1]);
    for body in cases {
        assert!(client.raw_rpc(2, "/raft/v1/vote", body).await.is_err());
    }
    assert!(client
        .raw_rpc(2, "/raft/v1/vote?unexpected=1", serde_json::to_vec(&vote_body()).unwrap())
        .await
        .is_err());
    assert!(client
        .raw_rpc(2, "/public", serde_json::to_vec(&vote_body()).unwrap())
        .await
        .is_err());
    assert_eq!(fixture.raft.metrics().borrow().current_term, 0);
    fixture.stop().await;
}

#[tokio::test]
async fn mutual_tls_rejects_missing_unknown_and_foreign_certificates() {
    let fixture = Fixture::new().await;
    let mut missing = fixture.client();
    missing.without_client_certificate(CertificateDer::from(fixture.credentials.ca.clone()));
    assert!(missing
        .raw_rpc(2, "/raft/v1/vote", serde_json::to_vec(&vote_body()).unwrap())
        .await
        .is_err());
    let mut peers = fixture.peers.clone();
    peers.insert(
        4,
        crate::peer_transport::Peer {
            address: peers[&2].address,
            server_name: "node4.test".into(),
            certificate_sha256: crate::peer_transport::fingerprint(&fixture.credentials.certs[&4]),
        },
    );
    let unknown = fixture.credentials.transport(4, "test", peers);
    assert!(unknown
        .raw_rpc(2, "/raft/v1/vote", serde_json::to_vec(&vote_body()).unwrap())
        .await
        .is_err());
    let foreign = Credentials::generate();
    let mut peers = fixture.peers.clone();
    peers.get_mut(&1).unwrap().certificate_sha256 =
        crate::peer_transport::fingerprint(&foreign.certs[&1]);
    let foreign = PeerTransport::<TypeConfig>::new(
        "test".into(),
        1,
        peers,
        vec![fixture.credentials.ca.clone().into()],
        vec![foreign.certs[&1].clone().into()],
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(foreign.keys[&1].clone())),
    )
    .unwrap();
    assert!(foreign
        .raw_rpc(2, "/raft/v1/vote", serde_json::to_vec(&vote_body()).unwrap())
        .await
        .is_err());
    assert_eq!(fixture.raft.metrics().borrow().current_term, 0);
    fixture.stop().await;
}

#[tokio::test]
async fn outbound_identity_and_body_limits_fail_closed() {
    let fixture = Fixture::new().await;
    for wrong_name in [false, true] {
        let mut peers = fixture.peers.clone();
        if wrong_name {
            peers.get_mut(&2).unwrap().server_name = "different.test".into();
        } else {
            peers.get_mut(&2).unwrap().certificate_sha256 = [0; 32];
        }
        let client = fixture.credentials.transport(1, "test", peers);
        assert!(client
            .raw_rpc(2, "/raft/v1/vote", serde_json::to_vec(&vote_body()).unwrap())
            .await
            .is_err());
    }
    let mut network = fixture.client();
    let mut client = network.new_client(2, &()).await;
    let large = InstallSnapshotRequest::<TypeConfig> {
        vote: Vote::new_committed(5, 1),
        meta: Default::default(),
        offset: 0,
        data: vec![0; MAX_BODY],
        done: true,
    };
    assert!(client.install_snapshot(large, option()).await.is_err());
    let mut unknown = network.new_client(999, &()).await;
    assert!(unknown.vote(VoteRequest::new(Vote::new(5, 1), None), option()).await.is_err());
    assert_eq!(fixture.raft.metrics().borrow().current_term, 0);
    fixture.stop().await;
}

#[tokio::test]
async fn enrollment_rejects_duplicate_certificate_identities() {
    let fixture = Fixture::new().await;
    let mut peers = fixture.peers.clone();
    peers.get_mut(&3).unwrap().certificate_sha256 = peers[&2].certificate_sha256;
    assert!(PeerTransport::<TypeConfig>::new(
        "test".into(),
        1,
        peers,
        vec![fixture.credentials.ca.clone().into()],
        vec![fixture.credentials.certs[&1].clone().into()],
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(fixture.credentials.keys[&1].clone()))
    )
    .is_err());
    fixture.stop().await;
}

#[tokio::test]
async fn rpc_deadline_closes_an_unresponsive_tls_connection() {
    use tokio::io::AsyncReadExt;
    let fixture = Fixture::new().await;
    let silent = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut peers = fixture.peers.clone();
    peers.get_mut(&2).unwrap().address = silent.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = silent.accept().await.unwrap();
        let mut hello = Vec::new();
        tokio::time::timeout(Duration::from_secs(10), socket.read_to_end(&mut hello))
            .await
            .unwrap()
            .unwrap();
        assert!(!hello.is_empty(), "a real TLS handshake was attempted");
    });
    let mut transport = fixture.credentials.transport(1, "test", peers);
    let mut client = transport.new_client(2, &()).await;
    let error = client
        .vote(VoteRequest::new(Vote::new(5, 1), None), RPCOption::new(Duration::from_secs(2)))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("deadline has elapsed"), "{error}");
    server.await.unwrap();
    fixture.stop().await;
}
