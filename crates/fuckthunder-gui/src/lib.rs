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
}

impl Account {
    /// Parses one `email:password` row. Passwords may contain colons.
    pub fn parse(row: &str) -> Result<Self> {
        let (email, password) = row
            .trim()
            .split_once(':')
            .ok_or_else(|| Error::Invalid("expected email:password".into()))?;
        let email = email.trim();
        if email.is_empty() || !email.contains('@') || password.is_empty() {
            return Err(Error::Invalid(
                "expected non-empty email and password".into(),
            ));
        }
        Ok(Self {
            email: email.to_owned(),
            password: password.to_owned(),
        })
    }

    /// Infers the implicit-TLS IMAP host, including the United Internet exceptions.
    #[must_use]
    pub fn imap_host(&self) -> String {
        let domain = self
            .email
            .rsplit_once('@')
            .map_or("", |(_, domain)| domain)
            .to_ascii_lowercase();
        match domain.as_str() {
            "web.de" => "imap.web.de".into(),
            "gmx.at" | "gmx.ch" | "gmx.de" | "gmx.net" => "imap.gmx.net".into(),
            _ => format!("imap.{domain}"),
        }
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
}

async fn connect(account: &Account, options: &SessionOptions) -> Result<ImapClient> {
    let mut config = ClientConfig::tls(account.imap_host()).with_timeout(options.timeout);
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

/// Logs in and lists selectable folders without selecting a mailbox.
pub async fn load_folders(account: &Account, options: &SessionOptions) -> Result<Vec<Folder>> {
    let mut client = connect(account, options).await?;
    client.login(&account.email, &account.password).await?;
    let mut folders: Vec<_> = client
        .list("", "*")
        .await?
        .into_iter()
        .filter(|entry| entry.is_selectable())
        .map(|entry| Folder {
            name: entry.name.to_string(),
        })
        .collect();
    client.logout().await.ok();
    folders.sort_by_key(|folder| {
        (
            !folder.name.eq_ignore_ascii_case("INBOX"),
            folder.name.clone(),
        )
    });
    Ok(folders)
}

/// Fetches a newest-first page using only `EXAMINE`, `UID SEARCH` and summary `FETCH`.
pub async fn load_page(
    account: &Account,
    options: &SessionOptions,
    folder: &str,
    page: usize,
) -> Result<MessagePage> {
    let mut client = connect(account, options).await?;
    client.login(&account.email, &account.password).await?;
    let mailbox = MailboxName::from_utf8(folder)?;
    client.select(&mailbox, true).await?;
    let mut uids = client.uid_search(&SearchQuery::all()).await?;
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
        client.uid_fetch(&set, &FetchItems::summary()).await?
    };
    client.logout().await.ok();
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
                    subject: decode_header(envelope.subject.as_deref().unwrap_or("(no subject)")),
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

/// Lazily fetches and decodes one complete message using `BODY.PEEK[]`.
pub async fn load_message(
    account: &Account,
    options: &SessionOptions,
    folder: &str,
    uid: u32,
) -> Result<MessageView> {
    let mut client = connect(account, options).await?;
    client.login(&account.email, &account.password).await?;
    let mailbox = MailboxName::from_utf8(folder)?;
    client.select(&mailbox, true).await?;
    let set: SequenceSet = uid.to_string().parse()?;
    let fetched = client
        .uid_fetch(&set, &FetchItems::peek_section(""))
        .await?;
    client.logout().await.ok();
    let raw = fetched
        .iter()
        .find(|message| message.uid == Some(uid))
        .and_then(|message| message.sections.first())
        .map(|section| section.data.as_slice())
        .ok_or_else(|| Error::Protocol(format!("UID {uid} returned no message body")))?;
    decode_message(raw)
}

fn decode_header(value: &str) -> String {
    let synthetic = format!("X: {value}\r\n\r\n");
    mailparse::parse_headers(synthetic.as_bytes())
        .ok()
        .and_then(|(headers, _)| headers.get_first_value("X"))
        .unwrap_or_else(|| value.to_owned())
}

fn plain_text_part<'a>(part: &ParsedMail<'a>, output: &mut Option<String>) {
    if output.is_some() {
        return;
    }
    if part.ctype.mimetype.eq_ignore_ascii_case("text/plain")
        && part.get_content_disposition().disposition != mailparse::DispositionType::Attachment
    {
        *output = part.get_body().ok();
        return;
    }
    for child in &part.subparts {
        plain_text_part(child, output);
    }
}

/// Decodes MIME headers and the first non-attachment plain-text body.
pub fn decode_message(raw: &[u8]) -> Result<MessageView> {
    let parsed =
        mailparse::parse_mail(raw).map_err(|error| Error::Parse(format!("MIME: {error}")))?;
    let mut body = None;
    plain_text_part(&parsed, &mut body);
    Ok(MessageView {
        subject: parsed
            .headers
            .get_first_value("Subject")
            .unwrap_or_else(|| "(no subject)".into()),
        from: parsed.headers.get_first_value("From").unwrap_or_default(),
        to: parsed.headers.get_first_value("To").unwrap_or_default(),
        date: parsed.headers.get_first_value("Date").unwrap_or_default(),
        body: body.unwrap_or_else(|| {
            "[This message has no plain-text part. HTML is intentionally not rendered.]".into()
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_crlf_and_password_colons_without_exposing_secret() {
        let accounts = parse_accounts("# comment\r\nuser@gmx.de:pa:ss\r\n\r\n");
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].imap_host(), "imap.gmx.net");
        assert_eq!(accounts[0].password, "pa:ss");
        assert!(!format!("{:?}", accounts[0]).contains("pa:ss"));
    }

    #[test]
    fn maps_united_internet_hosts() {
        assert_eq!(
            Account::parse("user@web.de:secret").unwrap().imap_host(),
            "imap.web.de"
        );
        assert_eq!(
            Account::parse("user@gmx.at:secret").unwrap().imap_host(),
            "imap.gmx.net"
        );
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
    fn html_only_message_is_not_rendered_as_markup() {
        let message = decode_message(
            b"Content-Type: text/html; charset=utf-8\r\n\r\n<script>alert(1)</script>",
        )
        .unwrap();
        assert!(message.body.contains("intentionally not rendered"));
        assert!(!message.body.contains("script"));
    }
}
