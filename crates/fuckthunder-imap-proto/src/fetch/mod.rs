//! `FETCH` data: envelopes, body structures and body sections.

use fuckthunder_core::{Envelope, Error, FlagSet, ImapDateTime, Result};

use crate::token::Token;

mod bodystructure;
mod envelope;

pub use bodystructure::parse_body_structure;
pub use envelope::parse_envelope;

/// One `BODY[...]` section returned by `FETCH`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BodySection {
    /// Section spec as written by the server, e.g. `HEADER.FIELDS (FROM TO)`.
    pub spec: String,
    /// Partial-fetch origin and length, when `BODY[]<origin.len>` was used.
    pub partial: Option<(u32, u32)>,
    /// Raw section bytes.
    pub data: Vec<u8>,
}

/// Parsed body structure (RFC 3501 §7.4.2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BodyStructure {
    /// Top-level media type (`TEXT`, `MULTIPART`, ...).
    pub media_type: String,
    /// Media subtype (`PLAIN`, `MIXED`, ...).
    pub media_subtype: String,
    /// Content-Type parameters.
    pub params: Vec<(String, String)>,
    /// Content-ID.
    pub id: Option<String>,
    /// Content-Description.
    pub description: Option<String>,
    /// Content-Transfer-Encoding.
    pub encoding: String,
    /// Octet count.
    pub size: u64,
    /// Line count for `TEXT` parts.
    pub lines: Option<u64>,
    /// MD5 digest when the server sends one.
    pub md5: Option<String>,
    /// Content-Disposition and its parameters.
    pub disposition: Option<(String, Vec<(String, String)>)>,
    /// Content-Language values.
    pub language: Vec<String>,
    /// Content-Location.
    pub location: Option<String>,
    /// Child parts of a `multipart/*` body.
    pub children: Vec<BodyStructure>,
    /// Embedded message envelope for `message/rfc822`.
    pub envelope: Option<Envelope>,
    /// Embedded message structure for `message/rfc822`.
    pub nested: Option<Box<BodyStructure>>,
}

impl BodyStructure {
    /// Returns `true` when this part has child parts.
    #[must_use]
    pub fn is_multipart(&self) -> bool {
        self.media_type.eq_ignore_ascii_case("MULTIPART")
    }

    /// Returns `true` when the part is an attached/embedded message.
    #[must_use]
    pub fn is_message(&self) -> bool {
        self.media_type.eq_ignore_ascii_case("MESSAGE")
            && self.media_subtype.eq_ignore_ascii_case("RFC822")
    }
}

/// Everything a single `FETCH` response can carry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FetchData {
    /// Message sequence number.
    pub seq: u32,
    /// UID, present when the fetch asked for `UID`.
    pub uid: Option<u32>,
    /// Message flags.
    pub flags: Option<FlagSet>,
    /// `INTERNALDATE`.
    pub internal_date: Option<ImapDateTime>,
    /// `RFC822.SIZE`.
    pub rfc822_size: Option<u64>,
    /// Parsed `ENVELOPE`.
    pub envelope: Option<Envelope>,
    /// Parsed `BODYSTRUCTURE`.
    pub body_structure: Option<BodyStructure>,
    /// Any `BODY[...]` sections in wire order.
    pub sections: Vec<BodySection>,
    /// `MODSEQ` (RFC 7162).
    pub mod_seq: Option<u64>,
    /// Items this crate does not model, kept as lossy text for diagnostics.
    pub extra: Vec<(String, String)>,
}

/// Parses the inside of a `FETCH (...)` list, given the sequence number.
///
/// # Errors
///
/// Returns [`Error::Parse`] when a known item is malformed.
pub fn parse_fetch_items(seq: u32, items: &[Token<'_>]) -> Result<FetchData> {
    let mut data = FetchData {
        seq,
        ..FetchData::default()
    };
    let mut index = 0;
    while index < items.len() {
        let key = items[index]
            .to_text()
            .ok_or_else(|| Error::Parse("FETCH item name is not a string".into()))?;
        index += 1;
        let Some(value) = items.get(index) else {
            return Err(Error::Parse(format!("FETCH item {key} has no value")));
        };
        index += 1;
        let upper = key.to_ascii_uppercase();
        match upper.as_str() {
            "UID" => data.uid = Some(as_u32(value)?),
            "FLAGS" => data.flags = Some(parse_flags(value)?),
            "INTERNALDATE" => {
                let text = value
                    .to_text()
                    .ok_or_else(|| Error::Parse("INTERNALDATE is not a string".into()))?;
                data.internal_date = Some(ImapDateTime::parse(&text)?);
            }
            "RFC822.SIZE" => data.rfc822_size = Some(as_u64(value)?),
            "ENVELOPE" => data.envelope = Some(parse_envelope(value)?),
            "BODY" | "BODYSTRUCTURE" => data.body_structure = Some(parse_body_structure(value)?),
            "MODSEQ" => {
                let list = value
                    .as_list()
                    .ok_or_else(|| Error::Parse("MODSEQ is not a list".into()))?;
                let first = list
                    .first()
                    .ok_or_else(|| Error::Parse("empty MODSEQ list".into()))?;
                data.mod_seq = Some(as_u64(first)?);
            }
            "RFC822" | "RFC822.HEADER" | "RFC822.TEXT" => {
                data.sections.push(BodySection {
                    spec: upper,
                    partial: None,
                    data: as_bytes(value)?.to_vec(),
                });
            }
            _ if upper.starts_with("BODY[") => {
                let inner = key.split_once('[').map_or("", |(_, rest)| rest);
                let spec = inner.split(']').next().unwrap_or(inner).to_owned();
                data.sections.push(BodySection {
                    spec,
                    partial: parse_partial(&key),
                    data: as_bytes(value)?.to_vec(),
                });
            }
            _ => data.extra.push((key, value.to_text().unwrap_or_default())),
        }
    }
    Ok(data)
}

fn parse_partial(key: &str) -> Option<(u32, u32)> {
    let inner = key.rsplit_once('<')?.1.strip_suffix('>')?;
    let (origin, len) = inner.split_once('.')?;
    Some((origin.parse().ok()?, len.parse().ok()?))
}

fn as_bytes<'a>(token: &'a Token<'_>) -> Result<&'a [u8]> {
    token
        .as_bytes()
        .ok_or_else(|| Error::Parse("expected a string token".into()))
}

fn as_u32(token: &Token<'_>) -> Result<u32> {
    token
        .as_u32()
        .ok_or_else(|| Error::Parse("expected a 32-bit number".into()))
}

fn as_u64(token: &Token<'_>) -> Result<u64> {
    token
        .as_u64()
        .ok_or_else(|| Error::Parse("expected a number".into()))
}

fn parse_flags(token: &Token<'_>) -> Result<FlagSet> {
    let list = token
        .as_list()
        .ok_or_else(|| Error::Parse("FLAGS is not a list".into()))?;
    let atoms: Vec<String> = list.iter().filter_map(Token::to_text).collect();
    Ok(FlagSet::parse(atoms))
}

/// Returns `None` for `NIL`, otherwise the token text unless empty.
fn optional_text(token: &Token<'_>) -> Option<String> {
    if token.is_nil() {
        return None;
    }
    token.to_text().filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests;
