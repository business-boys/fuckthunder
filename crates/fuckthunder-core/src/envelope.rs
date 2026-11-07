//! RFC 3501 envelope structures returned by `FETCH ENVELOPE`.

use serde::{Deserialize, Serialize};

/// One address inside an envelope address list.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Address {
    /// Display name, if any.
    pub name: Option<String>,
    /// Source route (obsolete, almost always `None`).
    pub adl: Option<String>,
    /// Local part / mailbox.
    pub mailbox: Option<String>,
    /// Host part.
    pub host: Option<String>,
}

impl Address {
    /// Renders the address as `Display Name <local@host>`.
    #[must_use]
    pub fn to_mailbox_string(&self) -> String {
        let local = self.mailbox.as_deref().unwrap_or("");
        let host = self.host.as_deref().unwrap_or("");
        let addr = if host.is_empty() {
            local.to_owned()
        } else {
            format!("{local}@{host}")
        };
        match &self.name {
            Some(name) if !name.is_empty() => format!("{name} <{addr}>"),
            _ => addr,
        }
    }
}

/// A parsed `ENVELOPE` fetch item.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    /// `Date` header.
    pub date: Option<String>,
    /// `Subject` header.
    pub subject: Option<String>,
    /// `From` addresses.
    pub from: Vec<Address>,
    /// `Sender` addresses.
    pub sender: Vec<Address>,
    /// `Reply-To` addresses.
    pub reply_to: Vec<Address>,
    /// `To` addresses.
    pub to: Vec<Address>,
    /// `Cc` addresses.
    pub cc: Vec<Address>,
    /// `Bcc` addresses.
    pub bcc: Vec<Address>,
    /// `In-Reply-To` header.
    pub in_reply_to: Option<String>,
    /// `Message-ID` header.
    pub message_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_rendering() {
        let addr = Address {
            name: Some("Alice".into()),
            mailbox: Some("alice".into()),
            host: Some("example.org".into()),
            ..Address::default()
        };
        assert_eq!(addr.to_mailbox_string(), "Alice <alice@example.org>");

        let bare = Address {
            mailbox: Some("bob".into()),
            ..Address::default()
        };
        assert_eq!(bare.to_mailbox_string(), "bob");
    }

    #[test]
    fn envelope_default_is_empty() {
        let envelope = Envelope::default();
        assert!(envelope.from.is_empty());
        assert!(envelope.subject.is_none());
    }
}
