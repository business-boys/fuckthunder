//! Typed builder for `SEARCH` criteria.

use fuckthunder_core::{quote_if_needed, Error, ImapDateTime, Result, SequenceSet};
use std::str::FromStr;

/// A composable `SEARCH` criteria expression.
///
/// ```
/// use fuckthunder_imap_proto::SearchQuery;
/// let query = SearchQuery::unseen().and(SearchQuery::from("alice@example.org"));
/// assert_eq!(query.to_wire(), "UNSEEN FROM alice@example.org");
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchQuery(String);

impl SearchQuery {
    /// Creates an empty query (matches everything when rendered alone).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `ALL`
    #[must_use]
    pub fn all() -> Self {
        Self("ALL".into())
    }

    /// `UNSEEN`
    #[must_use]
    pub fn unseen() -> Self {
        Self("UNSEEN".into())
    }

    /// `SEEN`
    #[must_use]
    pub fn seen() -> Self {
        Self("SEEN".into())
    }

    /// `ANSWERED`
    #[must_use]
    pub fn answered() -> Self {
        Self("ANSWERED".into())
    }

    /// `UNANSWERED`
    #[must_use]
    pub fn unanswered() -> Self {
        Self("UNANSWERED".into())
    }

    /// `FLAGGED`
    #[must_use]
    pub fn flagged() -> Self {
        Self("FLAGGED".into())
    }

    /// `DELETED`
    #[must_use]
    pub fn deleted() -> Self {
        Self("DELETED".into())
    }

    /// `DRAFT`
    #[must_use]
    pub fn draft() -> Self {
        Self("DRAFT".into())
    }

    /// `RECENT`
    #[must_use]
    pub fn recent() -> Self {
        Self("RECENT".into())
    }

    /// `NEW`
    #[must_use]
    pub fn new_messages() -> Self {
        Self("NEW".into())
    }

    /// `FROM "value"`
    #[must_use]
    pub fn from(value: &str) -> Self {
        Self(format!("FROM {}", quote_if_needed(value)))
    }

    /// `TO "value"`
    #[must_use]
    pub fn to(value: &str) -> Self {
        Self(format!("TO {}", quote_if_needed(value)))
    }

    /// `CC "value"`
    #[must_use]
    pub fn cc(value: &str) -> Self {
        Self(format!("CC {}", quote_if_needed(value)))
    }

    /// `BCC "value"`
    #[must_use]
    pub fn bcc(value: &str) -> Self {
        Self(format!("BCC {}", quote_if_needed(value)))
    }

    /// `SUBJECT "value"`
    #[must_use]
    pub fn subject(value: &str) -> Self {
        Self(format!("SUBJECT {}", quote_if_needed(value)))
    }

    /// `BODY "value"`
    #[must_use]
    pub fn body(value: &str) -> Self {
        Self(format!("BODY {}", quote_if_needed(value)))
    }

    /// `TEXT "value"`
    #[must_use]
    pub fn text(value: &str) -> Self {
        Self(format!("TEXT {}", quote_if_needed(value)))
    }

    /// `HEADER name "value"`
    #[must_use]
    pub fn header(name: &str, value: &str) -> Self {
        Self(format!(
            "HEADER {} {}",
            quote_if_needed(name),
            quote_if_needed(value)
        ))
    }

    /// `KEYWORD name`
    #[must_use]
    pub fn keyword(name: &str) -> Self {
        Self(format!("KEYWORD {}", quote_if_needed(name)))
    }

    /// `UNKEYWORD name`
    #[must_use]
    pub fn unkeyword(name: &str) -> Self {
        Self(format!("UNKEYWORD {}", quote_if_needed(name)))
    }

    /// `BEFORE dd-Mon-yyyy`
    #[must_use]
    pub fn before(date: ImapDateTime) -> Self {
        Self(format!("BEFORE {}", date.to_wire_date()))
    }

    /// `ON dd-Mon-yyyy`
    #[must_use]
    pub fn on(date: ImapDateTime) -> Self {
        Self(format!("ON {}", date.to_wire_date()))
    }

    /// `SINCE dd-Mon-yyyy`
    #[must_use]
    pub fn since(date: ImapDateTime) -> Self {
        Self(format!("SINCE {}", date.to_wire_date()))
    }

    /// `SENTBEFORE`
    #[must_use]
    pub fn sent_before(date: ImapDateTime) -> Self {
        Self(format!("SENTBEFORE {}", date.to_wire_date()))
    }

    /// `SENTON`
    #[must_use]
    pub fn sent_on(date: ImapDateTime) -> Self {
        Self(format!("SENTON {}", date.to_wire_date()))
    }

    /// `SENTSINCE`
    #[must_use]
    pub fn sent_since(date: ImapDateTime) -> Self {
        Self(format!("SENTSINCE {}", date.to_wire_date()))
    }

    /// `LARGER n`
    #[must_use]
    pub fn larger(bytes: u64) -> Self {
        Self(format!("LARGER {bytes}"))
    }

    /// `SMALLER n`
    #[must_use]
    pub fn smaller(bytes: u64) -> Self {
        Self(format!("SMALLER {bytes}"))
    }

    /// `UID set`
    #[must_use]
    pub fn uid(set: &SequenceSet) -> Self {
        Self(format!("UID {set}"))
    }

    /// `set` as a message-sequence criterion.
    #[must_use]
    pub fn sequence(set: &SequenceSet) -> Self {
        Self(set.to_string())
    }

    /// `NOT criteria`
    #[must_use]
    pub fn negate(self) -> Self {
        if self.0.is_empty() {
            return self;
        }
        Self(format!("NOT {}", self.0))
    }

    /// `OR a b`
    #[must_use]
    pub fn or(self, other: Self) -> Self {
        match (self.0.is_empty(), other.0.is_empty()) {
            (true, true) => Self::default(),
            (true, false) => other,
            (false, true) => self,
            (false, false) => Self(format!("OR ({}) ({})", self.0, other.0)),
        }
    }

    /// Conjunction of two criteria (IMAP uses juxtaposition for AND).
    #[must_use]
    pub fn and(self, other: Self) -> Self {
        match (self.0.is_empty(), other.0.is_empty()) {
            (true, true) => Self::default(),
            (true, false) => other,
            (false, true) => self,
            (false, false) => Self(format!("{} {}", self.0, other.0)),
        }
    }

    /// Returns the raw criteria text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns `true` when the query has no criteria.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Renders the query for the wire, falling back to `ALL`.
    #[must_use]
    pub fn to_wire(&self) -> String {
        if self.0.is_empty() {
            "ALL".into()
        } else {
            self.0.clone()
        }
    }
}

impl FromStr for SearchQuery {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self> {
        if text.contains(['\r', '\n']) {
            return Err(Error::Invalid(
                "search criteria must not contain CR or LF".into(),
            ));
        }
        Ok(Self(text.trim().to_owned()))
    }
}

/// Validates that a rendered query is safe to place on the wire.
///
/// # Errors
///
/// Returns [`fuckthunder_core::Error::Invalid`] when CR/LF is present.
pub fn validate(query: &SearchQuery) -> Result<()> {
    let text = query.to_wire();
    if text.contains(['\r', '\n']) {
        return Err(fuckthunder_core::Error::Invalid(
            "search criteria must not contain CR or LF".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_criteria() {
        assert_eq!(SearchQuery::all().to_wire(), "ALL");
        assert_eq!(SearchQuery::unseen().to_wire(), "UNSEEN");
        assert_eq!(SearchQuery::from("a b").to_wire(), "FROM \"a b\"");
        assert_eq!(
            SearchQuery::header("X-Test", "v").to_wire(),
            "HEADER X-Test v"
        );
        assert_eq!(SearchQuery::larger(1024).to_wire(), "LARGER 1024");
    }

    #[test]
    fn dates_render_without_time() {
        let date = ImapDateTime::parse_date("1-Feb-2024").unwrap();
        assert_eq!(SearchQuery::since(date).to_wire(), "SINCE 01-Feb-2024");
    }

    #[test]
    fn composition() {
        let query = SearchQuery::unseen().and(SearchQuery::from("alice"));
        assert_eq!(query.to_wire(), "UNSEEN FROM alice");
        let query = SearchQuery::unseen().or(SearchQuery::flagged());
        assert_eq!(query.to_wire(), "OR (UNSEEN) (FLAGGED)");
        assert_eq!(SearchQuery::unseen().negate().to_wire(), "NOT UNSEEN");
        assert_eq!(
            SearchQuery::new().and(SearchQuery::seen()).to_wire(),
            "SEEN"
        );
        assert_eq!(SearchQuery::new().or(SearchQuery::seen()).to_wire(), "SEEN");
        assert_eq!(SearchQuery::new().to_wire(), "ALL");
    }

    #[test]
    fn validates_control_characters() {
        let query = SearchQuery("SUBJECT \"a\r\nb\"".to_string());
        assert!(validate(&query).is_err());
        assert!(validate(&SearchQuery::all()).is_ok());
    }
}
