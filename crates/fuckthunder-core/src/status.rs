//! `STATUS` items and their values.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// A single `STATUS` item name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatusItem {
    /// `MESSAGES`
    Messages,
    /// `RECENT`
    Recent,
    /// `UIDNEXT`
    UidNext,
    /// `UIDVALIDITY`
    UidValidity,
    /// `UNSEEN`
    Unseen,
    /// `SIZE` (RFC 8438)
    Size,
    /// `HIGHESTMODSEQ` (RFC 7162)
    HighestModSeq,
}

impl StatusItem {
    /// Parses a status item atom.
    #[must_use]
    pub fn parse(atom: &str) -> Option<Self> {
        Some(match atom.to_ascii_uppercase().as_str() {
            "MESSAGES" => Self::Messages,
            "RECENT" => Self::Recent,
            "UIDNEXT" => Self::UidNext,
            "UIDVALIDITY" => Self::UidValidity,
            "UNSEEN" => Self::Unseen,
            "SIZE" => Self::Size,
            "HIGHESTMODSEQ" => Self::HighestModSeq,
            _ => return None,
        })
    }
}

impl std::fmt::Display for StatusItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Messages => "MESSAGES",
            Self::Recent => "RECENT",
            Self::UidNext => "UIDNEXT",
            Self::UidValidity => "UIDVALIDITY",
            Self::Unseen => "UNSEEN",
            Self::Size => "SIZE",
            Self::HighestModSeq => "HIGHESTMODSEQ",
        })
    }
}

/// Values returned by `STATUS`. Absent items stay `None`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusData {
    /// Number of messages.
    pub messages: Option<u32>,
    /// Number of recent messages.
    pub recent: Option<u32>,
    /// Predicted next UID.
    pub uid_next: Option<u32>,
    /// Mailbox UID validity.
    pub uid_validity: Option<u32>,
    /// Number of unseen messages.
    pub unseen: Option<u32>,
    /// Total mailbox size in octets (RFC 8438).
    pub size: Option<u64>,
    /// Highest modification sequence (RFC 7162).
    pub highest_mod_seq: Option<u64>,
}

impl StatusData {
    /// Stores a value for one item.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Parse`] when the value does not fit the item's domain.
    pub fn set(&mut self, item: StatusItem, value: u64) -> Result<()> {
        let narrow = |v: u64| {
            u32::try_from(v).map_err(|_| Error::Parse(format!("{item} value {v} exceeds u32")))
        };
        match item {
            StatusItem::Messages => self.messages = Some(narrow(value)?),
            StatusItem::Recent => self.recent = Some(narrow(value)?),
            StatusItem::UidNext => self.uid_next = Some(narrow(value)?),
            StatusItem::UidValidity => self.uid_validity = Some(narrow(value)?),
            StatusItem::Unseen => self.unseen = Some(narrow(value)?),
            StatusItem::Size => self.size = Some(value),
            StatusItem::HighestModSeq => self.highest_mod_seq = Some(value),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_parsing_is_case_insensitive() {
        assert_eq!(StatusItem::parse("messages"), Some(StatusItem::Messages));
        assert_eq!(
            StatusItem::parse("HIGHESTMODSEQ"),
            Some(StatusItem::HighestModSeq)
        );
        assert_eq!(StatusItem::parse("BOGUS"), None);
        assert_eq!(StatusItem::UidNext.to_string(), "UIDNEXT");
    }

    #[test]
    fn stores_values_and_rejects_overflow() {
        let mut data = StatusData::default();
        data.set(StatusItem::Messages, 12).unwrap();
        data.set(StatusItem::Size, u64::MAX).unwrap();
        assert_eq!(data.messages, Some(12));
        assert_eq!(data.size, Some(u64::MAX));
        assert!(data.set(StatusItem::Messages, u64::MAX).is_err());
    }
}
