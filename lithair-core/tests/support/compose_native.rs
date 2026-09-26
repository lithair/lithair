//! Native application qualification, alongside the lower-level consensus fixture.
use super::{
    address,
    credentials::Credentials,
    durable_log::{DurableLog, NodeIdentity},
};
use base64::Engine;
use lithair_core::{
    app::LithairServer,
    cluster::native::{Model, NativeCluster},
    DeclarativeModel,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::Write};
use tokio::net::TcpListener;

#[derive(Clone, Serialize, Deserialize, DeclarativeModel)]
struct Record {
    #[db(primary_key)]
    id: String,
    #[db(unique)]
    email: String,
    #[http(validate = "non_empty")]
    name: String,
    #[serde(default)]
    left: u64,
    #[serde(default)]
    right: u64,
}
fn pem(kind: &str, data: &[u8]) -> String {
    format!(
        "-----BEGIN {kind}-----\n{}\n-----END {kind}-----\n",
        base64::engine::general_purpose::STANDARD.encode(data)
    )
}
pub async fn provision(id: u64, credentials: &Credentials) -> anyhow::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let path = format!("/secrets/{id}/native");
    std::fs::create_dir(&path)?;
    for (name, data) in [
        ("ca.pem", pem("CERTIFICATE", &credentials.ca)),
        ("cert.pem", pem("CERTIFICATE", &credentials.certs[&id])),
        ("key.pem", pem("PRIVATE KEY", &credentials.keys[&id])),
    ] {
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(format!("{path}/{name}"))?;
        file.write_all(data.as_bytes())?;
        file.sync_all()?;
    }
    let mut config=format!("version = 1\ncluster_id = \"compose-native\"\nnode_id = {id}\nbootstrap_node = 1\ndata_dir = \"/data/native\"\ntls_ca = \"ca.pem\"\ntls_certificate = \"cert.pem\"\ntls_key = \"key.pem\"\n");
    let mut voters = BTreeMap::new();
    for peer in 1..=3 {
        let pin = super::fingerprint(&credentials.certs[&peer]);
        voters.insert(peer, pin);
        let ip = std::env::var(format!("LITHAIR_CLUSTER_NODE{peer}_IP"))?;
        config.push_str(&format!("\n[[peers]]\nnode_id = {peer}\naddress = \"{ip}:9553\"\nserver_name = \"node{peer}.test\"\ncertificate_sha256 = \"{}\"\n",hex::encode(pin)));
    }
    std::fs::write(format!("{path}/node.toml"), config)?;
    let directory = format!("/stores/{id}/native");
    std::fs::create_dir(&directory)?;
    std::fs::File::open(format!("/stores/{id}"))?.sync_all()?;
    drop(
        DurableLog::<openraft_memstore::TypeConfig>::create_for_node(
            directory,
            NodeIdentity {
                cluster_id: "compose-native".into(),
                node_id: id,
                bootstrap_node: 1,
                voters,
            },
        )
        .await?,
    );
    Ok(())
}
pub async fn open() -> anyhow::Result<NativeCluster> {
    NativeCluster::open(
        "/secrets/native/node.toml",
        "compose-native-v1",
        vec![Model::of::<Record>("site.records", "/api/records")?],
    )
    .await
}
pub async fn serve(id: u64, cluster: NativeCluster) -> anyhow::Result<()> {
    let listener = TcpListener::bind(address(&format!("control{id}"), 8180).await?).await?;
    LithairServer::new()
        .with_admin_panel(false)
        .with_data_dir("/data/app")
        .with_native_cluster(cluster)
        .build()?
        .serve_with_listener(listener, std::future::pending())
        .await
}
