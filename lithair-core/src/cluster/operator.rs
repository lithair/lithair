//! Experimental offline cluster provisioning and inspection. These tools do not
//! start an application or establish quorum/readiness. See OPENRAFT_OPERATOR.md.
use super::{
    durable_log::DurableLog,
    peer_transport::{Peer, PeerTransport},
};
use anyhow::{ensure, Context};
use rustls::{
    client::danger::ServerCertVerifier,
    pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime},
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufReader, Read},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};

openraft::declare_raft_types!(InspectionConfig: D=Value, R=(), Node=Value, SnapshotData=std::io::Cursor<Vec<u8>>);

/// Offline actions. `Provision` explicitly creates a store; the other actions
/// never create, repair or clean consensus storage.
#[derive(Clone, Copy, Debug)]
pub enum OperatorCommand {
    Check,
    Provision,
    Inspect,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Configuration {
    version: u32,
    cluster_id: String,
    node_id: u64,
    bootstrap_node: u64,
    data_dir: PathBuf,
    tls_ca: PathBuf,
    tls_certificate: PathBuf,
    tls_key: PathBuf,
    peers: Vec<ConfiguredPeer>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfiguredPeer {
    node_id: u64,
    address: SocketAddr,
    server_name: String,
    certificate_sha256: String,
}
pub(crate) struct Prepared<C> {
    pub directory: PathBuf,
    pub transport: PeerTransport<C>,
}
fn bounded(path: &Path, limit: u64) -> anyhow::Result<Vec<u8>> {
    ensure!(std::fs::metadata(path)?.is_file(), "expected a regular file");
    let mut file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    ensure!(file.metadata()?.is_file(), "expected a regular file");
    let mut data = Vec::new();
    (&mut file).take(limit + 1).read_to_end(&mut data)?;
    ensure!(data.len() as u64 <= limit, "operator input exceeds size limit");
    Ok(data)
}
fn certificates(path: &Path) -> anyhow::Result<Vec<CertificateDer<'static>>> {
    let bytes = bounded(path, 1024 * 1024)?;
    let mut certs = Vec::new();
    for item in rustls_pemfile::read_all(&mut BufReader::new(bytes.as_slice())) {
        // PEM parser errors may include the offending input line.
        match item.map_err(|_| anyhow::anyhow!("invalid certificate PEM"))? {
            rustls_pemfile::Item::X509Certificate(cert) => certs.push(cert),
            _ => anyhow::bail!("certificate file must contain only certificates"),
        }
    }
    ensure!(!certs.is_empty(), "empty certificate file");
    Ok(certs)
}
fn private_key(path: &Path) -> anyhow::Result<PrivateKeyDer<'static>> {
    let bytes = bounded(path, 64 * 1024)?;
    let mut keys = Vec::new();
    for item in rustls_pemfile::read_all(&mut BufReader::new(bytes.as_slice())) {
        keys.push(match item.map_err(|_| anyhow::anyhow!("invalid private key PEM"))? {
            rustls_pemfile::Item::Pkcs1Key(key) => PrivateKeyDer::Pkcs1(key),
            rustls_pemfile::Item::Pkcs8Key(key) => PrivateKeyDer::Pkcs8(key),
            rustls_pemfile::Item::Sec1Key(key) => PrivateKeyDer::Sec1(key),
            _ => anyhow::bail!("key file must contain exactly one private key"),
        });
    }
    ensure!(keys.len() == 1, "key file must contain exactly one private key");
    keys.pop().ok_or_else(|| anyhow::anyhow!("missing private key"))
}
pub(crate) fn prepare<C: openraft::RaftTypeConfig<NodeId = u64>>(
    config_path: &Path,
) -> anyhow::Result<Prepared<C>> {
    let config_path = config_path.canonicalize().context("configuration path is unavailable")?;
    let directory = config_path.parent().context("configuration has no parent")?;
    let raw = bounded(&config_path, 64 * 1024)?;
    let text = std::str::from_utf8(&raw).context("configuration must be UTF-8")?;
    // TOML diagnostics can echo input lines. Never print configuration contents.
    let config: Configuration = toml::from_str(text).map_err(|_| {
        anyhow::anyhow!("invalid cluster configuration: syntax, type or unknown field")
    })?;
    ensure!(config.version == 1, "unsupported operator configuration version");
    ensure!(config.peers.len() == 3, "exactly three peers are required");
    ensure!(!config.data_dir.as_os_str().is_empty(), "missing data directory");
    let mut peers = BTreeMap::new();
    for configured in config.peers {
        ensure!(
            configured.address.port() != 0
                && !configured.address.ip().is_unspecified()
                && !configured.address.ip().is_multicast(),
            "invalid peer dial address"
        );
        let pin: [u8; 32] = hex::decode(&configured.certificate_sha256)
            .ok()
            .and_then(|v| v.try_into().ok())
            .context("certificate fingerprint must be 64 hexadecimal digits")?;
        let old = peers.insert(
            configured.node_id,
            Peer {
                address: configured.address,
                server_name: configured.server_name,
                certificate_sha256: pin,
            },
        );
        ensure!(old.is_none(), "duplicate peer ID");
    }
    let chain = certificates(&directory.join(config.tls_certificate))?;
    let trust = certificates(&directory.join(config.tls_ca))?;
    let key = private_key(&directory.join(config.tls_key))?;
    let own = peers.get(&config.node_id).context("local node is not enrolled")?;
    let mut roots = rustls::RootCertStore::empty();
    for cert in &trust {
        roots.add(cert.clone())?;
    }
    let roots = Arc::new(roots);
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = rustls::client::WebPkiServerVerifier::builder_with_provider(
        roots.clone(),
        provider.clone(),
    )
    .build()?;
    let leaf = chain.first().context("missing local certificate")?;
    verifier
        .verify_server_cert(
            leaf,
            &chain[1..],
            &ServerName::try_from(own.server_name.clone())?,
            &[],
            UnixTime::now(),
        )
        .context("local server certificate does not match configured trust, name or validity")?;
    rustls::server::WebPkiClientVerifier::builder_with_provider(roots, provider)
        .build()?
        .verify_client_cert(leaf, &chain[1..], UnixTime::now())
        .context("local certificate is not valid for peer client authentication")?;
    let transport = PeerTransport::new(
        config.cluster_id,
        config.node_id,
        config.bootstrap_node,
        peers,
        trust,
        chain,
        key,
    )
    .context("invalid peer TLS configuration or enrollment")?;
    Ok(Prepared { directory: directory.join(config.data_dir), transport })
}

/// Execute one offline action using paths resolved against the configuration's
/// directory. Errors contain no private-key or configuration file contents.
/// Inspection validates opaque snapshot integrity, not application semantics.
pub async fn run(command: OperatorCommand, config_path: impl AsRef<Path>) -> anyhow::Result<Value> {
    let path = config_path.as_ref().to_owned();
    let prepared =
        tokio::task::spawn_blocking(move || prepare::<InspectionConfig>(&path)).await??;
    let identity = prepared.transport.node_identity()?;
    let mut report = json!({"configuration_version":1,"cluster_id":identity.cluster_id,
        "node_id":identity.node_id,"bootstrap_node":identity.bootstrap_node,
        "peer_ids":identity.voters.keys().copied().collect::<Vec<_>>(),
        "plan_sha256":hex::encode(identity.plan_digest()?),"data_dir":prepared.directory,
        "action":match command {OperatorCommand::Check=>"check",OperatorCommand::Provision=>"provision",OperatorCommand::Inspect=>"inspect"}});
    match command {
        OperatorCommand::Check => {}
        OperatorCommand::Provision => {
            let directory = prepared.directory.clone();
            tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                let parent = directory.parent().context("data directory has no parent")?;
                match std::fs::create_dir(&directory) {
                    Ok(()) => {}
                    Err(error)
                        if error.kind() == std::io::ErrorKind::AlreadyExists
                            && directory.is_dir() => {}
                    Err(error) => return Err(error.into()),
                }
                File::open(parent)?.sync_all()?;
                File::open(directory)?.sync_all()?;
                Ok(())
            })
            .await??;
            let store =
                DurableLog::<InspectionConfig>::create_for_node(&prepared.directory, identity)
                    .await?;
            // Provisioning deliberately does not claim bootstrap or start Raft.
            drop(store);
            report["provisioned"] = json!(true);
        }
        OperatorCommand::Inspect => {
            report["store"] =
                DurableLog::<InspectionConfig>::inspect_for_node(&prepared.directory, identity)
                    .await?;
        }
    }
    Ok(report)
}
