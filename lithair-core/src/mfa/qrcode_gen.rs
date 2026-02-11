//! QR code generation for TOTP setup
//!
//! Note: QR code generation is handled directly by totp-rs via the 'qr' feature.
//! This module provides convenience wrappers for Lithair's use case.

use super::TotpSecret;
use anyhow::Result;

/// Generate QR code for a TOTP secret
///
/// Uses totp-rs's built-in QR generation (feature 'qr')
/// Returns base64-encoded image
///
/// # Example
/// ```ignore
/// let secret = TotpSecret::generate_with_account(
///     TotpAlgorithm::SHA1, 6, 30, "Lithair", "admin"
/// )?;
/// let qr_base64 = generate_qr_code(&secret)?;
/// // Display in HTML: <img src="data:image/png;base64,{qr_base64}" />
/// ```
pub fn generate_qr_code(secret: &TotpSecret) -> Result<String> {
    // totp-rs handles everything: URI generation + QR code generation
    secret.get_qr_code()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mfa::TotpAlgorithm;

    #[test]
    fn test_qr_code_generation() {
        let secret =
            TotpSecret::generate_with_account(TotpAlgorithm::SHA1, 6, 30, "Lithair", "admin")
                .unwrap();
        let qr = generate_qr_code(&secret).unwrap();

        // totp-rs returns base64-encoded image
        assert!(!qr.is_empty());
    }
}
