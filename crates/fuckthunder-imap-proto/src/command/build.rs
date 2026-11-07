//! Core command builder: segments, literals and wire framing.

use fuckthunder_core::{quote_if_needed, Error, Result};

use super::{Command, EncodedCommand, Piece, Segment};

impl Command {
    /// Creates an empty command with the given name.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            segments: Vec::new(),
        }
    }

    /// Appends raw text.
    pub fn raw(&mut self, text: impl Into<String>) -> &mut Self {
        self.segments.push(Segment::Text(text.into()));
        self
    }

    /// Appends a quoted string, rejecting CR/LF injection.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`] when the value contains CR or LF.
    pub fn quoted(&mut self, value: &str) -> Result<&mut Self> {
        reject_control(value)?;
        self.segments.push(Segment::Text(quote_if_needed(value)));
        Ok(self)
    }

    /// Appends an unquoted atom after validating it.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`] when the atom is empty or contains forbidden
    /// bytes.
    pub fn atom(&mut self, value: &str) -> Result<&mut Self> {
        if value.is_empty()
            || value.bytes().any(|b| {
                b <= 0x20
                    || b == 0x7f
                    || matches!(b, b'(' | b')' | b'{' | b'%' | b'*' | b'"' | b'\\')
            })
        {
            return Err(Error::Invalid(format!("not a valid IMAP atom: {value:?}")));
        }
        self.segments.push(Segment::Text(value.to_owned()));
        Ok(self)
    }

    /// Appends a literal payload.
    pub fn literal(&mut self, data: Vec<u8>) -> &mut Self {
        self.segments.push(Segment::Literal(data));
        self
    }

    /// Returns the command name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Encodes the command with a tag.
    ///
    /// # Errors
    ///
    /// Fails when the tag is not a valid atom.
    pub fn encode(&self, tag: &str, literal_plus: bool) -> Result<EncodedCommand> {
        if tag.is_empty()
            || !tag
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        {
            return Err(Error::Invalid(format!("invalid command tag {tag:?}")));
        }
        let mut pieces = Vec::new();
        let mut current = format!("{tag} {}", self.name).into_bytes();
        for segment in &self.segments {
            match segment {
                Segment::Text(text) => {
                    current.push(b' ');
                    current.extend_from_slice(text.as_bytes());
                }
                Segment::Literal(data) => {
                    current.push(b' ');
                    if literal_plus {
                        current.extend_from_slice(format!("{{{}+}}\r\n", data.len()).as_bytes());
                        pieces.push(Piece::Bytes(std::mem::take(&mut current)));
                        pieces.push(Piece::Literal {
                            data: data.clone(),
                            non_sync: true,
                        });
                    } else {
                        current.extend_from_slice(format!("{{{}}}\r\n", data.len()).as_bytes());
                        pieces.push(Piece::Bytes(std::mem::take(&mut current)));
                        pieces.push(Piece::Literal {
                            data: data.clone(),
                            non_sync: false,
                        });
                    }
                }
            }
        }
        current.extend_from_slice(b"\r\n");
        pieces.push(Piece::Bytes(current));
        Ok(EncodedCommand { pieces })
    }
}

fn reject_control(value: &str) -> Result<()> {
    if value.contains(['\r', '\n', '\0']) {
        return Err(Error::Invalid(
            "value must not contain CR, LF or NUL".into(),
        ));
    }
    Ok(())
}
