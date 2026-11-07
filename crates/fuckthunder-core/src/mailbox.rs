//! IMAP mailbox names: modified UTF-7 (RFC 3501 §5.1.3) and wire quoting.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// A mailbox name in Unicode form.
///
/// The wire encoding is *modified UTF-7*: printable US-ASCII passes through,
/// `&` becomes `&-`, and every other code point is encoded as its UTF-16BE
/// representation in a modified BASE64 run delimited by `&` and `-`. The only
/// alphabet change from BASE64 is `/` → `,`; `+` is kept, so a run can never
/// start with the `-` terminator (matching Thunderbird's `nsUnicodeToUTF7`).
///
/// ```
/// use fuckthunder_core::MailboxName;
/// let name = MailboxName::from_utf8("Gesendet").unwrap();
/// assert_eq!(name.to_wire(), "Gesendet");
/// assert_eq!(MailboxName::decode_wire("&AOQ-").unwrap().as_str(), "ä");
/// ```
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MailboxName(String);

impl MailboxName {
    /// Creates a mailbox name from a UTF-8 string.
    ///
    /// # Errors
    ///
    /// Fails when the name is empty, longer than 255 bytes in its wire form, or
    /// contains a NUL character.
    pub fn from_utf8(name: impl Into<String>) -> Result<Self> {
        let name = name.into();
        if name.is_empty() {
            return Err(Error::Invalid("mailbox name must not be empty".into()));
        }
        if name.contains('\0') {
            return Err(Error::Invalid("mailbox name must not contain NUL".into()));
        }
        let encoded = encode_modified_utf7(&name);
        if encoded.len() > 255 {
            return Err(Error::Invalid("mailbox name exceeds 255 wire bytes".into()));
        }
        Ok(Self(name))
    }

    /// Returns the Unicode name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Encodes the name as modified UTF-7 for the wire.
    #[must_use]
    pub fn to_wire(&self) -> String {
        encode_modified_utf7(&self.0)
    }

    /// Encodes the name as a quoted or atom mailbox token ready for a command.
    ///
    /// ```
    /// use fuckthunder_core::MailboxName;
    /// let name = MailboxName::from_utf8("Sent Items").unwrap();
    /// assert_eq!(name.to_wire_token(), "\"Sent Items\"");
    /// ```
    #[must_use]
    pub fn to_wire_token(&self) -> String {
        quote_if_needed(&self.to_wire())
    }

    /// Decodes a modified UTF-7 name received from the server.
    ///
    /// # Errors
    ///
    /// Fails on invalid BASE64, odd UTF-16 byte counts, or unpaired surrogates.
    pub fn decode_wire(wire: &str) -> Result<Self> {
        // An empty name is legal in `NAMESPACE` prefixes (the personal root).
        decode_modified_utf7(wire).map(Self)
    }
}

impl std::fmt::Display for MailboxName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Encodes UTF-8 text into modified UTF-7.
#[must_use]
pub fn encode_modified_utf7(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut utf16: Vec<u8> = Vec::new();

    fn flush(utf16: &mut Vec<u8>, out: &mut String) {
        if utf16.is_empty() {
            return;
        }
        use base64::Engine as _;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&*utf16);
        out.push('&');
        for byte in b64.trim_end_matches('=').bytes() {
            out.push(match byte {
                b'/' => ',',
                other => char::from(other),
            });
        }
        out.push('-');
        utf16.clear();
    }

    for ch in input.chars() {
        if ch == '&' {
            flush(&mut utf16, &mut out);
            out.push_str("&-");
        } else if ('\u{20}'..='\u{7e}').contains(&ch) {
            flush(&mut utf16, &mut out);
            out.push(ch);
        } else {
            let mut buf = [0u16; 2];
            for unit in ch.encode_utf16(&mut buf) {
                utf16.extend_from_slice(&unit.to_be_bytes());
            }
        }
    }
    flush(&mut utf16, &mut out);
    out
}

/// Decodes modified UTF-7 into UTF-8 text.
///
/// # Errors
///
/// Returns [`Error::Parse`] for malformed escapes.
pub fn decode_modified_utf7(input: &str) -> Result<String> {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] != b'&' {
            let start = i;
            while i < bytes.len() && bytes[i] != b'&' {
                i += 1;
            }
            let chunk = std::str::from_utf8(&bytes[start..i])
                .map_err(|_| Error::Parse("non-UTF-8 in mailbox name".into()))?;
            out.push_str(chunk);
            continue;
        }
        // `&` starts an escape: either `&-` for a literal ampersand or a
        // modified-BASE64 run terminated by the first `-`. Since `-` is not in
        // the modified alphabet, the terminator is unambiguous.
        let rest = &input[i + 1..];
        let Some(end_rel) = rest.find('-') else {
            return Err(Error::Parse("unterminated modified UTF-7 escape".into()));
        };
        let run = &rest[..end_rel];
        if run.is_empty() {
            out.push('&');
        } else {
            out.push_str(&decode_run(run)?);
        }
        i = i + 1 + end_rel + 1;
    }
    Ok(out)
}

/// Decodes one modified-BASE64 run into UTF-16 text.
fn decode_run(run: &str) -> Result<String> {
    let mut b64 = String::with_capacity(run.len() + 3);
    for ch in run.chars() {
        b64.push(match ch {
            ',' => '/',
            other => other,
        });
    }
    if b64.len() % 4 == 1 {
        return Err(Error::Parse("invalid modified UTF-7 base64 length".into()));
    }
    while b64.len() % 4 != 0 {
        b64.push('=');
    }
    use base64::Engine as _;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|_| Error::Parse("invalid modified UTF-7 base64".into()))?;
    if raw.len() % 2 != 0 {
        return Err(Error::Parse("odd UTF-16 byte count in mailbox name".into()));
    }
    let units: Vec<u16> = raw
        .chunks_exact(2)
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .collect();
    String::from_utf16(&units)
        .map_err(|_| Error::Parse("unpaired surrogate in mailbox name".into()))
}

/// Returns `true` when an IMAP atom must be quoted before use.
///
/// The check follows RFC 3501 `atom-specials`: controls, space, list wildcards,
/// parentheses, braces, `%`, `"` and `\`.
#[must_use]
pub fn needs_quoting(atom: &str) -> bool {
    atom.is_empty()
        || atom.bytes().any(|b| {
            b <= 0x20 || b == 0x7f || matches!(b, b'(' | b')' | b'{' | b'%' | b'*' | b'"' | b'\\')
        })
}

/// Quotes a string when it is not a valid IMAP atom, escaping `"` and `\`.
#[must_use]
pub fn quote_if_needed(value: &str) -> String {
    if !needs_quoting(value) {
        return value.to_owned();
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3501_example_round_trips() {
        let wire = "~peter/mail/&U,BTFw-/&ZeVnLIqe-";
        let decoded = decode_modified_utf7(wire).unwrap();
        assert_eq!(decoded, "~peter/mail/台北/日本語");
        assert_eq!(encode_modified_utf7(&decoded), wire);
    }

    #[test]
    fn ampersand_and_ascii() {
        assert_eq!(encode_modified_utf7("A&B"), "A&-B");
        assert_eq!(decode_modified_utf7("A&-B").unwrap(), "A&B");
        assert_eq!(encode_modified_utf7("plain"), "plain");
    }

    #[test]
    fn mailbox_name_wire_token() {
        let name = MailboxName::from_utf8("Sent Items").unwrap();
        assert_eq!(name.to_wire_token(), "\"Sent Items\"");
        let atom = MailboxName::from_utf8("INBOX").unwrap();
        assert_eq!(atom.to_wire_token(), "INBOX");
        assert_eq!(atom.as_str(), "INBOX");
    }

    #[test]
    fn quoting_rules() {
        assert!(needs_quoting(""));
        assert!(needs_quoting("a b"));
        assert!(needs_quoting("a\"b"));
        assert!(needs_quoting("a\\b"));
        assert!(!needs_quoting("INBOX"));
        assert_eq!(quote_if_needed("a\"b"), "\"a\\\"b\"");
    }

    #[test]
    fn rejects_bad_input() {
        assert!(MailboxName::from_utf8("").is_err());
        assert!(MailboxName::from_utf8("a\0b").is_err());
        assert!(decode_modified_utf7("&AAA").is_err());
        assert!(decode_modified_utf7("&A-").is_err());
    }

    #[test]
    fn plus_prefixed_base64_runs_are_unambiguous() {
        // U+F800 is the first code point whose UTF-16BE base64 starts with
        // `+`. It must encode as `&+AA-`, not `&-AA-` (which is `&AA-`).
        assert_eq!(encode_modified_utf7("\u{f800}"), "&+AA-");
        assert_eq!(decode_modified_utf7("&+AA-").unwrap(), "\u{f800}");
        assert_eq!(encode_modified_utf7("&AA-"), "&-AA-");
        assert_eq!(decode_modified_utf7("&-AA-").unwrap(), "&AA-");
        assert_eq!(
            encode_modified_utf7("\u{f800}\u{f801}\u{fbff}"),
            "&+AD4Afv,-"
        );
    }

    proptest::proptest! {
        #[test]
        fn round_trip_any_string(s in ".*") {
            let encoded = encode_modified_utf7(&s);
            let decoded = decode_modified_utf7(&encoded).unwrap();
            proptest::prop_assert_eq!(s, decoded);
        }
    }
}
