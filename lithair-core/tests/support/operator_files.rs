use rcgen::{
    BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug)]
pub struct OperatorFiles {
    pub root: tempfile::TempDir,
    pub configs: BTreeMap<u64, PathBuf>,
}
impl OperatorFiles {
    pub fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let key = KeyPair::generate().unwrap();
        let ca = params.self_signed(&key).unwrap();
        std::fs::write(root.path().join("ca.pem"), ca.pem()).unwrap();
        let issuer = Issuer::new(params, key);
        let mut pins = BTreeMap::new();
        for id in 1..=3 {
            let directory = root.path().join(id.to_string());
            std::fs::create_dir(&directory).unwrap();
            let mut params = CertificateParams::new(vec![format!("node{id}.test")]).unwrap();
            params.extended_key_usages =
                vec![ExtendedKeyUsagePurpose::ServerAuth, ExtendedKeyUsagePurpose::ClientAuth];
            let key = KeyPair::generate().unwrap();
            let cert = params.signed_by(&key, &issuer).unwrap();
            std::fs::write(directory.join("cert.pem"), cert.pem()).unwrap();
            std::fs::write(directory.join("key.pem"), key.serialize_pem()).unwrap();
            let pin = Sha256::digest(cert.der())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            pins.insert(id, pin);
        }
        let mut configs = BTreeMap::new();
        for id in 1..=3 {
            let mut body = format!("version = 1\ncluster_id = \"operator-test\"\nnode_id = {id}\nbootstrap_node = 1\ndata_dir = \"raft\"\ntls_ca = \"../ca.pem\"\ntls_certificate = \"cert.pem\"\ntls_key = \"key.pem\"\n");
            for (peer, pin) in &pins {
                body.push_str(&format!("\n[[peers]]\nnode_id = {peer}\naddress = \"127.0.0.1:{}\"\nserver_name = \"node{peer}.test\"\ncertificate_sha256 = \"{pin}\"\n", 20000+peer));
            }
            let path = root.path().join(id.to_string()).join("node.toml");
            std::fs::write(&path, body).unwrap();
            configs.insert(id, path);
        }
        Self { root, configs }
    }
    pub fn data(&self, id: u64) -> PathBuf {
        self.root.path().join(id.to_string()).join("raft")
    }
}
