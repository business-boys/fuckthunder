//! Convenience constructors for every command the client issues.

use fuckthunder_core::{
    quote_if_needed, FlagSet, ImapDateTime, MailboxName, Result, SequenceSet, StatusItem,
};

use crate::search::SearchQuery;

use super::items::{FetchItems, StoreAction};
use super::Command;

impl Command {
    // ---- Convenience constructors -------------------------------------

    /// `CAPABILITY`
    #[must_use]
    pub fn capability() -> Self {
        Self::new("CAPABILITY")
    }

    /// `NOOP`
    #[must_use]
    pub fn noop() -> Self {
        Self::new("NOOP")
    }

    /// `LOGOUT`
    #[must_use]
    pub fn logout() -> Self {
        Self::new("LOGOUT")
    }

    /// `STARTTLS`
    #[must_use]
    pub fn starttls() -> Self {
        Self::new("STARTTLS")
    }

    /// `COMPRESS DEFLATE`
    #[must_use]
    pub fn compress_deflate() -> Self {
        Self::new("COMPRESS").with_raw("DEFLATE")
    }

    /// `IDLE`
    #[must_use]
    pub fn idle() -> Self {
        Self::new("IDLE")
    }

    /// `LOGIN "user" "password"`
    ///
    /// # Errors
    ///
    /// Fails when the credentials contain CR/LF.
    pub fn login(user: &str, password: &str) -> Result<Self> {
        let mut command = Self::new("LOGIN");
        command.quoted(user)?.quoted(password)?;
        Ok(command)
    }

    /// `AUTHENTICATE MECH [initial-response]`
    ///
    /// # Errors
    ///
    /// Fails when the mechanism or initial response is not a valid atom.
    pub fn authenticate(mechanism: &str, initial: Option<&str>) -> Result<Self> {
        let mut command = Self::new("AUTHENTICATE");
        command.atom(mechanism)?;
        if let Some(initial) = initial {
            command.atom(initial)?;
        }
        Ok(command)
    }

    /// `SELECT` or `EXAMINE`.
    #[must_use]
    pub fn select(mailbox: &MailboxName, examine: bool) -> Self {
        let name = if examine { "EXAMINE" } else { "SELECT" };
        Self::new(name).with_raw(mailbox.to_wire_token())
    }

    /// `UNSELECT` (RFC 3691).
    #[must_use]
    pub fn unselect() -> Self {
        Self::new("UNSELECT")
    }

    /// `CLOSE`
    #[must_use]
    pub fn close() -> Self {
        Self::new("CLOSE")
    }

    /// `CHECK`
    #[must_use]
    pub fn check() -> Self {
        Self::new("CHECK")
    }

    /// `ENABLE capability ...` (RFC 5161).
    #[must_use]
    pub fn enable(capabilities: &[&str]) -> Self {
        Self::new("ENABLE").with_raw(capabilities.join(" "))
    }

    /// `NAMESPACE` (RFC 2342).
    #[must_use]
    pub fn namespace() -> Self {
        Self::new("NAMESPACE")
    }

    /// `ID (...)` (RFC 2971); `NIL` when the map is empty.
    #[must_use]
    pub fn id(fields: &[(&str, &str)]) -> Self {
        if fields.is_empty() {
            return Self::new("ID").with_raw("NIL");
        }
        let mut rendered = String::from("(");
        for (index, (key, value)) in fields.iter().enumerate() {
            if index > 0 {
                rendered.push(' ');
            }
            rendered.push_str(&quote_always(key));
            rendered.push(' ');
            rendered.push_str(&quote_always(value));
        }
        rendered.push(')');
        Self::new("ID").with_raw(rendered)
    }

    /// `LIST reference pattern`
    #[must_use]
    pub fn list(reference: &str, pattern: &str) -> Self {
        Self::new("LIST").with_raw(format!(
            "{} {}",
            quote_if_needed(reference),
            quote_if_needed(pattern)
        ))
    }

    /// `LSUB reference pattern`
    #[must_use]
    pub fn lsub(reference: &str, pattern: &str) -> Self {
        Self::new("LSUB").with_raw(format!(
            "{} {}",
            quote_if_needed(reference),
            quote_if_needed(pattern)
        ))
    }

    /// `STATUS mailbox (items)`
    #[must_use]
    pub fn status(mailbox: &MailboxName, items: &[StatusItem]) -> Self {
        let list = items
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" ");
        Self::new("STATUS").with_raw(format!("{} ({list})", mailbox.to_wire_token()))
    }

    /// `CREATE mailbox`
    #[must_use]
    pub fn create(mailbox: &MailboxName) -> Self {
        Self::new("CREATE").with_raw(mailbox.to_wire_token())
    }

    /// `DELETE mailbox`
    #[must_use]
    pub fn delete(mailbox: &MailboxName) -> Self {
        Self::new("DELETE").with_raw(mailbox.to_wire_token())
    }

    /// `RENAME old new`
    #[must_use]
    pub fn rename(old: &MailboxName, new: &MailboxName) -> Self {
        Self::new("RENAME").with_raw(format!("{} {}", old.to_wire_token(), new.to_wire_token()))
    }

    /// `SUBSCRIBE mailbox`
    #[must_use]
    pub fn subscribe(mailbox: &MailboxName) -> Self {
        Self::new("SUBSCRIBE").with_raw(mailbox.to_wire_token())
    }

    /// `UNSUBSCRIBE mailbox`
    #[must_use]
    pub fn unsubscribe(mailbox: &MailboxName) -> Self {
        Self::new("UNSUBSCRIBE").with_raw(mailbox.to_wire_token())
    }

    /// `APPEND mailbox (flags) date literal`
    #[must_use]
    pub fn append(
        mailbox: &MailboxName,
        flags: Option<&FlagSet>,
        date: Option<ImapDateTime>,
        body: Vec<u8>,
    ) -> Self {
        let mut command = Self::new("APPEND").with_raw(mailbox.to_wire_token());
        if let Some(flags) = flags {
            command = command.with_raw(flags.to_parenthesised());
        }
        if let Some(date) = date {
            command = command.with_raw(format!("\"{}\"", date.to_wire()));
        }
        command.literal(body);
        command
    }

    /// `FETCH set items`
    #[must_use]
    pub fn fetch(set: &SequenceSet, items: &FetchItems) -> Self {
        Self::new("FETCH").with_raw(format!("{set} {}", items.to_wire()))
    }

    /// `UID FETCH set items`
    #[must_use]
    pub fn uid_fetch(set: &SequenceSet, items: &FetchItems) -> Self {
        Self::new("UID FETCH").with_raw(format!("{set} {}", items.to_wire()))
    }

    /// `STORE set action flags` (optionally `.SILENT`)
    #[must_use]
    pub fn store(set: &SequenceSet, action: StoreAction, flags: &FlagSet, silent: bool) -> Self {
        let suffix = if silent { ".SILENT" } else { "" };
        Self::new("STORE").with_raw(format!(
            "{set} {action}{suffix} {}",
            flags.to_parenthesised()
        ))
    }

    /// `UID STORE ...`
    #[must_use]
    pub fn uid_store(
        set: &SequenceSet,
        action: StoreAction,
        flags: &FlagSet,
        silent: bool,
    ) -> Self {
        let suffix = if silent { ".SILENT" } else { "" };
        Self::new("UID STORE").with_raw(format!(
            "{set} {action}{suffix} {}",
            flags.to_parenthesised()
        ))
    }

    /// `SEARCH criteria`
    #[must_use]
    pub fn search(query: &SearchQuery) -> Self {
        Self::new("SEARCH").with_raw(query.to_wire())
    }

    /// `UID SEARCH criteria`
    #[must_use]
    pub fn uid_search(query: &SearchQuery) -> Self {
        Self::new("UID SEARCH").with_raw(query.to_wire())
    }

    /// `COPY set mailbox`
    #[must_use]
    pub fn copy(set: &SequenceSet, mailbox: &MailboxName) -> Self {
        Self::new("COPY").with_raw(format!("{set} {}", mailbox.to_wire_token()))
    }

    /// `UID COPY set mailbox`
    #[must_use]
    pub fn uid_copy(set: &SequenceSet, mailbox: &MailboxName) -> Self {
        Self::new("UID COPY").with_raw(format!("{set} {}", mailbox.to_wire_token()))
    }

    /// `MOVE set mailbox` (RFC 6851)
    #[must_use]
    pub fn move_to(set: &SequenceSet, mailbox: &MailboxName) -> Self {
        Self::new("MOVE").with_raw(format!("{set} {}", mailbox.to_wire_token()))
    }

    /// `UID MOVE set mailbox`
    #[must_use]
    pub fn uid_move(set: &SequenceSet, mailbox: &MailboxName) -> Self {
        Self::new("UID MOVE").with_raw(format!("{set} {}", mailbox.to_wire_token()))
    }

    /// `EXPUNGE`
    #[must_use]
    pub fn expunge() -> Self {
        Self::new("EXPUNGE")
    }

    /// `UID EXPUNGE set` (RFC 4315)
    #[must_use]
    pub fn uid_expunge(set: &SequenceSet) -> Self {
        Self::new("UID EXPUNGE").with_raw(set.to_string())
    }

    fn with_raw(mut self, text: impl Into<String>) -> Self {
        self.raw(text);
        self
    }
}

fn quote_always(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        if ch == '"' || ch == '\\' {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('"');
    out
}
