//! Authenticated internal OpenRaft RPCs. Enrollment is operator supplied and
//! fixed for this foundation; it is not automatic Raft membership management.

use super::durable_log::NodeIdentity;
use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::{body::Incoming, service::service_fn, Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use openraft::error::{InstallSnapshotError, NetworkError, RPCError, RaftError, RemoteError};
use openraft::network::{RPCOption, RaftNetwork, RaftNetworkFactory};
use openraft::raft::{
    AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest, InstallSnapshotResponse,
    VoteRequest, VoteResponse,
};
use openraft::{Raft, RaftTypeConfig};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap, convert::Infallible, io, marker::PhantomData, net::SocketAddr,
    sync::Arc, time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    task::JoinSet,
};
use tokio_rustls::{TlsAcceptor, TlsConnector};

const WIRE_VERSION: u32 = 1;
// JSON byte arrays expand snapshot chunks: callers must keep chunks below 64KiB.
pub(crate) const MAX_BODY: usize = 1024 * 1024;
const MAX_CONNECTIONS: usize = 32;
const MAX_LIFETIME: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Peer {
    pub address: SocketAddr,
    pub server_name: String,
    pub certificate_sha256: [u8; 32],
}

pub(crate) fn fingerprint(cert: &[u8]) -> [u8; 32] {
    Sha256::digest(cert).into()
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope<T> {
    version: u32,
    cluster: String,
    sender: u64,
    recipient: u64,
    payload: T,
}

struct Settings {
    cluster: String,
    local: u64,
    peers: BTreeMap<u64, Peer>,
    server: Arc<rustls::ServerConfig>,
    client: Arc<rustls::ClientConfig>,
    outgoing: Semaphore,
}

// Manual Clone avoids requiring C: Clone beyond the RaftTypeConfig contract.
pub(crate) struct PeerTransport<C> {
    settings: Arc<Settings>,
    _config: PhantomData<C>,
}
impl<C> Clone for PeerTransport<C> {
    fn clone(&self) -> Self {
        Self { settings: self.settings.clone(), _config: PhantomData }
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

impl<C: RaftTypeConfig<NodeId = u64>> PeerTransport<C> {
    /// Bind durable enrollment to the identities already validated by this TLS
    /// transport. Addresses may change without changing certificate identities.
    pub(crate) fn node_identity(&self, bootstrap_node: u64) -> anyhow::Result<NodeIdentity> {
        let identity = NodeIdentity {
            cluster_id: self.settings.cluster.clone(),
            node_id: self.settings.local,
            bootstrap_node,
            voters: self
                .settings
                .peers
                .iter()
                .map(|(id, peer)| (*id, peer.certificate_sha256))
                .collect(),
        };
        identity.validate()?;
        Ok(identity)
    }

    pub(crate) fn new(
        cluster: String,
        local: u64,
        peers: BTreeMap<u64, Peer>,
        trust: Vec<CertificateDer<'static>>,
        chain: Vec<CertificateDer<'static>>,
        key: PrivateKeyDer<'static>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(!cluster.is_empty() && cluster.len() <= 128, "invalid cluster ID");
        let leaf = chain.first().ok_or_else(|| invalid("missing local certificate"))?;
        let own = peers.get(&local).ok_or_else(|| invalid("local node is not enrolled"))?;
        anyhow::ensure!(own.certificate_sha256 == fingerprint(leaf), "local certificate mismatch");
        let mut pins = std::collections::BTreeSet::new();
        for peer in peers.values() {
            ServerName::try_from(peer.server_name.clone())?;
            anyhow::ensure!(peer.address.port() != 0, "missing peer port");
            anyhow::ensure!(pins.insert(peer.certificate_sha256), "duplicate peer certificate");
        }
        let mut roots = rustls::RootCertStore::empty();
        for cert in trust {
            roots.add(cert)?;
        }
        anyhow::ensure!(!roots.is_empty(), "empty peer trust store");
        let roots = Arc::new(roots);
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
            roots.clone(),
            provider.clone(),
        )
        .build()?;
        let mut server = rustls::ServerConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()?
            .with_client_cert_verifier(verifier)
            .with_single_cert(chain.clone(), key.clone_key())?;
        server.alpn_protocols = vec![b"http/1.1".to_vec()];
        let mut client = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()?
            .with_root_certificates(roots)
            .with_client_auth_cert(chain, key)?;
        client.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(Self {
            settings: Arc::new(Settings {
                cluster,
                local,
                peers,
                server: Arc::new(server),
                client: Arc::new(client),
                outgoing: Semaphore::new(MAX_CONNECTIONS),
            }),
            _config: PhantomData,
        })
    }

    /// Owns accepted connections; shutdown cancels and joins them. No public HTTP
    /// server integration and no bootstrap endpoint is installed here.
    pub(crate) async fn serve(
        &self,
        listener: TcpListener,
        raft: Raft<C>,
        shutdown: impl std::future::Future<Output = ()>,
    ) -> io::Result<()> {
        tokio::pin!(shutdown);
        let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
        let mut tasks = JoinSet::new();
        let result = loop {
            tokio::select! {
                _ = &mut shutdown => break Ok(()),
                Some(_) = tasks.join_next(), if !tasks.is_empty() => {},
                accepted = listener.accept() => {
                    let (socket, _) = match accepted { Ok(v) => v, Err(e) => break Err(e) };
                    let Ok(permit) = slots.clone().try_acquire_owned() else { continue; };
                    let transport = self.clone();
                    let raft = raft.clone();
                    tasks.spawn(async move {
                        let _permit = permit;
                        let _ = tokio::time::timeout(MAX_LIFETIME, async move {
                            let tls = TlsAcceptor::from(transport.settings.server.clone()).accept(socket).await?;
                            let cert = tls.get_ref().1.peer_certificates().and_then(|c| c.first())
                                .ok_or_else(|| invalid("missing client certificate"))?;
                            let pin = fingerprint(cert);
                            let sender = transport.settings.peers.iter()
                                .find_map(|(id, peer)| (peer.certificate_sha256 == pin).then_some(*id))
                                .filter(|id| *id != transport.settings.local)
                                .ok_or_else(|| invalid("unenrolled client certificate"))?;
                            let service = service_fn(move |request| {
                                let transport = transport.clone();
                                let raft = raft.clone();
                                async move { Ok::<_, Infallible>(transport.dispatch(raft, sender, request).await) }
                            });
                            hyper::server::conn::http1::Builder::new()
                                .keep_alive(false).max_buf_size(16 * 1024)
                                .serve_connection(TokioIo::new(tls), service).await.map_err(io::Error::other)
                        }).await;
                    });
                }
            }
        };
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
        result
    }

    async fn dispatch(
        &self,
        raft: Raft<C>,
        sender: u64,
        request: Request<Incoming>,
    ) -> Response<Full<Bytes>> {
        if request.method() != Method::POST || request.uri().query().is_some() {
            return status(StatusCode::NOT_FOUND);
        }
        let path = request.uri().path().to_owned();
        if !matches!(path.as_str(), "/raft/v1/append" | "/raft/v1/vote" | "/raft/v1/snapshot") {
            return status(StatusCode::NOT_FOUND);
        }
        if request.headers().get(hyper::header::CONTENT_TYPE).and_then(|v| v.to_str().ok())
            != Some("application/json")
        {
            return status(StatusCode::UNSUPPORTED_MEDIA_TYPE);
        }
        let bytes = match Limited::new(request.into_body(), MAX_BODY).collect().await {
            Ok(body) => body.to_bytes(),
            Err(_) => return status(StatusCode::PAYLOAD_TOO_LARGE),
        };
        // Authenticate the envelope and the Raft candidate/leader before calling Raft.
        macro_rules! handle {
            ($ty:ty, $method:ident) => {{
                let envelope: Envelope<$ty> = match serde_json::from_slice(&bytes) {
                    Ok(v) => v,
                    Err(_) => return status(StatusCode::BAD_REQUEST),
                };
                if envelope.version != WIRE_VERSION || envelope.cluster != self.settings.cluster {
                    return status(StatusCode::CONFLICT);
                }
                if envelope.sender != sender
                    || envelope.recipient != self.settings.local
                    || envelope.payload.vote.leader_id.voted_for() != Some(sender)
                {
                    return status(StatusCode::FORBIDDEN);
                }
                let result = raft.$method(envelope.payload).await;
                match encode(&result) {
                    Ok(body) => Response::new(Full::new(body.into())),
                    Err(_) => status(StatusCode::INTERNAL_SERVER_ERROR),
                }
            }};
        }
        match path.as_str() {
            "/raft/v1/append" => handle!(AppendEntriesRequest<C>, append_entries),
            "/raft/v1/vote" => handle!(VoteRequest<u64>, vote),
            "/raft/v1/snapshot" => handle!(InstallSnapshotRequest<C>, install_snapshot),
            _ => status(StatusCode::NOT_FOUND),
        }
    }

    async fn exchange<T: Serialize, R: DeserializeOwned>(
        &self,
        target: u64,
        path: &str,
        payload: T,
        ttl: Duration,
    ) -> anyhow::Result<R> {
        let bytes = encode(&Envelope {
            version: WIRE_VERSION,
            cluster: self.settings.cluster.clone(),
            sender: self.settings.local,
            recipient: target,
            payload,
        })?;
        let deadline = ttl.min(MAX_LIFETIME);
        tokio::time::timeout(deadline, self.exchange_bytes(target, path, bytes)).await?
    }

    async fn exchange_bytes<R: DeserializeOwned>(
        &self,
        target: u64,
        path: &str,
        bytes: Vec<u8>,
    ) -> anyhow::Result<R> {
        let _permit = self.settings.outgoing.acquire().await?;
        let peer = self
            .settings
            .peers
            .get(&target)
            .filter(|_| target != self.settings.local)
            .ok_or_else(|| invalid("target is not an enrolled remote peer"))?;
        let socket = TcpStream::connect(peer.address).await?;
        let tls = TlsConnector::from(self.settings.client.clone())
            .connect(ServerName::try_from(peer.server_name.clone())?, socket)
            .await?;
        let cert = tls
            .get_ref()
            .1
            .peer_certificates()
            .and_then(|c| c.first())
            .ok_or_else(|| invalid("missing server certificate"))?;
        anyhow::ensure!(fingerprint(cert) == peer.certificate_sha256, "server identity mismatch");
        let (mut sender, connection) =
            hyper::client::conn::http1::handshake(TokioIo::new(tls)).await?;
        let request = Request::builder()
            .method(Method::POST)
            .uri(path)
            .header(hyper::header::HOST, &peer.server_name)
            .header(hyper::header::CONTENT_TYPE, "application/json")
            .header(hyper::header::CONNECTION, "close")
            .body(Full::new(Bytes::from(bytes)))?;
        // Drive HTTP without spawning a detached task: caller cancellation drops
        // the connection, including when OpenRaft cancels an RPC at its deadline.
        let response = async {
            let response = sender.send_request(request).await?;
            anyhow::ensure!(
                response.status() == StatusCode::OK,
                "peer rejected RPC: {}",
                response.status()
            );
            let body = Limited::new(response.into_body(), MAX_BODY)
                .collect()
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?
                .to_bytes();
            Ok(serde_json::from_slice(&body)?)
        };
        tokio::pin!(response);
        tokio::select! {
            result = &mut response => result,
            result = connection => { result?; response.await }
        }
    }

    #[cfg(test)]
    pub(crate) async fn raw_rpc(
        &self,
        target: u64,
        path: &str,
        body: Vec<u8>,
    ) -> anyhow::Result<serde_json::Value> {
        tokio::time::timeout(MAX_LIFETIME, self.exchange_bytes(target, path, body)).await?
    }

    #[cfg(test)]
    pub(crate) fn without_client_certificate(&mut self, ca: CertificateDer<'static>) {
        let mut roots = rustls::RootCertStore::empty();
        roots.add(ca).unwrap();
        let client = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
        Arc::get_mut(&mut self.settings).unwrap().client = Arc::new(client);
    }
}

fn status(code: StatusCode) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::new()));
    *response.status_mut() = code;
    response
}

// Bound serialization as well as reception, before allocating a full body.
fn encode(value: &impl Serialize) -> serde_json::Result<Vec<u8>> {
    struct Bounded(Vec<u8>);
    impl io::Write for Bounded {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if buf.len() > MAX_BODY.saturating_sub(self.0.len()) {
                return Err(invalid("RPC exceeds maximum body size"));
            }
            self.0.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Bounded(Vec::new());
    serde_json::to_writer(&mut writer, value)?;
    Ok(writer.0)
}

pub(crate) struct PeerClient<C> {
    transport: PeerTransport<C>,
    target: u64,
}
impl<C: RaftTypeConfig<NodeId = u64>> RaftNetworkFactory<C> for PeerTransport<C> {
    type Network = PeerClient<C>;
    async fn new_client(&mut self, target: u64, _node: &C::Node) -> Self::Network {
        PeerClient { transport: self.clone(), target }
    }
}
impl<C: RaftTypeConfig<NodeId = u64>> PeerClient<C> {
    async fn rpc<T: Serialize, R: DeserializeOwned, E: std::error::Error + DeserializeOwned>(
        &self,
        path: &str,
        payload: T,
        option: RPCOption,
    ) -> Result<R, RPCError<u64, C::Node, E>> {
        let result: Result<R, E> = self
            .transport
            .exchange(self.target, path, payload, option.hard_ttl())
            .await
            .map_err(|e| NetworkError::new(&io::Error::other(e.to_string())))?;
        result.map_err(|e| RemoteError::new(self.target, e).into())
    }
}
impl<C: RaftTypeConfig<NodeId = u64>> RaftNetwork<C> for PeerClient<C> {
    async fn append_entries(
        &mut self,
        rpc: AppendEntriesRequest<C>,
        option: RPCOption,
    ) -> Result<AppendEntriesResponse<u64>, RPCError<u64, C::Node, RaftError<u64>>> {
        self.rpc("/raft/v1/append", rpc, option).await
    }
    async fn vote(
        &mut self,
        rpc: VoteRequest<u64>,
        option: RPCOption,
    ) -> Result<VoteResponse<u64>, RPCError<u64, C::Node, RaftError<u64>>> {
        self.rpc("/raft/v1/vote", rpc, option).await
    }
    async fn install_snapshot(
        &mut self,
        rpc: InstallSnapshotRequest<C>,
        option: RPCOption,
    ) -> Result<
        InstallSnapshotResponse<u64>,
        RPCError<u64, C::Node, RaftError<u64, InstallSnapshotError>>,
    > {
        self.rpc("/raft/v1/snapshot", rpc, option).await
    }
}
