//! High-level IMAP client.

use std::time::Duration;

use fuckthunder_core::{
    CapabilitySet, Error, FlagSet, MailboxName, ResponseCode, Result, SequenceSet, StatusData,
    StatusItem,
};
use fuckthunder_imap_proto::{
    Command, FetchData, FetchItems, ListEntry, NamespaceResponse, Response, SearchQuery,
    StoreAction, Untagged,
};
use fuckthunder_sasl::{Credentials, Mechanism};
use tokio::time::Instant;

use crate::connection::{CommandOutcome, Connection};
use crate::transport::TlsMode;
use crate::types::{ClientConfig, SelectInfo, State};

/// A connected IMAP client.
///
/// ```
/// # async fn demo() -> fuckthunder_core::Result<()> {
/// use fuckthunder_imap::{ClientConfig, ImapClient};
/// let config = ClientConfig::plaintext("127.0.0.1", 1430);
/// let mut client = ImapClient::connect(config).await?;
/// client.login("alice", "secret").await?;
/// let mailboxes = client.list("", "*").await?;
/// assert!(mailboxes.is_empty() || !mailboxes.is_empty());
/// client.logout().await?;
/// # Ok(())
/// # }
/// ```
pub struct ImapClient {
    connection: Connection,
    identity: Option<Vec<(String, String)>>,
}

impl ImapClient {
    /// Opens the connection, performs `STARTTLS` when configured, and reads
    /// capabilities.
    ///
    /// # Errors
    ///
    /// Fails on transport, TLS, or protocol errors.
    pub async fn connect(config: ClientConfig) -> Result<Self> {
        let mut connection = Connection::open(&config).await?;
        if config.tls == TlsMode::StartTls {
            connection.start_tls().await?;
        }
        connection.capability().await?;
        Ok(Self {
            connection,
            identity: config.identity.clone(),
        })
    }

    /// Returns the negotiated capabilities.
    #[must_use]
    pub fn capabilities(&self) -> &CapabilitySet {
        self.connection.capabilities()
    }

    /// Returns the session state.
    #[must_use]
    pub fn state(&self) -> &State {
        self.connection.state()
    }

    /// Returns the server greeting.
    #[must_use]
    pub fn greeting(&self) -> &str {
        self.connection.greeting()
    }

    /// Returns `true` when the connection is encrypted.
    #[must_use]
    pub fn is_tls(&self) -> bool {
        self.connection.is_tls()
    }

    /// Mutable access to the underlying connection.
    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.connection
    }

    /// Re-reads capabilities.
    ///
    /// # Errors
    ///
    /// Fails on transport or protocol errors.
    pub async fn capability(&mut self) -> Result<CapabilitySet> {
        self.connection.capability().await
    }

    /// Authenticates with `LOGIN`.
    ///
    /// # Errors
    ///
    /// Fails when the server rejects the credentials.
    pub async fn login(&mut self, user: &str, password: &str) -> Result<()> {
        self.connection.login(user, password).await?;
        self.maybe_id().await
    }

    /// Authenticates with an explicit SASL mechanism.
    ///
    /// # Errors
    ///
    /// Fails when the exchange is rejected.
    pub async fn authenticate(&mut self, mechanism: &mut dyn Mechanism) -> Result<()> {
        self.connection.authenticate(mechanism).await?;
        self.maybe_id().await
    }

    /// Authenticates with an OAuth bearer token, preferring `OAUTHBEARER` and
    /// falling back to `XOAUTH2`.
    ///
    /// # Errors
    ///
    /// Fails when neither mechanism is advertised or the server rejects the
    /// token.
    pub async fn authenticate_oauth2(&mut self, user: &str, token: &str) -> Result<()> {
        let mechanisms = self.capabilities().auth_mechanisms();
        let name = if mechanisms
            .iter()
            .any(|m| m.eq_ignore_ascii_case("OAUTHBEARER"))
        {
            "OAUTHBEARER"
        } else if mechanisms.iter().any(|m| m.eq_ignore_ascii_case("XOAUTH2")) {
            "XOAUTH2"
        } else {
            return Err(Error::Unsupported(
                "server advertises neither OAUTHBEARER nor XOAUTH2".into(),
            ));
        };
        let credentials = Credentials::bearer(user, token);
        let mut mechanism = fuckthunder_sasl::mechanism(name, &credentials)?;
        self.authenticate(mechanism.as_mut()).await
    }

    /// Sends `ID` (RFC 2971) and returns the server's own `ID` reply, if any.
    ///
    /// # Errors
    ///
    /// Fails when the server refuses the command.
    pub async fn id(&mut self, fields: &[(&str, &str)]) -> Result<Vec<(String, Option<String>)>> {
        let outcome = self.connection.execute(Command::id(fields)).await?;
        for response in &outcome.untagged {
            if let Response::Untagged(Untagged::Id(fields)) = response {
                return Ok(fields.clone());
            }
        }
        Ok(Vec::new())
    }

    /// Sends the configured `ID` after authentication when the server
    /// advertises the extension (mirrors Thunderbird's behaviour).
    async fn maybe_id(&mut self) -> Result<()> {
        let Some(fields) = self.identity.clone() else {
            return Ok(());
        };
        if !self.capabilities().has("ID") {
            return Ok(());
        }
        let refs: Vec<(&str, &str)> = fields
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        self.id(&refs).await?;
        Ok(())
    }

    /// `SELECT` or `EXAMINE`.
    ///
    /// # Errors
    ///
    /// Fails when the mailbox does not exist or the server refuses access.
    pub async fn select(&mut self, mailbox: &MailboxName, examine: bool) -> Result<SelectInfo> {
        let outcome = self
            .connection
            .execute(Command::select(mailbox, examine))
            .await?;
        let mut info = SelectInfo {
            read_only: examine,
            ..SelectInfo::default()
        };
        apply_select_codes(&mut info, &outcome);
        for response in &outcome.untagged {
            if let Response::Untagged(untagged) = response {
                match untagged {
                    Untagged::Exists(count) => info.exists = *count,
                    Untagged::Recent(count) => info.recent = *count,
                    Untagged::Flags(flags) => info.flags = flags.clone(),
                    _ => {}
                }
            }
        }
        self.connection.set_selected(mailbox.clone());
        Ok(info)
    }

    /// Lists mailboxes matching an IMAP pattern.
    ///
    /// # Errors
    ///
    /// Fails when the server refuses the command.
    pub async fn list(&mut self, reference: &str, pattern: &str) -> Result<Vec<ListEntry>> {
        let outcome = self
            .connection
            .execute(Command::list(reference, pattern))
            .await?;
        Ok(collect_list(outcome, false))
    }

    /// Lists subscribed mailboxes.
    ///
    /// # Errors
    ///
    /// Fails when the server refuses the command.
    pub async fn lsub(&mut self, reference: &str, pattern: &str) -> Result<Vec<ListEntry>> {
        let outcome = self
            .connection
            .execute(Command::lsub(reference, pattern))
            .await?;
        Ok(collect_list(outcome, true))
    }

    /// Requests `STATUS` for a mailbox.
    ///
    /// # Errors
    ///
    /// Fails when the mailbox does not exist.
    pub async fn status(
        &mut self,
        mailbox: &MailboxName,
        items: &[StatusItem],
    ) -> Result<StatusData> {
        let outcome = self
            .connection
            .execute(Command::status(mailbox, items))
            .await?;
        for response in &outcome.untagged {
            if let Response::Untagged(Untagged::Status { data, .. }) = response {
                return Ok(data.clone());
            }
        }
        Err(Error::Protocol("STATUS response missing".into()))
    }

    /// `FETCH` by message sequence numbers.
    ///
    /// # Errors
    ///
    /// Fails when the server refuses the command.
    pub async fn fetch(&mut self, set: &SequenceSet, items: &FetchItems) -> Result<Vec<FetchData>> {
        let outcome = self.connection.execute(Command::fetch(set, items)).await?;
        Ok(collect_fetch(outcome))
    }

    /// `UID FETCH`.
    ///
    /// # Errors
    ///
    /// Fails when the server refuses the command.
    pub async fn uid_fetch(
        &mut self,
        set: &SequenceSet,
        items: &FetchItems,
    ) -> Result<Vec<FetchData>> {
        let outcome = self
            .connection
            .execute(Command::uid_fetch(set, items))
            .await?;
        Ok(collect_fetch(outcome))
    }

    /// `APPEND` with an optional flag set and internal date.
    ///
    /// # Errors
    ///
    /// Fails when the server refuses the message.
    pub async fn append(
        &mut self,
        mailbox: &MailboxName,
        flags: Option<&FlagSet>,
        date: Option<fuckthunder_core::ImapDateTime>,
        body: Vec<u8>,
    ) -> Result<Option<u32>> {
        let outcome = self
            .connection
            .execute(Command::append(mailbox, flags, date, body))
            .await?;
        Ok(match outcome.code {
            Some(ResponseCode::AppendUid { uid, .. }) => Some(uid),
            _ => None,
        })
    }

    /// `STORE` by sequence number.
    ///
    /// # Errors
    ///
    /// Fails when the server refuses the command.
    pub async fn store(
        &mut self,
        set: &SequenceSet,
        action: StoreAction,
        flags: &FlagSet,
        silent: bool,
    ) -> Result<Vec<FetchData>> {
        let outcome = self
            .connection
            .execute(Command::store(set, action, flags, silent))
            .await?;
        Ok(collect_fetch(outcome))
    }

    /// `UID STORE`.
    ///
    /// # Errors
    ///
    /// Fails when the server refuses the command.
    pub async fn uid_store(
        &mut self,
        set: &SequenceSet,
        action: StoreAction,
        flags: &FlagSet,
        silent: bool,
    ) -> Result<Vec<FetchData>> {
        let outcome = self
            .connection
            .execute(Command::uid_store(set, action, flags, silent))
            .await?;
        Ok(collect_fetch(outcome))
    }

    /// `SEARCH`.
    ///
    /// # Errors
    ///
    /// Fails when the server refuses the command.
    pub async fn search(&mut self, query: &SearchQuery) -> Result<Vec<u32>> {
        let outcome = self.connection.execute(Command::search(query)).await?;
        Ok(collect_search(outcome))
    }

    /// `UID SEARCH`.
    ///
    /// # Errors
    ///
    /// Fails when the server refuses the command.
    pub async fn uid_search(&mut self, query: &SearchQuery) -> Result<Vec<u32>> {
        let outcome = self.connection.execute(Command::uid_search(query)).await?;
        Ok(collect_search(outcome))
    }

    /// `COPY`.
    ///
    /// # Errors
    ///
    /// Fails when the server refuses the copy.
    pub async fn copy(&mut self, set: &SequenceSet, mailbox: &MailboxName) -> Result<()> {
        self.connection.execute(Command::copy(set, mailbox)).await?;
        Ok(())
    }

    /// `UID COPY`.
    ///
    /// # Errors
    ///
    /// Fails when the server refuses the copy.
    pub async fn uid_copy(&mut self, set: &SequenceSet, mailbox: &MailboxName) -> Result<()> {
        self.connection
            .execute(Command::uid_copy(set, mailbox))
            .await?;
        Ok(())
    }

    /// `MOVE` (requires the `MOVE` capability).
    ///
    /// # Errors
    ///
    /// Fails when `MOVE` is not advertised or the server refuses.
    pub async fn move_to(&mut self, set: &SequenceSet, mailbox: &MailboxName) -> Result<()> {
        self.require("MOVE")?;
        self.connection
            .execute(Command::move_to(set, mailbox))
            .await?;
        Ok(())
    }

    /// `UID MOVE`.
    ///
    /// # Errors
    ///
    /// Fails when `MOVE` is not advertised or the server refuses.
    pub async fn uid_move(&mut self, set: &SequenceSet, mailbox: &MailboxName) -> Result<()> {
        self.require("MOVE")?;
        self.connection
            .execute(Command::uid_move(set, mailbox))
            .await?;
        Ok(())
    }

    /// `EXPUNGE`.
    ///
    /// # Errors
    ///
    /// Fails when the server refuses the command.
    pub async fn expunge(&mut self) -> Result<Vec<u32>> {
        let outcome = self.connection.execute(Command::expunge()).await?;
        Ok(outcome
            .untagged
            .iter()
            .filter_map(|response| match response {
                Response::Untagged(Untagged::Expunge(number)) => Some(*number),
                _ => None,
            })
            .collect())
    }

    /// `UID EXPUNGE` (RFC 4315).
    ///
    /// # Errors
    ///
    /// Fails when `UIDPLUS` is not advertised or the server refuses.
    pub async fn uid_expunge(&mut self, set: &SequenceSet) -> Result<Vec<u32>> {
        self.require("UIDPLUS")?;
        let outcome = self.connection.execute(Command::uid_expunge(set)).await?;
        Ok(outcome
            .untagged
            .iter()
            .filter_map(|response| match response {
                Response::Untagged(Untagged::Expunge(number)) => Some(*number),
                _ => None,
            })
            .collect())
    }

    /// `ENABLE` extension negotiation (RFC 5161).
    ///
    /// # Errors
    ///
    /// Fails when `ENABLE` is not advertised or the server refuses.
    pub async fn enable(&mut self, capabilities: &[&str]) -> Result<Vec<String>> {
        self.require("ENABLE")?;
        let outcome = self
            .connection
            .execute(Command::enable(capabilities))
            .await?;
        for response in &outcome.untagged {
            if let Response::Untagged(Untagged::Enabled(items)) = response {
                return Ok(items.clone());
            }
        }
        Ok(Vec::new())
    }

    /// `NAMESPACE` (RFC 2342).
    ///
    /// # Errors
    ///
    /// Fails when the server refuses the command.
    pub async fn namespace(&mut self) -> Result<NamespaceResponse> {
        let outcome = self.connection.execute(Command::namespace()).await?;
        for response in &outcome.untagged {
            if let Response::Untagged(Untagged::Namespace(namespace)) = response {
                return Ok(namespace.clone());
            }
        }
        Ok(NamespaceResponse::default())
    }

    /// `NOOP`.
    ///
    /// # Errors
    ///
    /// Fails on transport or protocol errors.
    pub async fn noop(&mut self) -> Result<CommandOutcome> {
        self.connection.execute(Command::noop()).await
    }

    /// `LOGOUT`.
    ///
    /// # Errors
    ///
    /// Fails on transport or protocol errors.
    pub async fn logout(&mut self) -> Result<()> {
        let result = self.connection.execute(Command::logout()).await;
        self.connection.set_logout();
        result.map(|_| ())
    }

    /// `COMPRESS DEFLATE` (RFC 4978).
    ///
    /// # Errors
    ///
    /// Fails when compression is not advertised or the server refuses.
    pub async fn compress(&mut self) -> Result<()> {
        self.require("COMPRESS=DEFLATE")?;
        self.connection.compress().await
    }

    /// Runs `IDLE` for a bounded duration and returns the untagged responses.
    ///
    /// # Errors
    ///
    /// Fails when `IDLE` is not advertised or the server refuses.
    pub async fn idle_for(&mut self, duration: Duration) -> Result<Vec<Response>> {
        self.require("IDLE")?;
        let tag = self.connection.next_tag();
        self.connection
            .write_all(format!("{tag} IDLE\r\n").as_bytes())
            .await?;
        let mut events = Vec::new();
        // Wait for the continuation that starts the idle window.
        loop {
            match self.connection.read_untagged().await? {
                Response::Continuation { .. } => break,
                Response::Untagged(untagged) => events.push(Response::Untagged(untagged)),
                Response::Tagged {
                    status, code, text, ..
                } => return Err(Error::Server { status, code, text }),
            }
        }
        let deadline = Instant::now() + duration;
        loop {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            match tokio::time::timeout(deadline - now, self.connection.read_untagged()).await {
                Ok(Ok(Response::Untagged(untagged))) => events.push(Response::Untagged(untagged)),
                Ok(Ok(Response::Continuation { .. })) => {}
                Ok(Ok(Response::Tagged { .. })) => break,
                Ok(Err(error)) => {
                    let _ = self.connection.write_all(b"DONE\r\n").await;
                    return Err(error);
                }
                Err(_) => break,
            }
        }
        self.connection.write_all(b"DONE\r\n").await?;
        self.connection.send_tagged(&tag).await?;
        Ok(events)
    }

    fn require(&self, capability: &str) -> Result<()> {
        if self.capabilities().has(capability) {
            Ok(())
        } else {
            Err(Error::Unsupported(format!(
                "server does not advertise {capability}"
            )))
        }
    }
}

fn collect_list(outcome: CommandOutcome, subscribed: bool) -> Vec<ListEntry> {
    outcome
        .untagged
        .into_iter()
        .filter_map(|response| match response {
            Response::Untagged(Untagged::List(entry)) if !subscribed => Some(entry),
            Response::Untagged(Untagged::Lsub(entry)) if subscribed => Some(entry),
            _ => None,
        })
        .collect()
}

fn collect_fetch(outcome: CommandOutcome) -> Vec<FetchData> {
    outcome
        .untagged
        .into_iter()
        .filter_map(|response| match response {
            Response::Untagged(Untagged::Fetch(data)) => Some(*data),
            _ => None,
        })
        .collect()
}

fn collect_search(outcome: CommandOutcome) -> Vec<u32> {
    outcome
        .untagged
        .into_iter()
        .filter_map(|response| match response {
            Response::Untagged(Untagged::Search(ids)) => Some(ids),
            _ => None,
        })
        .flatten()
        .collect()
}

fn apply_select_codes(info: &mut SelectInfo, outcome: &CommandOutcome) {
    if let Some(code) = &outcome.code {
        apply_select_code(info, code);
    }
    for response in &outcome.untagged {
        if let Response::Untagged(Untagged::Ok {
            code: Some(code), ..
        }) = response
        {
            apply_select_code(info, code);
        }
    }
}

fn apply_select_code(info: &mut SelectInfo, code: &ResponseCode) {
    match code {
        ResponseCode::UidValidity(value) => info.uid_validity = Some(*value),
        ResponseCode::UidNext(value) => info.uid_next = Some(*value),
        ResponseCode::Unseen(value) => info.unseen = Some(*value),
        ResponseCode::PermanentFlags(flags) => info.permanent_flags = Some(flags.clone()),
        ResponseCode::ReadOnly => info.read_only = true,
        ResponseCode::ReadWrite => info.read_only = false,
        ResponseCode::HighestModSeq(value) => info.highest_mod_seq = Some(*value),
        _ => {}
    }
}
