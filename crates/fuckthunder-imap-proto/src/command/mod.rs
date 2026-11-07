//! Command construction and wire encoding.
//!
//! A [`Command`] is a list of segments. Text segments are written verbatim,
//! literal segments are framed with `{n}` (or `{n+}` for `LITERAL+`) so the
//! connection layer can drive the continuation handshake without string
//! surgery.

/// One piece of an encoded command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece {
    /// Raw bytes to write immediately.
    Bytes(Vec<u8>),
    /// A literal payload with its framing.
    Literal {
        /// The literal octets.
        data: Vec<u8>,
        /// `true` when `{n+}` (LITERAL+) was used and no continuation is needed.
        non_sync: bool,
    },
}

/// An encoded command, ready to be streamed to the socket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedCommand {
    /// Ordered pieces.
    pub pieces: Vec<Piece>,
}

impl EncodedCommand {
    /// Returns `true` when the command contains a synchronising literal and the
    /// server must send a continuation request before the literal is sent.
    #[must_use]
    pub fn needs_continuation(&self) -> bool {
        self.pieces.iter().any(|piece| {
            matches!(
                piece,
                Piece::Literal {
                    non_sync: false,
                    ..
                }
            )
        })
    }

    /// Concatenates every piece, useful for tests and small commands.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for piece in &self.pieces {
            match piece {
                Piece::Bytes(bytes) => out.extend_from_slice(bytes),
                Piece::Literal { data, .. } => out.extend_from_slice(data),
            }
        }
        out
    }
}

/// A single IMAP command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    name: String,
    segments: Vec<Segment>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Segment {
    Text(String),
    Literal(Vec<u8>),
}

mod build;
mod items;
mod ops;

pub use items::{FetchItems, StoreAction};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::SearchQuery;
    use fuckthunder_core::{FlagSet, ImapDateTime, MailboxName, SequenceSet, StatusItem};

    fn mailbox(name: &str) -> MailboxName {
        MailboxName::from_utf8(name).unwrap()
    }

    #[test]
    fn encodes_simple_commands() {
        let encoded = Command::capability().encode("a1", false).unwrap();
        assert_eq!(encoded.to_bytes(), b"a1 CAPABILITY\r\n");
        assert!(!encoded.needs_continuation());
    }

    #[test]
    fn encodes_login_with_quotes() {
        let encoded = Command::login("alice", "p\"ss")
            .unwrap()
            .encode("a2", false)
            .unwrap();
        assert_eq!(encoded.to_bytes(), b"a2 LOGIN alice \"p\\\"ss\"\r\n");
    }

    #[test]
    fn rejects_injection() {
        assert!(Command::login("a\r\nb", "x").is_err());
        assert!(Command::new("X").atom("a b").is_err());
        assert!(Command::new("X").encode("bad tag", false).is_err());
    }

    #[test]
    fn literal_framing() {
        let command = Command::append(&mailbox("INBOX"), None, None, b"hello".to_vec());
        let encoded = command.encode("a3", false).unwrap();
        assert!(encoded.needs_continuation());
        assert_eq!(encoded.to_bytes(), b"a3 APPEND INBOX {5}\r\nhello\r\n");
    }

    #[test]
    fn literal_plus_framing() {
        let command = Command::append(&mailbox("INBOX"), None, None, b"hi".to_vec());
        let encoded = command.encode("a4", true).unwrap();
        assert!(!encoded.needs_continuation());
        assert_eq!(encoded.to_bytes(), b"a4 APPEND INBOX {2+}\r\nhi\r\n");
    }

    #[test]
    fn append_includes_flags_and_date() {
        let flags = FlagSet::parse(["\\Seen"]);
        let date = ImapDateTime::parse("17-Jul-1996 02:44:25 -0700").unwrap();
        let command = Command::append(&mailbox("INBOX"), Some(&flags), Some(date), b"x".to_vec());
        let encoded = command.encode("a5", false).unwrap().to_bytes();
        assert_eq!(
            encoded,
            b"a5 APPEND INBOX (\\Seen) \"17-Jul-1996 02:44:25 -0700\" {1}\r\nx\r\n"
        );
    }

    #[test]
    fn fetch_and_store() {
        let set = SequenceSet::all();
        let items = FetchItems::summary();
        assert_eq!(
            Command::uid_fetch(&set, &items)
                .encode("a6", false)
                .unwrap()
                .to_bytes(),
            b"a6 UID FETCH 1:* (UID FLAGS ENVELOPE RFC822.SIZE INTERNALDATE)\r\n"
        );
        let flags = FlagSet::parse(["\\Seen"]);
        assert_eq!(
            Command::store(&set, StoreAction::Add, &flags, true)
                .encode("a7", false)
                .unwrap()
                .to_bytes(),
            b"a7 STORE 1:* +FLAGS.SILENT (\\Seen)\r\n"
        );
    }

    #[test]
    fn search_commands() {
        let query = SearchQuery::all().and(SearchQuery::unseen());
        assert_eq!(
            Command::uid_search(&query)
                .encode("a8", false)
                .unwrap()
                .to_bytes(),
            b"a8 UID SEARCH ALL UNSEEN\r\n"
        );
    }

    #[test]
    fn status_and_enable() {
        let command = Command::status(
            &mailbox("INBOX"),
            &[StatusItem::Messages, StatusItem::Unseen],
        );
        assert_eq!(
            command.encode("a9", false).unwrap().to_bytes(),
            b"a9 STATUS INBOX (MESSAGES UNSEEN)\r\n"
        );
        assert_eq!(
            Command::enable(&["UTF8=ACCEPT", "CONDSTORE"])
                .encode("a10", false)
                .unwrap()
                .to_bytes(),
            b"a10 ENABLE UTF8=ACCEPT CONDSTORE\r\n"
        );
    }

    #[test]
    fn id_command() {
        assert_eq!(
            Command::id(&[("name", "fuckthunder")])
                .encode("a11", false)
                .unwrap()
                .to_bytes(),
            b"a11 ID (\"name\" \"fuckthunder\")\r\n"
        );
        assert_eq!(
            Command::id(&[]).encode("a12", false).unwrap().to_bytes(),
            b"a12 ID NIL\r\n"
        );
    }
}
