//! Connection configuration and session state.

use std::time::Duration;

use fuckthunder_core::{FlagSet, MailboxName};

use crate::transport::TlsMode;

/// Connection state of an IMAP session (RFC 3501 §3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Before authentication.
    NotAuthenticated,
    /// Authenticated but no mailbox selected.
    Authenticated,
    /// A mailbox is selected.
    Selected(MailboxName),
    /// `LOGOUT` has completed.
    Logout,
}

impl State {
    /// Human-readable state name used in error messages.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::NotAuthenticated => "not-authenticated",
            Self::Authenticated => "authenticated",
            Self::Selected(_) => "selected",
            Self::Logout => "logout",
        }
    }
}

/// Protocol used to establish a proxy tunnel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProxyProtocol {
    /// SOCKS5 with remote DNS resolution (RFC 1928).
    Socks5,
    /// HTTP CONNECT (RFC 9110).
    HttpConnect,
}

/// Proxy tunnel settings.
#[derive(Clone)]
pub struct ProxyConfig {
    /// Proxy host name or address.
    pub host: String,
    /// Proxy port.
    pub port: u16,
    /// Optional username/password authentication.
    pub auth: Option<(String, String)>,
    /// Protocol used to establish the tunnel.
    pub protocol: ProxyProtocol,
}

impl std::fmt::Debug for ProxyConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProxyConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("auth", &self.auth.as_ref().map(|_| "<redacted>"))
            .field("protocol", &self.protocol)
            .finish()
    }
}

/// Identity presented by default in the post-login `ID` command.
///
/// Mirrors what Thunderbird 128 sends.
#[must_use]
pub fn default_identity() -> Vec<(String, String)> {
    vec![
        ("name".to_string(), "Thunderbird".to_string()),
        ("version".to_string(), "128.5.0esr".to_string()),
    ]
}

/// Configuration for opening an IMAP connection.
#[derive(Clone, Debug)]
pub struct ClientConfig {
    /// Server host name (also used for certificate validation).
    pub host: String,
    /// Server port.
    pub port: u16,
    /// Transport security mode.
    pub tls: TlsMode,
    /// Per-operation timeout.
    pub timeout: Duration,
    /// Largest literal the client will accept, in bytes.
    pub max_literal: usize,
    /// Optional proxy the connection is tunnelled through.
    pub proxy: Option<ProxyConfig>,
    /// `ID` fields sent after login (RFC 2971). `None` disables it.
    pub identity: Option<Vec<(String, String)>>,
    /// Reproduce Thunderbird's exact post-login command rhythm
    /// (`ID` -> `NAMESPACE` -> `ENABLE` -> `SELECT INBOX`).
    pub thunderbird_rhythm: bool,
}

impl ClientConfig {
    /// Implicit TLS on port 993.
    #[must_use]
    pub fn tls(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port: 993,
            tls: TlsMode::Implicit,
            timeout: Duration::from_secs(30),
            max_literal: 64 * 1024 * 1024,
            proxy: None,
            identity: Some(default_identity()),
            thunderbird_rhythm: false,
        }
    }

    /// `STARTTLS` on port 143.
    #[must_use]
    pub fn starttls(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port: 143,
            tls: TlsMode::StartTls,
            timeout: Duration::from_secs(30),
            max_literal: 64 * 1024 * 1024,
            proxy: None,
            identity: Some(default_identity()),
            thunderbird_rhythm: false,
        }
    }

    /// Plain TCP (tests and loopback servers only).
    #[must_use]
    pub fn plaintext(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            tls: TlsMode::Plain,
            timeout: Duration::from_secs(10),
            max_literal: 16 * 1024 * 1024,
            proxy: None,
            identity: Some(default_identity()),
            thunderbird_rhythm: false,
        }
    }

    /// Overrides the timeout.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Overrides the literal limit.
    #[must_use]
    pub fn with_max_literal(mut self, bytes: usize) -> Self {
        self.max_literal = bytes;
        self
    }

    /// Enables Thunderbird's exact post-login command rhythm
    /// (`ID` -> `NAMESPACE` -> `ENABLE` -> `SELECT INBOX`), so the wire
    /// pattern matches a real Thunderbird session.
    #[must_use]
    pub fn with_thunderbird_rhythm(mut self) -> Self {
        self.thunderbird_rhythm = true;
        self
    }

    /// Overrides the `ID` fields sent after login (RFC 2971).
    #[must_use]
    pub fn with_identity<I, K, V>(mut self, fields: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.identity = Some(
            fields
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        );
        self
    }

    /// Disables the post-login `ID` command entirely.
    #[must_use]
    pub fn without_identity(mut self) -> Self {
        self.identity = None;
        self
    }

    /// Tunnels the connection through a SOCKS5 proxy without authentication.
    #[must_use]
    pub fn with_socks5(mut self, host: impl Into<String>, port: u16) -> Self {
        self.proxy = Some(ProxyConfig {
            host: host.into(),
            port,
            auth: None,
            protocol: ProxyProtocol::Socks5,
        });
        self
    }

    /// Tunnels the connection through a SOCKS5 proxy with username/password
    /// authentication (RFC 1929).
    #[must_use]
    pub fn with_socks5_auth(
        mut self,
        host: impl Into<String>,
        port: u16,
        user: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        self.proxy = Some(ProxyConfig {
            host: host.into(),
            port,
            auth: Some((user.into(), password.into())),
            protocol: ProxyProtocol::Socks5,
        });
        self
    }

    /// Tunnels the connection through an HTTP proxy with `CONNECT`.
    #[must_use]
    pub fn with_http_connect(mut self, host: impl Into<String>, port: u16) -> Self {
        self.proxy = Some(ProxyConfig {
            host: host.into(),
            port,
            auth: None,
            protocol: ProxyProtocol::HttpConnect,
        });
        self
    }

    /// Tunnels the connection through an HTTP proxy with `CONNECT` and Basic
    /// authentication.
    #[must_use]
    pub fn with_http_connect_auth(
        mut self,
        host: impl Into<String>,
        port: u16,
        user: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        self.proxy = Some(ProxyConfig {
            host: host.into(),
            port,
            auth: Some((user.into(), password.into())),
            protocol: ProxyProtocol::HttpConnect,
        });
        self
    }
}

/// State captured by `SELECT`/`EXAMINE`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SelectInfo {
    /// `EXISTS` count.
    pub exists: u32,
    /// `RECENT` count.
    pub recent: u32,
    /// First unseen message sequence number.
    pub unseen: Option<u32>,
    /// Mailbox UID validity.
    pub uid_validity: Option<u32>,
    /// Predicted next UID.
    pub uid_next: Option<u32>,
    /// Flags currently in use.
    pub flags: FlagSet,
    /// Flags the client may store.
    pub permanent_flags: Option<FlagSet>,
    /// `true` for `EXAMINE` or `[READ-ONLY]`.
    pub read_only: bool,
    /// Highest modification sequence (RFC 7162).
    pub highest_mod_seq: Option<u64>,
}

impl SelectInfo {
    /// Returns the mailbox name that was selected.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} messages, {} recent, unseen={:?}, uidvalidity={:?}",
            self.exists, self.recent, self.unseen, self.uid_validity
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults() {
        let tls = ClientConfig::tls("imap.example.org");
        assert_eq!(tls.port, 993);
        assert_eq!(tls.tls, TlsMode::Implicit);
        let starttls = ClientConfig::starttls("imap.example.org");
        assert_eq!(starttls.port, 143);
        assert_eq!(starttls.tls, TlsMode::StartTls);
        let plain = ClientConfig::plaintext("127.0.0.1", 1430)
            .with_timeout(Duration::from_secs(1))
            .with_max_literal(1024);
        assert_eq!(plain.max_literal, 1024);
    }

    #[test]
    fn state_names() {
        assert_eq!(State::NotAuthenticated.name(), "not-authenticated");
        assert_eq!(State::Authenticated.name(), "authenticated");
        assert_eq!(
            State::Selected(MailboxName::from_utf8("INBOX").unwrap()).name(),
            "selected"
        );
        assert_eq!(State::Logout.name(), "logout");
    }
}
