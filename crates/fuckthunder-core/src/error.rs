//! The workspace-wide error type.
//!
//! Every crate in the workspace returns [`Result<T>`], so callers never have to
//! juggle three different error enums when a session spans TLS, SASL, OAuth and
//! the IMAP state machine.

use crate::response_code::ResponseCode;

/// Convenient alias used by all public APIs.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors produced while speaking IMAP, SASL or OAuth.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Transport failure (socket, TLS record layer, compression).
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    /// TLS handshake or certificate validation failure.
    #[error("tls error: {0}")]
    Tls(String),

    /// The peer violated the protocol grammar.
    #[error("protocol error: {0}")]
    Protocol(String),

    /// A response or command could not be parsed.
    #[error("parse error: {0}")]
    Parse(String),

    /// Authentication failed or a SASL mechanism is unusable.
    #[error("authentication failed: {0}")]
    Auth(String),

    /// The command is illegal in the current connection state.
    #[error("invalid state: command requires {expected}, connection is {actual}")]
    State {
        /// State required by the command.
        expected: &'static str,
        /// State the connection is currently in.
        actual: &'static str,
    },

    /// The server answered `NO` or `BAD` to a tagged command.
    #[error("server rejected command ({status}): {text}")]
    Server {
        /// Tagged status keyword (`NO` or `BAD`).
        status: crate::response_code::TaggedStatus,
        /// Optional machine-readable response code.
        code: Option<ResponseCode>,
        /// Human-readable diagnostic text.
        text: String,
    },

    /// OAuth 2.0 discovery, exchange or refresh failure.
    #[error("oauth error: {0}")]
    OAuth(String),

    /// The operation exceeded its deadline.
    #[error("operation timed out")]
    Timeout,

    /// The server closed the connection (or sent `BYE`).
    #[error("connection closed: {0}")]
    Closed(String),

    /// A configured resource limit was exceeded.
    #[error("limit exceeded: {0}")]
    Limit(String),

    /// The server does not advertise a capability required for this call.
    #[error("unsupported by server: {0}")]
    Unsupported(String),

    /// Input validation failed before anything was sent on the wire.
    #[error("invalid input: {0}")]
    Invalid(String),
}

impl Error {
    /// Returns `true` when the error is worth retrying on a fresh connection.
    ///
    /// Network, timeout and `BYE` failures are transient; parse errors and
    /// invalid input are not.
    #[must_use]
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Error::Io(_) | Error::Timeout | Error::Closed(_) | Error::Tls(_)
        )
    }

    /// Returns the response code when the server attached one to its refusal.
    #[must_use]
    pub fn response_code(&self) -> Option<&ResponseCode> {
        match self {
            Error::Server { code, .. } => code.as_ref(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_classification() {
        assert!(Error::Timeout.is_transient());
        assert!(Error::Closed("bye".into()).is_transient());
        assert!(!Error::Parse("bad".into()).is_transient());
        assert!(!Error::Invalid("x".into()).is_transient());
    }

    #[test]
    fn server_error_exposes_code() {
        let err = Error::Server {
            status: crate::response_code::TaggedStatus::No,
            code: Some(ResponseCode::TryCreate),
            text: "no such mailbox".into(),
        };
        assert!(matches!(err.response_code(), Some(ResponseCode::TryCreate)));
        assert!(err.to_string().contains("no such mailbox"));
    }
}
