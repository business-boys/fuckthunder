//! Domain types shared by every `fuckthunder-*` crate.
//!
//! This crate is deliberately free of I/O and of any `unsafe` code. It holds
//! the vocabulary of IMAP (RFC 3501 and its extensions) and OAuth 2.0 that the
//! protocol and client crates exchange:
//!
//! * [`MailboxName`] — modified UTF-7 mailbox names with correct quoting.
//! * [`FlagSet`] — system flags and keywords.
//! * [`SequenceSet`] — `1:5,7,9:*` message sets.
//! * [`CapabilitySet`] — server capabilities with typed helpers.
//! * [`ResponseCode`] — the `[CODE ...]` payload of tagged/untagged responses.
//! * [`Envelope`] — parsed RFC 3501 envelope for fetched messages.
//! * [`Error`] — the single error type used across the workspace.
//!
//! # Examples
//!
//! ```
//! use fuckthunder_core::{MailboxName, SequenceSet};
//!
//! let box_name = MailboxName::from_utf8("Отправленные").unwrap();
//! assert_eq!(box_name.to_wire(), "&BB4EQgQ,BEAEMAQyBDsENQQ9BD0ESwQ1-");
//!
//! let set: SequenceSet = "1:3,7".parse().unwrap();
//! assert_eq!(set.to_string(), "1:3,7");
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod capability;
pub mod datetime;
pub mod envelope;
pub mod error;
pub mod flags;
pub mod mailbox;
pub mod response_code;
pub mod seq;
pub mod status;

pub use capability::CapabilitySet;
pub use datetime::ImapDateTime;
pub use envelope::{Address, Envelope};
pub use error::{Error, Result};
pub use flags::{Flag, FlagSet};
pub use mailbox::{needs_quoting, quote_if_needed, MailboxName};
pub use response_code::{ResponseCode, TaggedStatus};
pub use seq::{Bound, SequenceSet};
pub use status::{StatusData, StatusItem};
