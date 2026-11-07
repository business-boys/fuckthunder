//! IMAP response parsing (RFC 3501 §7).
//!
//! [`parse_response`] turns one complete server response — a line, plus any
//! literal octets it announced — into a typed [`Response`]. Untagged responses
//! are dispatched into the extensions this workspace supports: `CAPABILITY`,
//! `LIST`/`LSUB`, `STATUS`, `SEARCH`, `FETCH`, `NAMESPACE`, `ENABLED`, `ID`,
//! `QUOTA`, `FLAGS`, `EXISTS`, `RECENT`, `EXPUNGE`, and the status responses
//! `OK`/`NO`/`BAD`/`PREAUTH`/`BYE`.

use fuckthunder_core::{
    CapabilitySet, FlagSet, MailboxName, ResponseCode, StatusData, TaggedStatus,
};

use crate::fetch::FetchData;

/// A namespace entry from `* NAMESPACE`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamespaceEntry {
    /// Namespace prefix.
    pub prefix: MailboxName,
    /// Hierarchy delimiter, `None` when the server uses flat namespaces.
    pub delimiter: Option<char>,
}

/// Parsed `* NAMESPACE` response.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NamespaceResponse {
    /// Personal namespaces.
    pub personal: Vec<NamespaceEntry>,
    /// Other users' namespaces.
    pub other_users: Vec<NamespaceEntry>,
    /// Shared namespaces.
    pub shared: Vec<NamespaceEntry>,
}

/// One entry of a `LIST`/`LSUB`/`XLIST` response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListEntry {
    /// Attributes such as `\HasNoChildren`, `\Sent`, `\Noselect`.
    pub attributes: Vec<String>,
    /// Hierarchy delimiter.
    pub delimiter: Option<char>,
    /// Mailbox name.
    pub name: MailboxName,
}

impl ListEntry {
    /// Returns `true` when the attribute list contains `attribute`
    /// (case-insensitive).
    #[must_use]
    pub fn has_attribute(&self, attribute: &str) -> bool {
        self.attributes
            .iter()
            .any(|a| a.eq_ignore_ascii_case(attribute))
    }

    /// Returns `true` when the mailbox can be selected.
    #[must_use]
    pub fn is_selectable(&self) -> bool {
        !self.has_attribute("\\Noselect") && !self.has_attribute("\\NonExistent")
    }

    /// Returns the RFC 6154 special-use attribute, if present.
    #[must_use]
    pub fn special_use(&self) -> Option<&str> {
        self.attributes
            .iter()
            .find(|a| {
                matches!(
                    a.to_ascii_lowercase().as_str(),
                    "\\all"
                        | "\\archive"
                        | "\\drafts"
                        | "\\flagged"
                        | "\\junk"
                        | "\\sent"
                        | "\\trash"
                        | "\\important"
                )
            })
            .map(String::as_str)
    }
}

/// An untagged (`*`) response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Untagged {
    /// `* CAPABILITY ...`
    Capability(CapabilitySet),
    /// `* LIST ...`
    List(ListEntry),
    /// `* LSUB ...`
    Lsub(ListEntry),
    /// `* STATUS mailbox (items)`
    Status {
        /// Mailbox the status refers to.
        mailbox: MailboxName,
        /// Parsed status values.
        data: StatusData,
    },
    /// `* SEARCH 1 2 3`
    Search(Vec<u32>),
    /// `* FLAGS (...)`
    Flags(FlagSet),
    /// `* n EXISTS`
    Exists(u32),
    /// `* n RECENT`
    Recent(u32),
    /// `* n EXPUNGE`
    Expunge(u32),
    /// `* n FETCH (...)`
    Fetch(Box<FetchData>),
    /// `* OK ...`
    Ok {
        /// Optional response code.
        code: Option<ResponseCode>,
        /// Human-readable text.
        text: String,
    },
    /// `* NO ...`
    No {
        /// Optional response code.
        code: Option<ResponseCode>,
        /// Human-readable text.
        text: String,
    },
    /// `* BAD ...`
    Bad {
        /// Optional response code.
        code: Option<ResponseCode>,
        /// Human-readable text.
        text: String,
    },
    /// `* PREAUTH ...`
    PreAuth {
        /// Optional response code.
        code: Option<ResponseCode>,
        /// Human-readable text.
        text: String,
    },
    /// `* BYE ...`
    Bye {
        /// Optional response code.
        code: Option<ResponseCode>,
        /// Human-readable text.
        text: String,
    },
    /// `* NAMESPACE ...`
    Namespace(NamespaceResponse),
    /// `* ENABLED ...`
    Enabled(Vec<String>),
    /// `* ID (...)`
    Id(Vec<(String, Option<String>)>),
    /// `* QUOTA root (resource usage limit)`
    Quota {
        /// Quota root name.
        root: String,
        /// Resource name, e.g. `STORAGE`.
        resource: String,
        /// Current usage.
        usage: u64,
        /// Configured limit.
        limit: u64,
    },
    /// Any untagged response this crate does not model.
    Other {
        /// Leading atom, e.g. `ESEARCH`.
        atom: String,
        /// Remaining response text, lossily decoded.
        text: String,
    },
}

/// One parsed server response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Response {
    /// A tagged completion response (`a1 OK ...`).
    Tagged {
        /// Command tag.
        tag: String,
        /// Completion status.
        status: TaggedStatus,
        /// Optional response code.
        code: Option<ResponseCode>,
        /// Human-readable text.
        text: String,
    },
    /// An untagged response (`* ...`).
    Untagged(Untagged),
    /// A continuation request (`+ ...`).
    Continuation {
        /// Optional response code.
        code: Option<ResponseCode>,
        /// Human-readable text.
        text: String,
    },
}

impl Response {
    /// Returns the tagged status when this is a tagged completion.
    #[must_use]
    pub fn tagged_status(&self) -> Option<TaggedStatus> {
        match self {
            Self::Tagged { status, .. } => Some(*status),
            _ => None,
        }
    }

    /// Returns the tag when this is a tagged completion.
    #[must_use]
    pub fn tag(&self) -> Option<&str> {
        match self {
            Self::Tagged { tag, .. } => Some(tag),
            _ => None,
        }
    }
}

mod parse;

pub use parse::parse_response;

#[cfg(test)]
mod tests;
