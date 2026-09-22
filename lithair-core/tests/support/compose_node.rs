//! Test-only Compose node. Never a public Lithair application entry point.
#[path = "openraft_credentials.rs"]
#[allow(dead_code)]
mod credentials;
#[path = "../../src/cluster/durable_log/mod.rs"]
#[allow(dead_code)]
mod durable_log;
#[path = "../../src/cluster/peer_transport/mod.rs"]
#[allow(dead_code)]
mod peer_transport;
#[path = "openraft_snapshot_machine.rs"]
mod snapshot_machine;

use anyhow::{ensure, Context};
use bytes::Bytes;
use credentials::Credentials;
use durable_log::{credentials::PendingCertificate, CredentialPolicy, DurableLog, NodeIdentity};
use http_body_util::{BodyExt, Full, Limited};
use hyper::{body::Incoming, service::service_fn, Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use openraft::{storage::Adaptor, Config, Raft, SnapshotPolicy};
use openraft_memstore::{ClientRequest, TypeConfig};
use peer_transport::{fingerprint, PeerTransport};
use serde_json::{json, Value};
use snapshot_machine::SnapshotMachine;
use std::{collections::BTreeMap, convert::Infallible, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{net::TcpListener, time::timeout};

fn identity(credentials: &Credentials, node_id: u64) -> NodeIdentity {
    NodeIdentity {
        cluster_id: "compose-qualification".into(),
        node_id,
        bootstrap_node: 1,
        voters: (1..=3).map(|id| (id, fingerprint(&credentials.certs[&id]))).collect(),
    }
}
async fn prepare() -> anyhow::Result<()> {
    let credentials = Credentials::generate();
    for id in 1..=3 {
        let mut local = credentials.clone();
        local.keys.retain(|key, _| *key == id || (id == 2 && *key == 5));
        local.certs.retain(|key, _| *key <= 3 || *key == 5);
        let path = format!("/secrets/{id}/credentials.json");
        use std::{io::Write, os::unix::fs::OpenOptionsExt};
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(&serde_json::to_vec(&local)?)?;
        file.sync_all()?;
        let store = DurableLog::<TypeConfig>::create_for_node(
            format!("/stores/{id}"),
            identity(&local, id),
        )
        .await?;
        drop(store);
    }
    println!("provisioned three independent stores; bootstrap remains unclaimed");
    Ok(())
}
async fn address(name: &str, port: u16) -> anyhow::Result<SocketAddr> {
    // Peer dial addresses are explicit and remain available when another node
    // is stopped. Docker DNS for a stopped container need not resolve.
    if let Some(id) = name.strip_prefix("raft") {
        let ip = std::env::var(format!("LITHAIR_CLUSTER_NODE{id}_IP"))?.parse()?;
        return Ok(SocketAddr::new(ip, port));
    }
    timeout(Duration::from_secs(30), async {
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        loop {
            if let Ok(mut addresses) = tokio::net::lookup_host((name, port)).await {
                if let Some(address) = addresses.find(|a| a.is_ipv4()) {
                    return address;
                }
            }
            tick.tick().await;
        }
    })
    .await
    .context("Compose DNS deadline")
}
#[derive(Clone)]
struct Node {
    id: u64,
    raft: Raft<TypeConfig>,
    log: DurableLog<TypeConfig>,
    network: PeerTransport<TypeConfig>,
    machine: SnapshotMachine,
}
impl Node {
    async fn dispatch(&self, request: Request<Incoming>) -> anyhow::Result<Response<Full<Bytes>>> {
        let path = request.uri().path().to_owned();
        let method = request.method().clone();
        let response = |status, value: Value| {
            let mut response = Response::new(Full::new(Bytes::from(value.to_string())));
            *response.status_mut() = status;
            response
                .headers_mut()
                .insert(hyper::header::CONTENT_TYPE, "application/json".parse().unwrap());
            response
        };
        if method == Method::GET && path == "/test/state" {
            let metrics = self.raft.metrics().borrow().clone();
            let state = self.machine.memory.get_state_machine().await;
            return Ok(response(
                StatusCode::OK,
                json!({"id":self.id,"leader":metrics.current_leader,
                "running":metrics.running_state.is_ok(),"state":state.client_status,
                "applied":state.last_applied_log.map(|l|l.index),"purged":metrics.purged.map(|l|l.index),
                "installed":self.machine.installs.load(std::sync::atomic::Ordering::Relaxed),
                "credential_generation":self.network.credential_policy().generation,
                "initialized":self.raft.is_initialized().await?}),
            ));
        }
        if method != Method::POST
            || !matches!(path.as_str(), "/test/initialize" | "/test/write" | "/test/barrier")
        {
            return Ok(response(StatusCode::NOT_FOUND, json!({"ok":false})));
        }
        let body = Limited::new(request.into_body(), 16 * 1024)
            .collect()
            .await
            .map_err(|_| anyhow::anyhow!("invalid or oversized test request"))?
            .to_bytes();
        let body: Value = serde_json::from_slice(&body)?;
        let accepted = match path.as_str() {
            "/test/initialize" => self.network.bootstrap(&self.log, &self.raft).await.is_ok(),
            "/test/write" => {
                let key = body["key"].as_str().context("missing key")?;
                let value = body["value"].as_str().context("missing value")?;
                matches!(
                    timeout(
                        Duration::from_secs(2),
                        self.raft.client_write(ClientRequest {
                            client: key.into(),
                            serial: 1,
                            status: value.into()
                        })
                    )
                    .await,
                    Ok(Ok(_))
                )
            }
            _ => matches!(
                timeout(Duration::from_secs(2), self.raft.ensure_linearizable()).await,
                Ok(Ok(_))
            ),
        };
        Ok(response(
            if accepted { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE },
            json!({"ok":accepted}),
        ))
    }
}
async fn serve(id: u64, credentials: Credentials) -> anyhow::Result<()> {
    ensure!(
        credentials.keys.contains_key(&id)
            && credentials.keys.keys().all(|key| *key == id || (id == 2 && *key == 5)),
        "node must receive only its own private keys"
    );
    let mut addresses = BTreeMap::new();
    for peer in 1..=3 {
        addresses.insert(peer, address(&format!("raft{peer}"), 9443).await?);
    }
    let policy = configured_policy(&credentials, id)?;
    let selected = if id == 2 && std::path::Path::new("/data/use-renewed-certificate").exists() {
        5
    } else {
        id
    };
    let network = transport(&credentials, id, selected, &addresses, policy)?;
    let listener = TcpListener::bind(addresses[&id]).await?;
    let control = TcpListener::bind(address(&format!("control{id}"), 8080).await?).await?;
    let log = DurableLog::<TypeConfig>::open_with_credentials(
        "/data",
        network.node_identity()?,
        network.credential_policy().clone(),
    )
    .await?;
    let machine = SnapshotMachine::restore(log.clone()).await?;
    let (store, _) = Adaptor::new(machine.clone());
    let (_, apply) = Adaptor::new(machine.clone());
    let config = Config {
        heartbeat_interval: 100,
        election_timeout_min: 400,
        election_timeout_max: 800,
        snapshot_policy: SnapshotPolicy::LogsSinceLast(8),
        replication_lag_threshold: 8,
        max_in_snapshot_log_to_keep: 2,
        purge_batch_size: 1,
        max_payload_entries: 64,
        snapshot_max_chunk_size: 64 * 1024,
        ..Config::default()
    }
    .validate()?;
    let raft = Raft::new(id, Arc::new(config), network.clone(), store, apply).await?;
    let node = Node { id, raft: raft.clone(), log, network: network.clone(), machine };
    let rpc = network.serve(listener, raft, std::future::pending());
    let http = async move {
        let mut connections = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                Some(_) = connections.join_next(), if !connections.is_empty() => {},
                socket = control.accept() => {
                    let (socket, _) = socket?;
                    let node = node.clone();
                    connections.spawn(async move {
                        let service = service_fn(move |request| {
                            let node = node.clone();
                            async move { Ok::<_,Infallible>(node.dispatch(request).await.unwrap_or_else(|_| {
                                Response::builder().status(StatusCode::BAD_REQUEST).body(Full::new(Bytes::new())).unwrap()
                            })) }
                        });
                        let _ = timeout(Duration::from_secs(15), hyper::server::conn::http1::Builder::new()
                            .serve_connection(TokioIo::new(socket),service)).await;
                    });
                }
            }
        }
        #[allow(unreachable_code)]
        Ok::<(), std::io::Error>(())
    };
    tokio::select! { result = rpc => result?, result = http => result? }
    Ok(())
}
async fn reject_unauthenticated(credentials: Credentials, id: u64) -> anyhow::Result<()> {
    use rustls::pki_types::{CertificateDer, ServerName};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut roots = rustls::RootCertStore::empty();
    roots.add(CertificateDer::from(credentials.ca))?;
    let client = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_root_certificates(roots)
    .with_no_client_auth();
    // TCP must connect; an unavailable service is not proof of TLS rejection.
    let socket = tokio::net::TcpStream::connect(address(&format!("raft{id}"), 9443).await?).await?;
    let result = timeout(Duration::from_secs(6), async {
        let mut tls = tokio_rustls::TlsConnector::from(Arc::new(client))
            .connect(ServerName::try_from(format!("node{id}.test"))?, socket)
            .await?;
        tls.write_all(
            b"POST /raft/v2/preflight HTTP/1.1\r\nHost: peer\r\nContent-Length: 0\r\n\r\n",
        )
        .await?;
        let mut bytes = vec![0; 1024];
        let count = tls.read(&mut bytes).await?;
        Ok::<usize, anyhow::Error>(count)
    })
    .await?;
    // A TLS error, or EOF after the TLS 1.3 handshake, is the expected rejection.
    if let Ok(count) = result {
        ensure!(count == 0, "server answered a client without a certificate");
    }
    println!("peer listener rejected the client without a certificate");
    Ok(())
}
fn configured_policy(credentials: &Credentials, id: u64) -> anyhow::Result<CredentialPolicy> {
    match std::fs::read("/data/credential-policy.json") {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(CredentialPolicy::initial(&identity(credentials, id)))
        }
        Err(error) => Err(error.into()),
    }
}
fn transport(
    credentials: &Credentials,
    id: u64,
    selected: u64,
    addresses: &BTreeMap<u64, SocketAddr>,
    policy: CredentialPolicy,
) -> anyhow::Result<PeerTransport<TypeConfig>> {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    PeerTransport::with_credentials(
        identity(credentials, id),
        policy,
        credentials.peers(addresses),
        vec![CertificateDer::from(credentials.ca.clone())],
        vec![CertificateDer::from(credentials.certs[&selected].clone())],
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(credentials.keys[&selected].clone())),
    )
}
async fn update_policy(credentials: &Credentials, id: u64, generation: u64) -> anyhow::Result<()> {
    ensure!(matches!(generation, 1 | 2), "invalid test generation");
    let binding = identity(credentials, id);
    let mut policy = CredentialPolicy::initial(&binding);
    policy.generation = generation;
    let renewed = fingerprint(&credentials.certs[&5]);
    if generation == 1 {
        policy.pending = Some(PendingCertificate { node_id: 2, certificate_sha256: renewed });
    } else {
        policy.current.insert(2, renewed);
    }
    DurableLog::<TypeConfig>::transition_credentials(
        "/data",
        binding,
        generation - 1,
        policy.clone(),
    )
    .await?;
    std::fs::write("/data/credential-policy.json", serde_json::to_vec(&policy)?)?;
    Ok(())
}
async fn probe_retired(credentials: &Credentials, id: u64, server: bool) -> anyhow::Result<()> {
    let mut addresses = BTreeMap::new();
    for peer in 1..=3 {
        addresses.insert(peer, address(&format!("raft{peer}"), 9443).await?);
    }
    let selected = if id == 2 { 5 } else { id };
    let valid =
        transport(credentials, id, selected, &addresses, configured_policy(credentials, id)?)?;
    let target = if id == 2 { 1 } else { 2 };
    ensure!(valid.preflight(target).await?, "healthy peer must answer a valid credential");
    if server {
        ensure!(id == 1, "outgoing probe runs on node 1");
        addresses.get_mut(&2).context("missing peer")?.set_port(9444);
        let probe =
            transport(credentials, id, selected, &addresses, configured_policy(credentials, id)?)?;
        let error = probe.preflight(2).await.expect_err("retired server certificate was accepted");
        ensure!(
            format!("{error:#}").contains("server identity mismatch"),
            "expected certificate pin rejection: {error:#}"
        );
    } else {
        ensure!(id == 2, "incoming probe uses node 2's retired key");
        let probe = transport(
            credentials,
            id,
            id,
            &addresses,
            CredentialPolicy::initial(&identity(credentials, id)),
        )?;
        ensure!(probe.preflight(1).await.is_err(), "retired client certificate was accepted");
    }
    ensure!(valid.preflight(target).await?, "peer must still answer a valid credential");
    println!("retired certificate rejected; valid peer remains reachable");
    Ok(())
}
async fn old_server(credentials: &Credentials, id: u64) -> anyhow::Result<()> {
    ensure!(id == 2, "old server represents node 2");
    let address = address("raft2", 9444).await?;
    let addresses = [(1, address), (2, address), (3, address)].into();
    let network = transport(
        credentials,
        id,
        id,
        &addresses,
        CredentialPolicy::initial(&identity(credentials, id)),
    )?;
    let listener = TcpListener::bind(address).await?;
    let (log, sm) = Adaptor::new(openraft_memstore::MemStore::new_async().await);
    let config =
        Config { enable_elect: false, enable_heartbeat: false, ..Config::default() }.validate()?;
    let raft = Raft::new(id, Arc::new(config), network.clone(), log, sm).await?;
    network.serve(listener, raft, std::future::pending()).await?;
    Ok(())
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let command = args.get(1).context("missing fixture command")?;
    if command == "prepare" {
        return prepare().await;
    }
    let id: u64 = args.get(2).context("missing node ID")?.parse()?;
    ensure!((1..=3).contains(&id), "invalid node ID");
    let credentials: Credentials =
        serde_json::from_slice(&std::fs::read("/secrets/credentials.json")?)?;
    match command.as_str() {
        "serve" => serve(id, credentials).await,
        "old-server" => old_server(&credentials, id).await,
        "probe-retired-client" => probe_retired(&credentials, id, false).await,
        "probe-retired-server" => probe_retired(&credentials, id, true).await,
        "update-policy" => {
            update_policy(&credentials, id, args.get(3).context("missing generation")?.parse()?)
                .await
        }
        "renew-certificate" => {
            ensure!(id == 2, "only node 2 has a renewal");
            // Offline inspection proves the store lock is available.
            DurableLog::<TypeConfig>::inspect_for_node("/data", identity(&credentials, id)).await?;
            ensure!(configured_policy(&credentials, id)?.generation == 1, "overlap required");
            std::fs::write("/data/use-renewed-certificate", b"renewed")?;
            Ok(())
        }
        "inspect" => {
            println!(
                "{}",
                DurableLog::<TypeConfig>::inspect_for_node("/data", identity(&credentials, id))
                    .await?
            );
            Ok(())
        }
        "reject-unauthenticated" => reject_unauthenticated(credentials, id).await,
        _ => anyhow::bail!("unknown fixture command"),
    }
}
