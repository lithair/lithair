//! Ephemeral test CA and node certificates; never used by application code.
use crate::peer_transport::{fingerprint, Peer, PeerTransport};
use openraft_memstore::TypeConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, net::SocketAddr};

#[derive(Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub ca: Vec<u8>,
    pub certs: BTreeMap<u64, Vec<u8>>,
    pub keys: BTreeMap<u64, Vec<u8>>,
}
impl Credentials {
    pub fn generate() -> Self {
        use rcgen::{
            BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
            KeyUsagePurpose,
        };
        let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let key = KeyPair::generate().unwrap();
        let ca = params.self_signed(&key).unwrap();
        let issuer = Issuer::new(params, key);
        let mut certs = BTreeMap::new();
        let mut keys = BTreeMap::new();
        // Node 4 is CA-signed but not enrolled in the three-member fixture.
        for id in 1..=4 {
            let mut params = CertificateParams::new(vec![format!("node{id}.test")]).unwrap();
            params.extended_key_usages =
                vec![ExtendedKeyUsagePurpose::ServerAuth, ExtendedKeyUsagePurpose::ClientAuth];
            let key = KeyPair::generate().unwrap();
            let cert = params.signed_by(&key, &issuer).unwrap();
            certs.insert(id, cert.der().to_vec());
            keys.insert(id, key.serialize_der());
        }
        Self { ca: ca.der().to_vec(), certs, keys }
    }
    pub fn peers(&self, addresses: &BTreeMap<u64, SocketAddr>) -> BTreeMap<u64, Peer> {
        addresses
            .iter()
            .map(|(id, address)| {
                (
                    *id,
                    Peer {
                        address: *address,
                        server_name: format!("node{id}.test"),
                        certificate_sha256: fingerprint(&self.certs[id]),
                    },
                )
            })
            .collect()
    }
    pub fn transport(
        &self,
        id: u64,
        cluster: &str,
        peers: BTreeMap<u64, Peer>,
    ) -> PeerTransport<TypeConfig> {
        PeerTransport::new(
            cluster.into(),
            id,
            1,
            peers,
            vec![CertificateDer::from(self.ca.clone())],
            vec![CertificateDer::from(self.certs[&id].clone())],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.keys[&id].clone())),
        )
        .unwrap()
    }
}
