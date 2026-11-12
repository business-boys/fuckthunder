//! UI-independent account checking and read-only mail operations for the GPUI client.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::{fmt, time::Duration};

use fuckthunder_core::{Error, MailboxName, Result, SequenceSet};
use fuckthunder_imap::{ClientConfig, ImapClient, ProxyConfig, ProxyProtocol};
use fuckthunder_imap_proto::{FetchItems, SearchQuery};
use futures::{stream, StreamExt};
use mailparse::{MailHeaderMap, ParsedMail};
use tokio::sync::mpsc;

pub mod api;
mod store;

pub use api::{ApiError, ApiServer, ApiStatus};
pub use store::{ImportSummary, ProxyRecord, RunSummary, Store, StoredAccount, WorkspaceSettings};

/// Number of message summaries fetched for one page.
pub const PAGE_SIZE: usize = 50;

/// An account held in memory for the current process only.
#[derive(Clone)]
pub struct Account {
    /// Login and email address.
    pub email: String,
    password: String,
    host: Option<String>,
    port: u16,
    starttls: bool,
}

impl Account {
    /// Creates an account with an optional explicit IMAP host.
    pub fn new(
        email: &str,
        password: &str,
        host: Option<&str>,
        port: u16,
        starttls: bool,
    ) -> Result<Self> {
        if email.contains(['\r', '\n']) {
            return Err(Error::Invalid("email must not contain line breaks".into()));
        }
        let email = email.trim();
        let (local, domain) = email
            .split_once('@')
            .ok_or_else(|| Error::Invalid("expected non-empty email and password".into()))?;
        if local.is_empty()
            || domain.is_empty()
            || domain.contains('@')
            || email.chars().any(char::is_whitespace)
        {
            return Err(Error::Invalid(
                "expected non-empty email and password".into(),
            ));
        }
        if password.is_empty() || password.contains(['\r', '\n']) {
            return Err(Error::Invalid(
                "expected non-empty email and password".into(),
            ));
        }
        let host = host
            .map(str::trim)
            .map(|host| {
                if host.is_empty()
                    || host.chars().any(char::is_whitespace)
                    || host.contains(['/', ':'])
                {
                    return Err(Error::Invalid("invalid explicit IMAP host".into()));
                }
                Ok(host.to_owned())
            })
            .transpose()?;
        if port == 0 {
            return Err(Error::Invalid("IMAP port must be nonzero".into()));
        }
        Ok(Self {
            email: email.to_owned(),
            password: password.to_owned(),
            host,
            port,
            starttls,
        })
    }

    /// Parses one `email:password` row. Passwords may contain colons.
    pub fn parse(row: &str) -> Result<Self> {
        let (email, password) = row
            .trim()
            .split_once(':')
            .ok_or_else(|| Error::Invalid("expected email:password".into()))?;
        Self::new(email, password, None, 993, false)
    }

    /// Returns the explicit IMAP host or a provider-specific inferred host.
    #[must_use]
    pub fn imap_host(&self) -> String {
        if let Some(host) = &self.host {
            return host.clone();
        }
        let domain = self
            .email
            .rsplit_once('@')
            .map_or("", |(_, domain)| domain)
            .to_ascii_lowercase();
        match domain.as_str() {
            "web.de" => "imap.web.de".into(),
            "gmx.at" | "gmx.ch" | "gmx.de" | "gmx.net" => "imap.gmx.net".into(),
            "gmail.com" | "googlemail.com" => "imap.gmail.com".into(),
            "outlook.com" | "hotmail.com" | "live.com" => "outlook.office365.com".into(),
            "icloud.com" | "me.com" | "mac.com" => "imap.mail.me.com".into(),
            "t-online.de" => "secureimap.t-online.de".into(),
            "yahoo.com" | "yahoo.co.uk" | "yahoo.de" | "yahoo.fr" | "yahoo.it" | "yahoo.es"
            | "yahoo.ca" | "yahoo.com.au" => "imap.mail.yahoo.com".into(),
            "libero.it" => "imapmail.libero.it".into(),
            "virgilio.it" => "in.virgilio.it".into(),
            _ => format!("imap.{domain}"),
        }
    }

    /// Returns the configured IMAP port.
    #[must_use]
    pub fn imap_port(&self) -> u16 {
        self.port
    }

    /// Returns whether the connection upgrades plaintext IMAP with STARTTLS.
    #[must_use]
    pub fn uses_starttls(&self) -> bool {
        self.starttls
    }
}

impl fmt::Debug for Account {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Account")
            .field("email", &self.email)
            .field("password", &"<redacted>")
            .finish()
    }
}

/// Parses a UTF-8 account list, ignoring empty lines and `#` comments.
pub fn parse_accounts(input: &str) -> Vec<Account> {
    input
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| Account::parse(line).ok())
        .collect()
}

/// Connection settings shared by account operations.
#[derive(Clone, Debug)]
pub struct SessionOptions {
    /// Optional per-process proxy. Account cookies/state are never shared.
    pub proxy: Option<ProxyConfig>,
    /// Deadline applied by the IMAP client to individual operations.
    pub timeout: Duration,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            proxy: None,
            timeout: Duration::from_secs(30),
        }
    }
}

/// Parses `socks5://[user:password@]host:port` or `http://[user:password@]host:port`.
pub fn parse_proxy(value: &str) -> std::result::Result<ProxyConfig, url::ParseError> {
    let url = url::Url::parse(value)?;
    let host = url.host_str().unwrap_or_default().to_owned();
    let protocol = match url.scheme() {
        "socks5" => ProxyProtocol::Socks5,
        "http" => ProxyProtocol::HttpConnect,
        _ => return Err(url::ParseError::RelativeUrlWithoutBase),
    };
    let port = url.port().unwrap_or(match protocol {
        ProxyProtocol::Socks5 => 1080,
        ProxyProtocol::HttpConnect => 8080,
    });
    let auth = (!url.username().is_empty()).then(|| {
        (
            url.username().to_owned(),
            url.password().unwrap_or_default().to_owned(),
        )
    });
    Ok(ProxyConfig {
        host,
        port,
        auth,
        protocol,
    })
}

/// Final state of one account check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckStatus {
    /// No check has completed.
    Idle,
    /// The account is currently being checked.
    Checking,
    /// Login and read-only INBOX access succeeded.
    Valid {
        /// Number of messages reported by INBOX.
        messages: u32,
    },
    /// The server explicitly rejected authentication.
    Invalid {
        /// Server diagnostic, without credentials.
        reason: String,
    },
    /// Transport, timeout, protocol or mailbox failure; not an invalid credential verdict.
    Error {
        /// Diagnostic suitable for the result table.
        reason: String,
    },
}

/// One completed item from a batch check.
#[derive(Clone, Debug)]
pub struct CheckResult {
    /// Index in the imported account list.
    pub index: usize,
    /// New account state.
    pub status: CheckStatus,
}

/// One folder available for read-only viewing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    /// Display and IMAP mailbox name.
    /// Suggested filename from MIME metadata.
    pub name: String,
}

/// A message summary shown in the paginated list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MessageSummary {
    /// Stable IMAP UID within the folder's UIDVALIDITY generation.
    pub uid: u32,
    /// Decoded subject.
    pub subject: String,
    /// Decoded sender.
    pub from: String,
    /// Date supplied by the message envelope.
    pub date: String,
    /// RFC 822 message size.
    pub size: u64,
}

/// One page of message summaries.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MessagePage {
    /// Zero-based page index.
    pub page: usize,
    /// Total messages in the folder at search time.
    pub total: usize,
    /// Newest-first summaries.
    pub messages: Vec<MessageSummary>,
}

/// Fully decoded plain-text message shown by the viewer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MessageView {
    /// Decoded subject.
    pub subject: String,
    /// Decoded From header.
    pub from: String,
    /// Decoded To header.
    pub to: String,
    /// Date header.
    pub date: String,
    /// Plain-text body, or an explicit notice for HTML-only mail.
    pub body: String,
    /// Original HTML body when the message is HTML, for formatted rendering.
    pub html: Option<String>,
    /// Original RFC 5322 message bytes.
    pub raw: std::sync::Arc<[u8]>,
    /// Ordered decoded headers, including duplicates.
    pub headers: Vec<(String, String)>,
    /// Decoded downloadable and inline MIME parts.
    pub attachments: Vec<Attachment>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
/// A decoded MIME attachment or inline resource.
pub struct Attachment {
    /// Suggested filename from MIME metadata.
    pub name: String,
    /// MIME media type.
    pub media_type: String,
    /// Content-ID without angle brackets.
    pub content_id: Option<String>,
    /// Transfer-decoded part bytes.
    pub data: std::sync::Arc<[u8]>,
}

fn attachments(part: &ParsedMail<'_>, result: &mut Vec<Attachment>) -> Result<()> {
    let disposition = part.get_content_disposition();
    let cid = part.headers.get_first_value("Content-ID");
    let filename = disposition
        .params
        .get("filename")
        .or_else(|| part.ctype.params.get("name"));
    if filename.is_some()
        || disposition.disposition == mailparse::DispositionType::Attachment
        || (part.subparts.is_empty()
            && cid.is_some()
            && !matches!(part.ctype.mimetype.as_str(), "text/html" | "text/plain"))
    {
        result.push(Attachment {
            name: filename
                .cloned()
                .unwrap_or_else(|| format!("attachment-{}", result.len() + 1)),
            media_type: part.ctype.mimetype.clone(),
            content_id: cid.map(|id| id.trim().trim_matches(['<', '>']).to_owned()),
            data: part
                .get_body_raw()
                .map_err(|error| Error::Parse(format!("MIME attachment: {error}")))?
                .into(),
        });
        return Ok(());
    }
    for child in &part.subparts {
        attachments(child, result)?;
    }
    Ok(())
}

impl MessageView {
    /// Builds an isolated HTML document, resolving inline MIME images.
    pub fn html_document(&self, remote_images: bool) -> String {
        use base64::Engine as _;
        let escape = |text: &str| {
            text.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
        };
        let mut html = self
            .html
            .clone()
            .unwrap_or_else(|| format!("<pre>{}</pre>", escape(&self.body)));
        for file in &self.attachments {
            if let Some(cid) = &file.content_id {
                if file.media_type.starts_with("image/") {
                    let data = base64::engine::general_purpose::STANDARD.encode(&file.data);
                    html = html.replace(
                        &format!("cid:{cid}"),
                        &format!("data:{};base64,{data}", file.media_type),
                    );
                }
            }
        }
        let images = if remote_images {
            "data: https: http:"
        } else {
            "data:"
        };
        format!("<!DOCTYPE html><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src {images}; style-src 'unsafe-inline'; script-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none'\"><style>body{{margin:0;padding:20px;font:15px/1.6 -apple-system,sans-serif;color:#222;background:#fff;overflow-wrap:anywhere}}img{{max-width:100%;height:auto}}pre{{white-space:pre-wrap;overflow-wrap:anywhere}}table{{max-width:100%}}</style>{html}")
    }
}

async fn connect(account: &Account, options: &SessionOptions) -> Result<ImapClient> {
    let host = account.imap_host();
    let mut config = if account.uses_starttls() {
        ClientConfig::starttls(host)
    } else {
        ClientConfig::tls(host)
    }
    .with_timeout(options.timeout);
    config.port = account.imap_port();
    config.proxy = options.proxy.clone();
    ImapClient::connect(config).await
}

fn invalid_reason(error: &Error) -> Option<String> {
    match error {
        Error::Auth(reason) => Some(reason.clone()),
        Error::Server { text, .. } => {
            let lower = text.to_ascii_lowercase();
            [
                "auth",
                "credential",
                "login",
                "log in",
                "password",
                "invalid",
                "incorrect",
            ]
            .iter()
            .any(|needle| lower.contains(needle))
            .then(|| text.clone())
        }
        _ => None,
    }
}

/// Checks one account without modifying mailbox state.
pub async fn check_account(account: &Account, options: &SessionOptions) -> CheckStatus {
    let mut client = match connect(account, options).await {
        Ok(client) => client,
        Err(error) => {
            return CheckStatus::Error {
                reason: error.to_string(),
            };
        }
    };
    if let Err(error) = client.login(&account.email, &account.password).await {
        return match invalid_reason(&error) {
            Some(reason) => CheckStatus::Invalid { reason },
            None => CheckStatus::Error {
                reason: error.to_string(),
            },
        };
    }
    let inbox = match MailboxName::from_utf8("INBOX") {
        Ok(inbox) => inbox,
        Err(error) => {
            return CheckStatus::Error {
                reason: error.to_string(),
            };
        }
    };
    let result = client.select(&inbox, true).await;
    client.logout().await.ok();
    match result {
        Ok(info) => CheckStatus::Valid {
            messages: info.exists,
        },
        Err(error) => CheckStatus::Error {
            reason: error.to_string(),
        },
    }
}
/// One message that matched a mail search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MailHit {
    /// Folder the message lives in.
    pub folder: String,
    /// Decoded subject.
    pub subject: String,
    /// Decoded sender.
    pub from: String,
}

/// Searches every selectable folder for messages whose subject, sender or body contains the query.
pub async fn search_account_mail(
    account: &Account,
    options: &SessionOptions,
    query: &str,
    limit: usize,
) -> Result<Vec<MailHit>> {
    let mut session = MailSession::connect(account, options).await?;
    let result = session.search(query, limit).await;
    session.disconnect().await.ok();
    result
}

/// Runs a bounded batch and sends each result as soon as it completes.
pub async fn check_accounts(
    accounts: Vec<(usize, Account)>,
    options: SessionOptions,
    concurrency: usize,
    results: mpsc::UnboundedSender<CheckResult>,
) {
    stream::iter(accounts.into_iter().map(|(index, account)| {
        let options = options.clone();
        async move {
            let status = check_account(&account, &options).await;
            (index, status)
        }
    }))
    .buffer_unordered(concurrency.max(1))
    .for_each(|(index, status)| {
        let _ = results.send(CheckResult { index, status });
        futures::future::ready(())
    })
    .await;
}

/// One authenticated read-only IMAP session, owned by a mailbox tab.
pub struct MailSession {
    pub(crate) client: ImapClient,
}

impl MailSession {
    /// Connects and logs in once with the configured endpoint and proxy.
    pub async fn connect(account: &Account, options: &SessionOptions) -> Result<Self> {
        let mut client = connect(account, options).await?;
        client.login(&account.email, &account.password).await?;
        Ok(Self { client })
    }

    #[cfg(test)]
    fn from_client(client: ImapClient) -> Self {
        Self { client }
    }

    /// Lists selectable folders, INBOX first.
    pub async fn folders(&mut self) -> Result<Vec<Folder>> {
        let mut folders: Vec<_> = self
            .client
            .list("", "*")
            .await?
            .into_iter()
            .filter(|entry| entry.is_selectable())
            .map(|entry| Folder {
                name: entry.name.to_string(),
            })
            .collect();
        folders.sort_by_key(|folder| {
            (
                !folder.name.eq_ignore_ascii_case("INBOX"),
                folder.name.clone(),
            )
        });
        Ok(folders)
    }

    /// Searches the whole folder on the server and fetches a newest-first page.
    /// A blank query matches every message.
    pub async fn page(&mut self, folder: &str, page: usize, query: &str) -> Result<MessagePage> {
        let mailbox = MailboxName::from_utf8(folder)?;
        self.client.select(&mailbox, true).await?;
        let query = query.trim();
        let search = if query.is_empty() {
            SearchQuery::all()
        } else {
            SearchQuery::text(query)
        };
        let mut uids = self.client.uid_search(&search).await?;
        uids.sort_unstable_by(|left, right| right.cmp(left));
        let total = uids.len();
        let selected: Vec<_> = uids
            .into_iter()
            .skip(page.saturating_mul(PAGE_SIZE))
            .take(PAGE_SIZE)
            .collect();
        let mut messages = if selected.is_empty() {
            Vec::new()
        } else {
            let set: SequenceSet = selected
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",")
                .parse()?;
            self.client.uid_fetch(&set, &FetchItems::summary()).await?
        };
        messages.sort_by_key(|message| std::cmp::Reverse(message.uid.unwrap_or_default()));
        Ok(MessagePage {
            page,
            total,
            messages: messages
                .into_iter()
                .map(|message| {
                    let envelope = message.envelope.unwrap_or_default();
                    MessageSummary {
                        uid: message.uid.unwrap_or_default(),
                        subject: decode_header(
                            envelope.subject.as_deref().unwrap_or("(no subject)"),
                        ),
                        from: envelope.from.first().map_or_else(String::new, |address| {
                            decode_header(&address.to_mailbox_string())
                        }),
                        date: envelope.date.unwrap_or_default(),
                        size: message.rfc822_size.unwrap_or_default(),
                    }
                })
                .collect(),
        })
    }

    /// Fetches one message with `BODY.PEEK[]`, so its unread flag stays unchanged.
    pub async fn message(&mut self, folder: &str, uid: u32) -> Result<MessageView> {
        let mailbox = MailboxName::from_utf8(folder)?;
        self.client.select(&mailbox, true).await?;
        let set: SequenceSet = uid.to_string().parse()?;
        let fetched = self
            .client
            .uid_fetch(&set, &FetchItems::peek_section(""))
            .await?;
        let raw = fetched
            .iter()
            .find(|message| message.uid == Some(uid))
            .and_then(|message| message.sections.first())
            .map(|section| section.data.as_slice())
            .ok_or_else(|| Error::Protocol(format!("UID {uid} returned no message body")))?;
        decode_message(raw)
    }

    /// Searches every selectable folder for subject, sender or body matches, newest first.
    pub async fn search(&mut self, query: &str, limit: usize) -> Result<Vec<MailHit>> {
        let folders = self.folders().await?;
        let search = SearchQuery::subject(query)
            .or(SearchQuery::from(query))
            .or(SearchQuery::body(query));
        let mut hits = Vec::new();
        for folder in folders {
            if hits.len() >= limit {
                break;
            }
            let mailbox = MailboxName::from_utf8(&folder.name)?;
            if self.client.select(&mailbox, true).await.is_err() {
                continue;
            }
            let Ok(mut uids) = self.client.uid_search(&search).await else {
                continue;
            };
            uids.sort_unstable_by(|left, right| right.cmp(left));
            let selected: Vec<_> = uids.into_iter().take(limit - hits.len()).collect();
            if selected.is_empty() {
                continue;
            }
            let set: SequenceSet = selected
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",")
                .parse()?;
            let Ok(messages) = self.client.uid_fetch(&set, &FetchItems::summary()).await else {
                continue;
            };
            for message in messages {
                let envelope = message.envelope.unwrap_or_default();
                hits.push(MailHit {
                    folder: folder.name.clone(),
                    subject: decode_header(envelope.subject.as_deref().unwrap_or("(no subject)")),
                    from: envelope.from.first().map_or_else(String::new, |address| {
                        decode_header(&address.to_mailbox_string())
                    }),
                });
                if hits.len() >= limit {
                    break;
                }
            }
        }
        Ok(hits)
    }

    /// Logs out.
    pub async fn disconnect(&mut self) -> Result<()> {
        self.client.logout().await
    }
}

/// Logs in and lists selectable folders.
pub async fn load_folders(account: &Account, options: &SessionOptions) -> Result<Vec<Folder>> {
    let mut session = MailSession::connect(account, options).await?;
    let result = session.folders().await;
    session.disconnect().await.ok();
    result
}

/// Fetches a newest-first page with read-only mailbox access.
pub async fn load_page(
    account: &Account,
    options: &SessionOptions,
    folder: &str,
    page: usize,
) -> Result<MessagePage> {
    let mut session = MailSession::connect(account, options).await?;
    let result = session.page(folder, page, "").await;
    session.disconnect().await.ok();
    result
}

/// Fetches and decodes one message without changing its unread state.
pub async fn load_message(
    account: &Account,
    options: &SessionOptions,
    folder: &str,
    uid: u32,
) -> Result<MessageView> {
    let mut session = MailSession::connect(account, options).await?;
    let result = session.message(folder, uid).await;
    session.disconnect().await.ok();
    result
}

fn decode_header(value: &str) -> String {
    let synthetic = format!("X: {value}\r\n\r\n");
    mailparse::parse_headers(synthetic.as_bytes())
        .ok()
        .and_then(|(headers, _)| headers.get_first_value("X"))
        .unwrap_or_else(|| value.to_owned())
}

fn readable_part<'a>(part: &ParsedMail<'a>, plain: &mut Option<String>, html: &mut Option<String>) {
    let disposition = part.get_content_disposition();
    if disposition.disposition == mailparse::DispositionType::Attachment
        || disposition.params.contains_key("filename")
        || part.ctype.params.contains_key("name")
    {
        return;
    }
    let mimetype = part.ctype.mimetype.to_ascii_lowercase();
    if mimetype == "text/plain" && plain.is_none() {
        *plain = part.get_body().ok();
    } else if mimetype == "text/html" && html.is_none() {
        *html = part.get_body().ok();
    }
    for child in &part.subparts {
        readable_part(child, plain, html);
    }
}

/// Converts an HTML body to readable wrapped text without rendering markup.
fn html_to_text(html: &str) -> String {
    let text = html2text::config::plain_no_decorate()
        .no_table_borders()
        .string_from_read(html.as_bytes(), 100)
        .unwrap_or_else(|_| html.to_owned());
    let trimmed = text.trim();
    if trimmed.is_empty() {
        "[This HTML message has no readable text.]".into()
    } else {
        trimmed.to_owned()
    }
}

/// Decodes MIME headers and a readable body: plain text first, HTML converted to text otherwise.
pub fn decode_message(raw: &[u8]) -> Result<MessageView> {
    let parsed =
        mailparse::parse_mail(raw).map_err(|error| Error::Parse(format!("MIME: {error}")))?;
    let (mut plain, mut html) = (None, None);
    readable_part(&parsed, &mut plain, &mut html);
    let (body, html) = match (plain.filter(|text| !text.trim().is_empty()), html) {
        (Some(text), html) => (text, html),
        (None, Some(html)) => (html_to_text(&html), Some(html)),
        (None, None) => ("[This message has no readable text part.]".into(), None),
    };
    let mut files = Vec::new();
    attachments(&parsed, &mut files)?;
    Ok(MessageView {
        raw: raw.into(),
        headers: parsed
            .headers
            .iter()
            .map(|header| (header.get_key(), header.get_value()))
            .collect(),
        attachments: files,
        subject: parsed
            .headers
            .get_first_value("Subject")
            .unwrap_or_else(|| "(no subject)".into()),
        from: parsed.headers.get_first_value("From").unwrap_or_default(),
        to: parsed.headers.get_first_value("To").unwrap_or_default(),
        date: parsed.headers.get_first_value("Date").unwrap_or_default(),
        body,
        html,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        net::TcpListener,
    };

    #[tokio::test]
    async fn reads_folders_search_and_message_on_one_authenticated_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let logins = Arc::new(Mutex::new(0));
        let server_logins = logins.clone();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut io = BufReader::new(socket);
            io.write_all(b"* OK mock ready\r\n").await.unwrap();
            for _ in 0..8 {
                let mut line = Vec::new();
                io.read_until(b'\n', &mut line).await.unwrap();
                assert!(!line.is_empty(), "client reconnected for another operation");
                let text = String::from_utf8_lossy(&line);
                let mut words = text.split_whitespace();
                let tag = words.next().unwrap_or("a1").to_owned();
                let command = words.next().unwrap_or("").to_ascii_uppercase();
                println!("server received: {text:?}");
                match command.as_str() {
                    "CAPABILITY" => io
                        .write_all(format!("{tag} OK\r\n").as_bytes())
                        .await
                        .unwrap(),
                    "LOGIN" => {
                        *server_logins.lock().unwrap() += 1;
                        io.write_all(format!("{tag} OK\r\n").as_bytes())
                            .await
                            .unwrap();
                    }
                    "LIST" => io
                        .write_all(format!("* LIST () \"/\" INBOX\r\n{tag} OK\r\n").as_bytes())
                        .await
                        .unwrap(),
                    "EXAMINE" => io
                        .write_all(format!("* 1 EXISTS\r\n{tag} OK [READ-ONLY]\r\n").as_bytes())
                        .await
                        .unwrap(),
                    "UID" => {
                        let subcommand = words.next().unwrap_or("").to_ascii_uppercase();
                        if subcommand == "SEARCH" {
                            assert!(text.contains("TEXT"));
                            io.write_all(format!("* SEARCH 41\r\n{tag} OK\r\n").as_bytes())
                                .await
                                .unwrap();
                        } else if text.contains("BODY.PEEK[]") {
                            let raw = "Subject: Hello\r\nFrom: sender@example.com\r\n\r\nBody";
                            io.write_all(
                                format!(
                                    "* 1 FETCH (UID 41 BODY[] {{{}}}\r\n{})\r\n{tag} OK\r\n",
                                    raw.len(),
                                    raw
                                )
                                .as_bytes(),
                            )
                            .await
                            .unwrap();
                        } else {
                            io.write_all(format!("* 1 FETCH (UID 41 ENVELOPE (NIL NIL NIL NIL NIL NIL NIL NIL NIL NIL))\r\n{tag} OK\r\n").as_bytes()).await.unwrap();
                        }
                    }
                    _ => {
                        io.write_all(format!("{tag} BAD unknown\\r\\n").as_bytes())
                            .await
                            .unwrap();
                        panic!("unexpected command: {text}");
                    }
                }
            }
        });
        let account = Account::new(
            "person@example.com",
            "secret",
            Some("127.0.0.1"),
            port,
            false,
        )
        .unwrap();
        let mut client = ImapClient::connect(ClientConfig::plaintext("127.0.0.1", port))
            .await
            .unwrap();
        client
            .login(&account.email, &account.password)
            .await
            .unwrap();
        let mut session = MailSession::from_client(client);
        assert_eq!(session.folders().await.unwrap()[0].name, "INBOX");
        let result = session.page("INBOX", 0, "hello").await;
        println!("page: {result:?}");
        assert_eq!(result.unwrap().total, 1);
        assert_eq!(session.message("INBOX", 41).await.unwrap().subject, "Hello");
        server.await.unwrap();
        assert_eq!(*logins.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn searches_mail_by_subject_sender_and_body_across_folders() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut io = BufReader::new(socket);
            io.write_all(b"* OK mock ready\r\n").await.unwrap();
            for _ in 0..7 {
                let mut line = Vec::new();
                io.read_until(b'\n', &mut line).await.unwrap();
                assert!(!line.is_empty());
                let text = String::from_utf8_lossy(&line);
                let mut words = text.split_whitespace();
                let tag = words.next().unwrap_or("a1").to_owned();
                let command = words.next().unwrap_or("").to_ascii_uppercase();
                match command.as_str() {
                    "CAPABILITY" => io
                        .write_all(format!("{tag} OK\r\n").as_bytes())
                        .await
                        .unwrap(),
                    "LOGIN" => io
                        .write_all(format!("{tag} OK\r\n").as_bytes())
                        .await
                        .unwrap(),
                    "LIST" => io
                        .write_all(format!("* LIST () \"/\" INBOX\r\n{tag} OK\r\n").as_bytes())
                        .await
                        .unwrap(),
                    "EXAMINE" => io
                        .write_all(format!("* 2 EXISTS\r\n{tag} OK [READ-ONLY]\r\n").as_bytes())
                        .await
                        .unwrap(),
                    "UID" => {
                        let subcommand = words.next().unwrap_or("").to_ascii_uppercase();
                        if subcommand == "SEARCH" {
                            assert!(
                                text.contains("SUBJECT"),
                                "query must search subject: {text}"
                            );
                            assert!(text.contains("FROM"), "query must search sender: {text}");
                            assert!(text.contains("BODY"), "query must search body: {text}");
                            io.write_all(format!("* SEARCH 41 40\r\n{tag} OK\r\n").as_bytes())
                                .await
                                .unwrap();
                        } else {
                            io.write_all(format!("* 1 FETCH (UID 41 ENVELOPE (NIL NIL NIL NIL NIL NIL NIL NIL NIL NIL))\r\n{tag} OK\r\n").as_bytes()).await.unwrap();
                        }
                    }
                    "LOGOUT" => {
                        io.write_all(format!("{tag} OK\r\n").as_bytes())
                            .await
                            .unwrap();
                    }
                    _ => panic!("unexpected command: {text}"),
                }
            }
        });
        let account = Account::new(
            "person@example.com",
            "secret",
            Some("127.0.0.1"),
            port,
            false,
        )
        .unwrap();
        let mut client = ImapClient::connect(ClientConfig::plaintext("127.0.0.1", port))
            .await
            .unwrap();
        client
            .login(&account.email, &account.password)
            .await
            .unwrap();
        let mut session = MailSession::from_client(client);
        let hits = session.search("invoice", 5).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].folder, "INBOX");
        session.disconnect().await.ok();
        server.await.unwrap();
    }

    #[test]
    fn parses_crlf_and_password_colons_without_exposing_secret() {
        let accounts = parse_accounts("# comment\r\nuser@gmx.de:pa:ss\r\n\r\n");
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].imap_host(), "imap.gmx.net");
        assert_eq!(accounts[0].password, "pa:ss");
        assert!(!format!("{:?}", accounts[0]).contains("pa:ss"));
    }

    #[test]
    fn rejects_malformed_credentials_and_manual_endpoints() {
        let cases = [
            ("", "secret", None, 993),
            ("user", "secret", None, 993),
            ("@example.com", "secret", None, 993),
            ("user@", "secret", None, 993),
            ("us er@example.com", "secret", None, 993),
            ("user@example.com\r\n", "secret", None, 993),
            ("user@example.com", "", None, 993),
            ("user@example.com", "sec\r\nret", None, 993),
            ("user@example.com", "secret", Some("  "), 993),
            ("user@example.com", "secret", Some("mail example.com"), 993),
            ("user@example.com", "secret", Some("mail/example.com"), 993),
            (
                "user@example.com",
                "secret",
                Some("mail.example.com:143"),
                993,
            ),
            ("user@example.com", "secret", None, 0),
        ];
        for (email, password, host, port) in cases {
            assert!(
                Account::new(email, password, host, port, false).is_err(),
                "accepted {email:?}, {password:?}, {host:?}, {port}"
            );
        }
    }

    #[test]
    fn maps_known_provider_hosts_without_wildcard_provider_guesses() {
        let cases = [
            ("user@web.de", "imap.web.de"),
            ("user@gmx.at", "imap.gmx.net"),
            ("user@gmail.com", "imap.gmail.com"),
            ("user@googlemail.com", "imap.gmail.com"),
            ("user@outlook.com", "outlook.office365.com"),
            ("user@hotmail.com", "outlook.office365.com"),
            ("user@live.com", "outlook.office365.com"),
            ("user@icloud.com", "imap.mail.me.com"),
            ("user@me.com", "imap.mail.me.com"),
            ("user@mac.com", "imap.mail.me.com"),
            ("user@t-online.de", "secureimap.t-online.de"),
            ("user@yahoo.co.uk", "imap.mail.yahoo.com"),
            ("user@libero.it", "imapmail.libero.it"),
            ("user@virgilio.it", "in.virgilio.it"),
            ("user@unknown.example", "imap.unknown.example"),
        ];
        for (email, expected) in cases {
            let row = format!("{email}:secret");
            assert_eq!(Account::parse(&row).unwrap().imap_host(), expected);
        }
    }

    #[test]
    fn decodes_encoded_words_and_multipart_plain_text() {
        let raw = b"Subject: =?UTF-8?Q?Hallo_=E2=9C=93?=\r\nFrom: =?UTF-8?Q?Andr=C3=A9?= <a@example.org>\r\nMIME-Version: 1.0\r\nContent-Type: multipart/alternative; boundary=x\r\n\r\n--x\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<b>ignored</b>\r\n--x\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nHallo=20Welt\r\n--x--\r\n";
        let message = decode_message(raw).unwrap();
        assert_eq!(message.subject, "Hallo ✓");
        assert_eq!(message.from, "André <a@example.org>");
        assert!(message.body.contains("Hallo Welt"));
        assert!(!message.body.contains("ignored"));
    }

    #[test]
    fn preserves_original_message_headers_and_transfer_decoded_attachments() {
        let raw = b"Subject: Inline test\r\nX-Trace: first\r\nX-Trace: second\r\nContent-Type: multipart/related; boundary=m\r\n\r\n--m\r\nContent-Type: text/html\r\n\r\n<p>Hello</p><img src=\"cid:logo\">\r\n--m\r\nContent-Type: image/png\r\nContent-ID: <logo>\r\nContent-Disposition: inline; filename=logo.png\r\nContent-Transfer-Encoding: base64\r\n\r\naGVsbG8=\r\n--m--\r\n";
        let message = decode_message(raw).unwrap();
        assert_eq!(&*message.raw, raw);
        assert_eq!(
            message
                .headers
                .iter()
                .filter(|(key, _)| key == "X-Trace")
                .map(|(_, value)| value.as_str())
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
        let attachment = &message.attachments[0];
        assert_eq!(attachment.name, "logo.png");
        assert_eq!(&*attachment.data, b"hello");
        assert!(message
            .html_document(false)
            .contains("data:image/png;base64,aGVsbG8="));
        assert!(!message.html_document(false).contains("src=\"cid:logo\""));
    }

    #[test]
    fn html_only_message_shows_readable_text_without_markup_or_scripts() {
        let message = decode_message(
            b"Content-Type: text/html; charset=utf-8\r\n\r\n<html><head><style>p{color:red}</style><script>alert(1)</script></head><body><h1>Order shipped</h1><p>Your <b>parcel</b> is on the way.</p><a href=\"https://example.com/track\">Track</a></body></html>",
        )
        .unwrap();
        assert!(message.body.contains("Order shipped"));
        assert!(message.body.contains("parcel"));
        assert!(message.body.contains("is on the way"));
        assert!(!message.body.contains("<b>"));
        assert!(!message.body.contains("alert(1)"));
        assert!(!message.body.contains("color:red"));
    }
}
