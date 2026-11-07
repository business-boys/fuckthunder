//! `FETCH` item lists and `STORE` modes.

/// `STORE` mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreAction {
    /// `FLAGS`
    Replace,
    /// `+FLAGS`
    Add,
    /// `-FLAGS`
    Remove,
}

impl std::fmt::Display for StoreAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Replace => "FLAGS",
            Self::Add => "+FLAGS",
            Self::Remove => "-FLAGS",
        })
    }
}

/// Items requested by `FETCH`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FetchItems(Vec<String>);

impl FetchItems {
    /// Creates an empty item list.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a raw item.
    pub fn push(&mut self, item: impl Into<String>) -> &mut Self {
        self.0.push(item.into());
        self
    }

    /// `(FLAGS)`
    #[must_use]
    pub fn flags() -> Self {
        Self::new().with("FLAGS")
    }

    /// `(UID)`
    #[must_use]
    pub fn uid() -> Self {
        Self::new().with("UID")
    }

    /// `(ENVELOPE)`
    #[must_use]
    pub fn envelope() -> Self {
        Self::new().with("ENVELOPE")
    }

    /// `(INTERNALDATE)`
    #[must_use]
    pub fn internal_date() -> Self {
        Self::new().with("INTERNALDATE")
    }

    /// `(RFC822.SIZE)`
    #[must_use]
    pub fn rfc822_size() -> Self {
        Self::new().with("RFC822.SIZE")
    }

    /// `(BODYSTRUCTURE)`
    #[must_use]
    pub fn body_structure() -> Self {
        Self::new().with("BODYSTRUCTURE")
    }

    /// `(BODY[])`
    #[must_use]
    pub fn body() -> Self {
        Self::new().with("BODY[]")
    }

    /// `(BODY.PEEK[...])` so the fetch does not set `\Seen`.
    #[must_use]
    pub fn peek_section(section: &str) -> Self {
        Self::new().with(format!("BODY.PEEK[{section}]"))
    }

    /// `(BODY[...])`
    #[must_use]
    pub fn section(section: &str) -> Self {
        Self::new().with(format!("BODY[{section}]"))
    }

    /// `(BODY[HEADER.FIELDS (..)])`
    #[must_use]
    pub fn header_fields(fields: &[&str]) -> Self {
        let upper = fields
            .iter()
            .map(|f| f.to_ascii_uppercase())
            .collect::<Vec<_>>()
            .join(" ");
        Self::peek_section(&format!("HEADER.FIELDS ({upper})"))
    }

    /// `(MODSEQ)`
    #[must_use]
    pub fn mod_seq() -> Self {
        Self::new().with("MODSEQ")
    }

    /// A typical message-list fetch: UID, flags, envelope, size, internaldate.
    #[must_use]
    pub fn summary() -> Self {
        let mut items = Self::new();
        items
            .push("UID")
            .push("FLAGS")
            .push("ENVELOPE")
            .push("RFC822.SIZE")
            .push("INTERNALDATE");
        items
    }

    fn with(mut self, item: impl Into<String>) -> Self {
        self.0.push(item.into());
        self
    }

    /// Renders the parenthesised item list.
    #[must_use]
    pub fn to_wire(&self) -> String {
        format!("({})", self.0.join(" "))
    }

    /// Returns `true` when the list requests `UID`.
    #[must_use]
    pub fn wants_uid(&self) -> bool {
        self.0.iter().any(|i| i.eq_ignore_ascii_case("UID"))
    }
}
