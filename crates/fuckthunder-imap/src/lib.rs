//! Async IMAP client built on safe Rust.
//!
//! The client is split into three layers:
//!
//! * [`transport`] — plain TCP, implicit TLS, `STARTTLS` and DEFLATE.
//! * [`connection`] — response framing, literal continuations, tagged command
//!   execution and SASL exchanges.
//! * [`ImapClient`] — typed mailbox operations (`SELECT`, `LIST`, `FETCH`,
//!   `APPEND`, `STORE`, `SEARCH`, `IDLE`, `NAMESPACE`, ...).
//!
//! # Examples
//!
//! ```no_run
//! use fuckthunder_imap::{ClientConfig, ImapClient};
//! use fuckthunder_core::MailboxName;
//!
//! # async fn run() -> fuckthunder_core::Result<()> {
//! let config = ClientConfig::tls("imap.example.org");
//! let mut client = ImapClient::connect(config).await?;
//! client.login("alice@example.org", "app-password").await?;
//!
//! let inbox = MailboxName::from_utf8("INBOX").unwrap();
//! let info = client.select(&inbox, false).await?;
//! println!("{}", info.summary());
//!
//! for entry in client.list("", "*").await? {
//!     println!("{}", entry.name);
//! }
//! client.logout().await?;
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod client;
pub mod connection;
pub mod transport;
pub mod types;

pub use client::ImapClient;
pub use connection::{CommandOutcome, Connection};
pub use transport::{client_config, TlsMode, Transport};
pub use types::{ClientConfig, ProxyConfig, SelectInfo, State};
