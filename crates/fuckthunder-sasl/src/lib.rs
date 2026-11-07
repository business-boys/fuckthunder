//! SASL client mechanisms used by IMAP `AUTHENTICATE`.
//!
//! The trait is deliberately tiny so the IMAP client can drive it without
//! knowing which mechanism is in use:
//!
//! ```
//! use fuckthunder_sasl::{Credentials, Mechanism, Plain};
//!
//! let mut mech = Plain::new(&Credentials::password("alice", "secret"));
//! assert_eq!(mech.initial_response().unwrap(), b"\0alice\0secret");
//! ```
//!
//! Supported mechanisms:
//!
//! * `PLAIN` (RFC 4616)
//! * `LOGIN` (draft-murchison-sasl-login)
//! * `CRAM-MD5` (RFC 2195)
//! * `XOAUTH2` (Google/Microsoft bearer-token flow)
//! * `OAUTHBEARER` (RFC 7628)
//! * `ANONYMOUS` (RFC 4505)
//! * `EXTERNAL` (RFC 4422)

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod mechanisms;

pub use mechanisms::{Anonymous, CramMd5, External, Login, OAuthBearer, Plain, XOAuth2};

use fuckthunder_core::{Error, Result};

/// Credentials passed to a mechanism factory.
#[derive(Clone, Debug, Default)]
pub struct Credentials {
    /// Authentication identity (usually the e-mail address).
    pub user: String,
    /// Password, when the mechanism uses one.
    pub password: Option<String>,
    /// Bearer token, when the mechanism uses OAuth.
    pub token: Option<String>,
    /// Optional authorization identity.
    pub authzid: Option<String>,
    /// Host name for `OAUTHBEARER`.
    pub host: Option<String>,
    /// Port for `OAUTHBEARER`.
    pub port: Option<u16>,
}

impl Credentials {
    /// Password credentials.
    #[must_use]
    pub fn password(user: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            user: user.into(),
            password: Some(password.into()),
            ..Self::default()
        }
    }

    /// OAuth bearer-token credentials.
    #[must_use]
    pub fn bearer(user: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            user: user.into(),
            token: Some(token.into()),
            ..Self::default()
        }
    }
}

/// A client-side SASL mechanism.
pub trait Mechanism: Send {
    /// Mechanism name as advertised by the server (`PLAIN`, `XOAUTH2`, ...).
    fn name(&self) -> &str;

    /// Initial client response, if the mechanism supports one.
    fn initial_response(&mut self) -> Option<Vec<u8>>;

    /// Processes a server challenge and produces the next client response.
    ///
    /// # Errors
    ///
    /// Fails when the challenge is malformed or the exchange is out of order.
    fn step(&mut self, challenge: &[u8]) -> Result<Option<Vec<u8>>>;

    /// Returns `true` once the mechanism has produced its final response.
    fn is_complete(&self) -> bool;
}

/// Builds a mechanism by name from the server's `AUTH=...` capability list.
///
/// # Errors
///
/// Returns [`Error::Unsupported`] for mechanisms this crate does not implement
/// and [`Error::Auth`] when the credentials are missing the required secret.
pub fn mechanism(name: &str, credentials: &Credentials) -> Result<Box<dyn Mechanism>> {
    let upper = name.to_ascii_uppercase();
    Ok(match upper.as_str() {
        "PLAIN" => Box::new(Plain::new(credentials)),
        "LOGIN" => Box::new(Login::new(credentials)),
        "CRAM-MD5" => Box::new(CramMd5::new(credentials)),
        "XOAUTH2" => Box::new(XOAuth2::new(credentials)),
        "OAUTHBEARER" => Box::new(OAuthBearer::new(credentials)),
        "ANONYMOUS" => Box::new(Anonymous::new(credentials)),
        "EXTERNAL" => Box::new(External::new(credentials)),
        other => {
            return Err(Error::Unsupported(format!(
                "SASL mechanism {other} is not implemented"
            )))
        }
    })
}

/// Decodes a BASE64 challenge sent by the server.
///
/// # Errors
///
/// Returns [`Error::Auth`] when the challenge is not valid BASE64.
pub fn decode_challenge(challenge: &str) -> Result<Vec<u8>> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(challenge.trim())
        .map_err(|e| Error::Auth(format!("invalid SASL challenge: {e}")))
}

/// Encodes a client response for the wire.
#[must_use]
pub fn encode_response(response: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_selects_mechanisms() {
        let creds = Credentials::password("u", "p");
        assert_eq!(mechanism("plain", &creds).unwrap().name(), "PLAIN");
        assert_eq!(mechanism("CRAM-MD5", &creds).unwrap().name(), "CRAM-MD5");
        assert!(mechanism("GSSAPI", &creds).is_err());
        let bearer = Credentials::bearer("u", "t");
        assert_eq!(mechanism("XOAUTH2", &bearer).unwrap().name(), "XOAUTH2");
    }

    #[test]
    fn base64_round_trip() {
        let encoded = encode_response(b"hello");
        assert_eq!(decode_challenge(&encoded).unwrap(), b"hello");
        assert!(decode_challenge("!!!").is_err());
    }
}
