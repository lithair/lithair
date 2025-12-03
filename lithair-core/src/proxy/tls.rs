//! TLS fingerprinting and certificate validation
//!
//! Provides utilities for extracting and validating TLS certificate fingerprints.
//! Used for advanced filtering based on server certificates (SHA-256, SHA-1).

use sha2::{Digest as Sha2Digest, Sha256};
use sha1::Sha1;
use rustls::pki_types::CertificateDer;

/// TLS certificate fingerprint
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CertificateFingerprint {
    /// SHA-256 fingerprint (lowercase hex)
    pub sha256: String,
    /// SHA-1 fingerprint (lowercase hex)
    pub sha1: String,
    /// Raw certificate DER bytes
    pub der: Vec<u8>,
}

impl CertificateFingerprint {
    /// Create fingerprint from certificate DER bytes
    pub fn from_der(der: &[u8]) -> Self {
        let sha256 = {
            let mut hasher = Sha256::new();
            hasher.update(der);
            let result = hasher.finalize();
            hex::encode(result).to_lowercase()
        };

        let sha1 = {
            let mut hasher = Sha1::new();
            hasher.update(der);
            let result = hasher.finalize();
            hex::encode(result).to_lowercase()
        };

        Self {
            sha256,
            sha1,
            der: der.to_vec(),
        }
    }

    /// Create fingerprint from rustls certificate
    pub fn from_rustls_cert(cert: &CertificateDer<'_>) -> Self {
        Self::from_der(cert.as_ref())
    }

    /// Get SHA-256 fingerprint with colons (standard format)
    /// Example: "ab:cd:ef:12:34:56:..."
    pub fn sha256_formatted(&self) -> String {
        self.sha256
            .as_bytes()
            .chunks(2)
            .map(|chunk| std::str::from_utf8(chunk).unwrap_or(""))
            .collect::<Vec<_>>()
            .join(":")
    }

    /// Get SHA-1 fingerprint with colons (standard format)
    /// Example: "ab:cd:ef:12:34:56:..."
    pub fn sha1_formatted(&self) -> String {
        self.sha1
            .as_bytes()
            .chunks(2)
            .map(|chunk| std::str::from_utf8(chunk).unwrap_or(""))
            .collect::<Vec<_>>()
            .join(":")
    }
}

/// TLS fingerprinter for extracting certificate information
pub struct TlsFingerprinter;

impl TlsFingerprinter {
    /// Extract fingerprints from a certificate chain
    ///
    /// # Arguments
    /// * `certs` - Certificate chain (first is server cert)
    ///
    /// # Returns
    /// Fingerprint of the server certificate (first in chain)
    pub fn extract_from_chain(certs: &[CertificateDer<'_>]) -> Option<CertificateFingerprint> {
        certs.first().map(CertificateFingerprint::from_rustls_cert)
    }

    /// Validate fingerprint against a blocklist
    ///
    /// # Arguments
    /// * `fingerprint` - The fingerprint to check
    /// * `blocklist_sha256` - Set of blocked SHA-256 fingerprints (lowercase hex)
    /// * `blocklist_sha1` - Set of blocked SHA-1 fingerprints (lowercase hex)
    ///
    /// # Returns
    /// `true` if the certificate should be blocked
    pub fn is_blocked(
        fingerprint: &CertificateFingerprint,
        blocklist_sha256: &std::collections::HashSet<String>,
        blocklist_sha1: &std::collections::HashSet<String>,
    ) -> bool {
        blocklist_sha256.contains(&fingerprint.sha256)
            || blocklist_sha1.contains(&fingerprint.sha1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fingerprint_from_der() {
        let fake_cert = b"fake certificate data for testing";
        let fingerprint = CertificateFingerprint::from_der(fake_cert);

        // SHA-256 should be 64 chars (32 bytes in hex)
        assert_eq!(fingerprint.sha256.len(), 64);
        // SHA-1 should be 40 chars (20 bytes in hex)
        assert_eq!(fingerprint.sha1.len(), 40);
        // Should be lowercase
        assert_eq!(fingerprint.sha256, fingerprint.sha256.to_lowercase());
        assert_eq!(fingerprint.sha1, fingerprint.sha1.to_lowercase());
    }

    #[test]
    fn test_formatted_fingerprints() {
        let fake_cert = b"test";
        let fingerprint = CertificateFingerprint::from_der(fake_cert);

        let sha256_formatted = fingerprint.sha256_formatted();
        let sha1_formatted = fingerprint.sha1_formatted();

        // Should contain colons
        assert!(sha256_formatted.contains(':'));
        assert!(sha1_formatted.contains(':'));

        // Remove colons and compare to original
        let sha256_clean: String = sha256_formatted.chars().filter(|c| *c != ':').collect();
        let sha1_clean: String = sha1_formatted.chars().filter(|c| *c != ':').collect();

        assert_eq!(sha256_clean, fingerprint.sha256);
        assert_eq!(sha1_clean, fingerprint.sha1);
    }

    #[test]
    fn test_is_blocked() {
        let fake_cert = b"blocked certificate";
        let fingerprint = CertificateFingerprint::from_der(fake_cert);

        let mut blocklist_sha256 = std::collections::HashSet::new();
        let blocklist_sha1 = std::collections::HashSet::new();

        // Not blocked initially
        assert!(!TlsFingerprinter::is_blocked(&fingerprint, &blocklist_sha256, &blocklist_sha1));

        // Add to blocklist
        blocklist_sha256.insert(fingerprint.sha256.clone());

        // Now blocked
        assert!(TlsFingerprinter::is_blocked(&fingerprint, &blocklist_sha256, &blocklist_sha1));
    }
}
