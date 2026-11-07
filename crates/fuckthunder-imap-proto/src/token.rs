//! Zero-copy IMAP tokenizer.
//!
//! The tokenizer understands the four lexical shapes of IMAP: atoms, quoted
//! strings, literals (`{n}CRLF...`) and parenthesised lists, plus the bracketed
//! response-code group. It borrows from the input wherever possible and only
//! allocates when a quoted string contains escapes or a literal needs to be
//! materialised by the reader.

use std::borrow::Cow;

use fuckthunder_core::{Error, Result};

/// One lexical token of an IMAP response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token<'a> {
    /// Unquoted atom, e.g. `OK`, `UID`, `\Seen`.
    Atom(&'a [u8]),
    /// Quoted string with escapes already removed.
    Quoted(Cow<'a, [u8]>),
    /// Literal payload (`{12}CRLF` followed by 12 octets).
    Literal(Cow<'a, [u8]>),
    /// Parenthesised list.
    List(Vec<Token<'a>>),
    /// Bracketed response-code group (`[UIDNEXT 42]`).
    Code(Vec<Token<'a>>),
}

impl<'a> Token<'a> {
    /// Returns the raw bytes for string-like tokens.
    #[must_use]
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Atom(bytes) => Some(bytes),
            Self::Quoted(value) | Self::Literal(value) => Some(value.as_ref()),
            Self::List(_) | Self::Code(_) => None,
        }
    }

    /// Returns the token as UTF-8 text, replacing invalid sequences.
    #[must_use]
    pub fn as_text(&self) -> Option<Cow<'_, str>> {
        self.as_bytes().map(String::from_utf8_lossy)
    }

    /// Returns the token as an owned string.
    #[must_use]
    pub fn to_text(&self) -> Option<String> {
        self.as_text().map(Cow::into_owned)
    }

    /// Parses the token as an unsigned integer.
    #[must_use]
    pub fn as_u64(&self) -> Option<u64> {
        let bytes = self.as_bytes()?;
        std::str::from_utf8(bytes).ok()?.parse().ok()
    }

    /// Parses the token as an unsigned 32-bit integer.
    #[must_use]
    pub fn as_u32(&self) -> Option<u32> {
        self.as_u64().and_then(|n| u32::try_from(n).ok())
    }

    /// Returns the list payload, if this is a list.
    #[must_use]
    pub fn as_list(&self) -> Option<&[Token<'a>]> {
        match self {
            Self::List(items) => Some(items),
            _ => None,
        }
    }

    /// Returns `true` for the `NIL` atom.
    #[must_use]
    pub fn is_nil(&self) -> bool {
        matches!(self, Self::Atom(bytes) if bytes.eq_ignore_ascii_case(b"NIL"))
    }
}

/// A cursor over an IMAP response buffer.
#[derive(Clone, Debug)]
pub struct Tokenizer<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> Tokenizer<'a> {
    /// Creates a tokenizer over a complete response (line plus literals).
    #[must_use]
    pub fn new(input: &'a [u8]) -> Self {
        Self { input, pos: 0 }
    }

    /// Current byte offset.
    #[must_use]
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Remaining unread bytes.
    #[must_use]
    pub fn remaining(&self) -> &'a [u8] {
        &self.input[self.pos..]
    }

    /// Returns `true` when only CRLF or whitespace remains.
    #[must_use]
    pub fn at_end(&self) -> bool {
        let rest = self.remaining();
        rest.iter()
            .all(|b| *b == b' ' || *b == b'\r' || *b == b'\n')
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.pos += 1;
        Some(byte)
    }

    fn skip_spaces(&mut self) {
        while matches!(self.peek(), Some(b' ')) {
            self.pos += 1;
        }
    }

    /// Reads all tokens until the end of the response.
    ///
    /// # Errors
    ///
    /// Fails on unbalanced parentheses, unterminated strings, or truncated
    /// literals.
    pub fn tokens(&mut self) -> Result<Vec<Token<'a>>> {
        let mut out = Vec::new();
        loop {
            self.skip_spaces();
            match self.peek() {
                None | Some(b'\r') | Some(b'\n') => break,
                Some(b')') => {
                    return Err(Error::Parse("unbalanced ')' in response".into()));
                }
                Some(_) => out.push(self.token()?),
            }
        }
        Ok(out)
    }

    /// Reads a single token.
    ///
    /// # Errors
    ///
    /// Fails on malformed input.
    pub fn token(&mut self) -> Result<Token<'a>> {
        self.skip_spaces();
        match self.bump() {
            None => Err(Error::Parse("unexpected end of response".into())),
            Some(b'(') => self.list(),
            Some(b'[') => self.code_group(),
            Some(b'"') => self.quoted(),
            Some(b'{') => self.literal(),
            Some(b')') => Err(Error::Parse("unbalanced ')' in response".into())),
            Some(first) => self.atom(first),
        }
    }

    fn list(&mut self) -> Result<Token<'a>> {
        let mut items = Vec::new();
        loop {
            self.skip_spaces();
            match self.peek() {
                None => return Err(Error::Parse("unterminated list".into())),
                Some(b')') => {
                    self.pos += 1;
                    return Ok(Token::List(items));
                }
                Some(b'\r') | Some(b'\n') => {
                    return Err(Error::Parse("CRLF inside list".into()));
                }
                Some(_) => items.push(self.token()?),
            }
        }
    }

    fn code_group(&mut self) -> Result<Token<'a>> {
        let mut items = Vec::new();
        loop {
            self.skip_spaces();
            match self.peek() {
                None => return Err(Error::Parse("unterminated response code".into())),
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Token::Code(items));
                }
                Some(b'\r') | Some(b'\n') => {
                    return Err(Error::Parse("CRLF inside response code".into()));
                }
                Some(_) => items.push(self.token()?),
            }
        }
    }

    fn quoted(&mut self) -> Result<Token<'a>> {
        let start = self.pos;
        let mut escaped = false;
        loop {
            match self.bump() {
                None | Some(b'\r') | Some(b'\n') => {
                    return Err(Error::Parse("unterminated quoted string".into()));
                }
                Some(b'\\') => {
                    escaped = true;
                    self.pos += 1;
                    if self.peek().is_none() {
                        return Err(Error::Parse("dangling escape in quoted string".into()));
                    }
                }
                Some(b'"') => {
                    let raw = &self.input[start..self.pos - 1];
                    if !escaped {
                        return Ok(Token::Quoted(Cow::Borrowed(raw)));
                    }
                    let mut out = Vec::with_capacity(raw.len());
                    let mut iter = raw.iter().copied();
                    while let Some(byte) = iter.next() {
                        if byte == b'\\' {
                            if let Some(next) = iter.next() {
                                out.push(next);
                            }
                        } else {
                            out.push(byte);
                        }
                    }
                    return Ok(Token::Quoted(Cow::Owned(out)));
                }
                Some(_) => {}
            }
        }
    }

    fn literal(&mut self) -> Result<Token<'a>> {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9') | Some(b'+')) {
            self.pos += 1;
        }
        let digits = &self.input[start..self.pos];
        if digits.is_empty() {
            return Err(Error::Parse("literal without length".into()));
        }
        let size_text = std::str::from_utf8(digits)
            .map_err(|_| Error::Parse("non-numeric literal length".into()))?
            .trim_end_matches('+');
        let size: usize = size_text
            .parse()
            .map_err(|_| Error::Parse("invalid literal length".into()))?;
        if self.bump() != Some(b'}') {
            return Err(Error::Parse("literal length not closed by '}'".into()));
        }
        if self.bump() != Some(b'\r') || self.bump() != Some(b'\n') {
            return Err(Error::Parse("literal length not followed by CRLF".into()));
        }
        let end = self
            .pos
            .checked_add(size)
            .ok_or_else(|| Error::Parse("literal length overflow".into()))?;
        if end > self.input.len() {
            return Err(Error::Parse(format!(
                "truncated literal: want {size} bytes, have {}",
                self.input.len() - self.pos
            )));
        }
        let data = &self.input[self.pos..end];
        self.pos = end;
        Ok(Token::Literal(Cow::Borrowed(data)))
    }

    fn atom(&mut self, first: u8) -> Result<Token<'a>> {
        let start = self.pos - 1;
        let _ = first;
        while let Some(byte) = self.peek() {
            // A fetch attribute such as `BODY[HEADER.FIELDS (FROM TO)]` is a
            // single atom even though it contains spaces and parentheses.
            if byte == b'[' {
                self.pos += 1;
                while let Some(inner) = self.peek() {
                    if inner == b']' {
                        break;
                    }
                    self.pos += 1;
                }
                if self.peek() == Some(b']') {
                    self.pos += 1;
                }
                continue;
            }
            if matches!(
                byte,
                b' ' | b'\r' | b'\n' | b'(' | b')' | b'"' | b'{' | b']'
            ) {
                break;
            }
            self.pos += 1;
        }
        Ok(Token::Atom(&self.input[start..self.pos]))
    }
}

/// Parses a complete response into a token stream.
///
/// # Errors
///
/// Fails on malformed input.
pub fn tokenize(input: &[u8]) -> Result<Vec<Token<'_>>> {
    Tokenizer::new(input).tokens()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_atoms_and_lists() {
        let tokens = tokenize(b"* LIST (\\HasNoChildren \\Sent) \"/\" \"Sent Items\"\r\n").unwrap();
        assert_eq!(tokens.len(), 5);
        assert_eq!(tokens[0].as_text().unwrap(), "*");
        assert_eq!(tokens[1].as_text().unwrap(), "LIST");
        let attrs = tokens[2].as_list().unwrap();
        assert_eq!(attrs.len(), 2);
        assert_eq!(attrs[1].as_text().unwrap(), "\\Sent");
        assert_eq!(tokens[4].as_text().unwrap(), "Sent Items");
    }

    #[test]
    fn tokenizes_literals() {
        let input = b"* 1 FETCH (BODY[] {5}\r\nhello)\r\n";
        let tokens = tokenize(input).unwrap();
        let list = tokens[3].as_list().unwrap();
        assert_eq!(list[1].as_text().unwrap(), "hello");
    }

    #[test]
    fn tokenizes_code_groups() {
        let tokens = tokenize(b"* OK [CAPABILITY IMAP4rev1 IDLE] ready\r\n").unwrap();
        assert_eq!(tokens[0].as_text().unwrap(), "*");
        assert!(matches!(tokens[2], Token::Code(_)));
        assert_eq!(tokens[3].as_text().unwrap(), "ready");
    }

    #[test]
    fn quoted_escapes_are_decoded() {
        let tokens = tokenize(b"\"a\\\"b\\\\c\"\r\n").unwrap();
        assert_eq!(tokens[0].as_bytes().unwrap(), b"a\"b\\c");
    }

    #[test]
    fn rejects_malformed() {
        assert!(tokenize(b"(a b\r\n").is_err());
        assert!(tokenize(b"\"abc\r\n").is_err());
        assert!(tokenize(b"{5}\r\nabc").is_err());
        assert!(tokenize(b"{x}\r\n").is_err());
        assert!(tokenize(b"a)\r\n").is_err());
    }

    #[test]
    fn literal_plus_marker() {
        let tokens = tokenize(b"x {3+}\r\nabc\r\n").unwrap();
        assert_eq!(tokens[1].as_bytes().unwrap(), b"abc");
    }

    proptest::proptest! {
        #[test]
        fn arbitrary_input_never_panics(bytes in proptest::collection::vec(0u8..=255, 0..256)) {
            let _ = tokenize(&bytes);
        }
    }
}
