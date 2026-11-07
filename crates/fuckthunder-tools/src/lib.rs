//! Shared fingerprint used by the parity harness.

use fuckthunder_imap_proto::{Response, Untagged};
use serde::{Deserialize, Serialize};

/// A normalised view of one response, comparable between the legacy-derived
/// Python model and the Rust parser.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fingerprint {
    /// `tagged`, `untagged` or `continuation`.
    pub kind: String,
    /// Untagged keyword or tagged status.
    pub keyword: Option<String>,
    /// Tagged completion status (`OK`, `NO`, `BAD`).
    pub status: Option<String>,
    /// Response-code name (`READ-WRITE`, `TRYCREATE`, ...).
    pub code: Option<String>,
    /// Numeric payload (`EXISTS`/`RECENT`/`EXPUNGE`).
    pub number: Option<u64>,
    /// `FETCH` sequence number.
    pub seq: Option<u32>,
    /// `FETCH` UID.
    pub uid: Option<u32>,
    /// Mailbox name for `LIST`/`STATUS`.
    pub mailbox: Option<String>,
    /// Count of `SEARCH` results or `FETCH` body sections.
    pub count: Option<u64>,
    /// `STATUS MESSAGES` value.
    pub messages: Option<u32>,
}

/// Builds a fingerprint from a parsed Rust response.
#[must_use]
pub fn fingerprint(response: &Response) -> Fingerprint {
    match response {
        Response::Tagged {
            status, code, text, ..
        } => Fingerprint {
            kind: "tagged".into(),
            keyword: Some(text.clone()),
            status: Some(status.to_string()),
            code: code.as_ref().map(|c| code_name(&c.to_string())),
            ..Fingerprint::default()
        },
        Response::Continuation { code, text } => Fingerprint {
            kind: "continuation".into(),
            keyword: Some(text.clone()),
            code: code.as_ref().map(|c| code_name(&c.to_string())),
            ..Fingerprint::default()
        },
        Response::Untagged(untagged) => untagged_fingerprint(untagged),
    }
}

fn untagged_fingerprint(untagged: &Untagged) -> Fingerprint {
    let mut out = Fingerprint {
        kind: "untagged".into(),
        ..Fingerprint::default()
    };
    match untagged {
        Untagged::Capability(_) => out.keyword = Some("CAPABILITY".into()),
        Untagged::List(entry) => {
            out.keyword = Some("LIST".into());
            out.mailbox = Some(entry.name.as_str().to_owned());
        }
        Untagged::Lsub(entry) => {
            out.keyword = Some("LSUB".into());
            out.mailbox = Some(entry.name.as_str().to_owned());
        }
        Untagged::Status { mailbox, data } => {
            out.keyword = Some("STATUS".into());
            out.mailbox = Some(mailbox.as_str().to_owned());
            out.messages = data.messages;
        }
        Untagged::Search(ids) => {
            out.keyword = Some("SEARCH".into());
            out.count = Some(ids.len() as u64);
        }
        Untagged::Flags(_) => out.keyword = Some("FLAGS".into()),
        Untagged::Exists(n) => {
            out.keyword = Some("EXISTS".into());
            out.number = Some(u64::from(*n));
        }
        Untagged::Recent(n) => {
            out.keyword = Some("RECENT".into());
            out.number = Some(u64::from(*n));
        }
        Untagged::Expunge(n) => {
            out.keyword = Some("EXPUNGE".into());
            out.number = Some(u64::from(*n));
        }
        Untagged::Fetch(data) => {
            out.keyword = Some("FETCH".into());
            out.seq = Some(data.seq);
            out.uid = data.uid;
            out.count = Some(data.sections.len() as u64);
        }
        Untagged::Ok { code, .. } => {
            out.keyword = Some("OK".into());
            out.code = code.as_ref().map(|c| code_name(&c.to_string()));
        }
        Untagged::No { code, .. } => {
            out.keyword = Some("NO".into());
            out.code = code.as_ref().map(|c| code_name(&c.to_string()));
        }
        Untagged::Bad { code, .. } => {
            out.keyword = Some("BAD".into());
            out.code = code.as_ref().map(|c| code_name(&c.to_string()));
        }
        Untagged::PreAuth { .. } => {
            out.keyword = Some("PREAUTH".into());
            out.status = Some("PREAUTH".into());
        }
        Untagged::Bye { code, .. } => {
            out.keyword = Some("BYE".into());
            out.code = code.as_ref().map(|c| code_name(&c.to_string()));
        }
        Untagged::Namespace(_) => out.keyword = Some("NAMESPACE".into()),
        Untagged::Enabled(items) => {
            out.keyword = Some("ENABLED".into());
            out.count = Some(items.len() as u64);
        }
        Untagged::Id(pairs) => {
            out.keyword = Some("ID".into());
            out.count = Some(pairs.len() as u64);
        }
        Untagged::Quota { .. } => out.keyword = Some("QUOTA".into()),
        Untagged::Other { atom, .. } => out.keyword = Some(atom.clone()),
    }
    out
}

fn code_name(rendered: &str) -> String {
    rendered
        .split([' ', ']'])
        .next()
        .unwrap_or_default()
        .to_owned()
}
