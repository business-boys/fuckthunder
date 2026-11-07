//! IMAP date and date-time parsing/formatting (RFC 3501 §9).

use chrono::{DateTime, FixedOffset, NaiveDate, TimeZone};

use crate::error::{Error, Result};

/// An IMAP `date-time` such as `17-Jul-1996 02:44:25 -0700`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ImapDateTime(DateTime<FixedOffset>);

impl ImapDateTime {
    /// Wraps an already-parsed chrono value.
    #[must_use]
    pub fn new(value: DateTime<FixedOffset>) -> Self {
        Self(value)
    }

    /// Parses a full IMAP date-time.
    ///
    /// ```
    /// use fuckthunder_core::ImapDateTime;
    /// let dt = ImapDateTime::parse("17-Jul-1996 02:44:25 -0700").unwrap();
    /// assert_eq!(dt.to_string(), "17-Jul-1996 02:44:25 -0700");
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`Error::Parse`] when the text does not match the grammar.
    pub fn parse(text: &str) -> Result<Self> {
        let value = DateTime::parse_from_str(text.trim(), "%d-%b-%Y %H:%M:%S %z")
            .map_err(|e| Error::Parse(format!("invalid IMAP date-time: {e}")))?;
        Ok(Self(value))
    }

    /// Parses an IMAP `date` (`17-Jul-1996`) into midnight UTC.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Parse`] for invalid dates.
    pub fn parse_date(text: &str) -> Result<Self> {
        let date = NaiveDate::parse_from_str(text.trim(), "%d-%b-%Y")
            .map_err(|e| Error::Parse(format!("invalid IMAP date: {e}")))?;
        let offset = FixedOffset::east_opt(0).expect("UTC offset is always valid");
        let value = offset
            .from_local_datetime(&date.and_hms_opt(0, 0, 0).expect("midnight is valid"))
            .single()
            .ok_or_else(|| Error::Parse("ambiguous IMAP date".into()))?;
        Ok(Self(value))
    }

    /// Returns the chrono value.
    #[must_use]
    pub fn inner(&self) -> DateTime<FixedOffset> {
        self.0
    }

    /// Formats the full date-time for `APPEND`/`SEARCH`.
    #[must_use]
    pub fn to_wire(&self) -> String {
        self.0.format("%d-%b-%Y %H:%M:%S %z").to_string()
    }

    /// Formats only the date part (`dd-Mon-yyyy`) for `SEARCH`.
    #[must_use]
    pub fn to_wire_date(&self) -> String {
        self.0.format("%d-%b-%Y").to_string()
    }
}

impl std::fmt::Display for ImapDateTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_wire())
    }
}

impl From<DateTime<FixedOffset>> for ImapDateTime {
    fn from(value: DateTime<FixedOffset>) -> Self {
        Self(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rfc_example() {
        let dt = ImapDateTime::parse("17-Jul-1996 02:44:25 -0700").unwrap();
        assert_eq!(dt.to_wire(), "17-Jul-1996 02:44:25 -0700");
        assert_eq!(dt.inner().timestamp(), 837_596_665);
    }

    #[test]
    fn parses_date_only() {
        let dt = ImapDateTime::parse_date("1-Feb-2024").unwrap();
        assert_eq!(dt.to_wire_date(), "01-Feb-2024");
    }

    #[test]
    fn rejects_garbage() {
        assert!(ImapDateTime::parse("yesterday").is_err());
        assert!(ImapDateTime::parse_date("32-Feb-2024").is_err());
    }
}
