//! PKCE (RFC 7636) helpers.

use base64::Engine as _;
use rand::RngCore;
use sha2::{Digest, Sha256};

/// A PKCE verifier/challenge pair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pkce {
    /// High-entropy verifier sent on token exchange.
    pub verifier: String,
    /// `BASE64URL(SHA256(verifier))` sent on the authorization request.
    pub challenge: String,
}

impl Pkce {
    /// Generates a fresh verifier (64 random bytes) and its S256 challenge.
    ///
    /// ```
    /// use fuckthunder_oauth::Pkce;
    /// let pkce = Pkce::generate();
    /// assert!(pkce.verify(&pkce.verifier));
    /// assert!(!pkce.verify("wrong"));
    /// ```
    #[must_use]
    pub fn generate() -> Self {
        Self::from_verifier(&random_url_token(64))
    }

    /// Derives the challenge for an existing verifier.
    #[must_use]
    pub fn from_verifier(verifier: &str) -> Self {
        Self {
            verifier: verifier.to_owned(),
            challenge: sha256_base64url(verifier),
        }
    }

    /// Verifies a verifier against this challenge.
    #[must_use]
    pub fn verify(&self, verifier: &str) -> bool {
        sha256_base64url(verifier) == self.challenge
    }
}

/// Generates a BASE64URL (unpadded) random token.
#[must_use]
pub fn random_url_token(byte_len: usize) -> String {
    let mut bytes = vec![0u8; byte_len];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Returns `BASE64URL(SHA256(input))` without padding.
#[must_use]
pub fn sha256_base64url(input: &str) -> String {
    let digest = Sha256::digest(input.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc7636_vector() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let pkce = Pkce::from_verifier(verifier);
        assert_eq!(
            pkce.challenge,
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert!(pkce.verify(verifier));
        assert!(!pkce.verify("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXj"));
    }

    #[test]
    fn tokens_are_url_safe_and_unique() {
        let a = random_url_token(32);
        let b = random_url_token(32);
        assert_ne!(a, b);
        assert!(!a.contains('+') && !a.contains('/') && !a.contains('='));
    }

    #[test]
    fn generated_pairs_verify() {
        let pkce = Pkce::generate();
        assert!(pkce.verify(&pkce.verifier));
        assert_eq!(pkce.verifier.len(), 86);
    }
}
