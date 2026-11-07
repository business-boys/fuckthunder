//! IMAP wire protocol: tokenizer, response parser and command encoder.
//!
//! This crate contains everything needed to speak IMAP 4rev1 (RFC 3501) on the
//! wire without any I/O:
//!
//! * [`token`] — zero-copy tokenizer for atoms, quoted strings, literals,
//!   lists and response-code groups.
//! * [`response`] — typed parsing of tagged, untagged and continuation
//!   responses, including the extensions the client uses.
//! * [`fetch`] — `ENVELOPE`, `BODYSTRUCTURE` and `BODY[...]` decoding.
//! * [`command`] — command construction with literal framing and injection
//!   guards.
//! * [`search`] — typed `SEARCH` criteria builder.
//! * [`fast`] — allocation-free scanner used by the benchmarks.
//!
//! # Examples
//!
//! ```
//! use fuckthunder_imap_proto::{parse_response, Command, FetchItems, Response, Untagged};
//! use fuckthunder_core::SequenceSet;
//!
//! let response = parse_response(b"* 2 FETCH (UID 41 FLAGS (\\Seen))\r\n").unwrap();
//! match response {
//!     Response::Untagged(Untagged::Fetch(data)) => assert_eq!(data.uid, Some(41)),
//!     _ => unreachable!(),
//! }
//!
//! let command = Command::uid_fetch(&SequenceSet::all(), &FetchItems::summary());
//! let wire = command.encode("a1", false).unwrap().to_bytes();
//! assert_eq!(wire, b"a1 UID FETCH 1:* (UID FLAGS ENVELOPE RFC822.SIZE INTERNALDATE)\r\n");
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod command;
pub mod fast;
pub mod fetch;
pub mod response;
pub mod search;
pub mod token;

pub use command::{Command, EncodedCommand, FetchItems, Piece, StoreAction};
pub use fetch::{
    parse_body_structure, parse_envelope, parse_fetch_items, BodySection, BodyStructure, FetchData,
};
pub use response::{
    parse_response, ListEntry, NamespaceEntry, NamespaceResponse, Response, Untagged,
};
pub use search::SearchQuery;
pub use token::{tokenize, Token, Tokenizer};
