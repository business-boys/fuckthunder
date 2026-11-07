//! IMAP flags and keyword sets.

use serde::{Deserialize, Serialize};

/// A message flag: a system flag or a server-defined keyword.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Flag {
    /// `\Seen`
    Seen,
    /// `\Answered`
    Answered,
    /// `\Flagged`
    Flagged,
    /// `\Deleted`
    Deleted,
    /// `\Draft`
    Draft,
    /// `\Recent` (server-assigned, cannot be stored by clients)
    Recent,
    /// `\*` marker in `PERMANENTFLAGS`
    Wildcard,
    /// A keyword such as `$Junk` or `NonJunk`.
    Keyword(String),
}

impl Flag {
    /// Parses a flag atom, matching system flags case-insensitively.
    ///
    /// ```
    /// use fuckthunder_core::Flag;
    /// assert_eq!(Flag::parse("\\seen"), Flag::Seen);
    /// assert_eq!(Flag::parse("$Junk"), Flag::Keyword("$Junk".into()));
    /// ```
    #[must_use]
    pub fn parse(atom: &str) -> Self {
        match atom.to_ascii_lowercase().as_str() {
            "\\seen" => Self::Seen,
            "\\answered" => Self::Answered,
            "\\flagged" => Self::Flagged,
            "\\deleted" => Self::Deleted,
            "\\draft" => Self::Draft,
            "\\recent" => Self::Recent,
            "\\*" => Self::Wildcard,
            _ => Self::Keyword(atom.to_owned()),
        }
    }

    /// Returns `true` when the flag may be stored with `STORE`/`APPEND`.
    #[must_use]
    pub fn is_storable(&self) -> bool {
        !matches!(self, Self::Recent | Self::Wildcard)
    }
}

impl std::fmt::Display for Flag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Seen => f.write_str("\\Seen"),
            Self::Answered => f.write_str("\\Answered"),
            Self::Flagged => f.write_str("\\Flagged"),
            Self::Deleted => f.write_str("\\Deleted"),
            Self::Draft => f.write_str("\\Draft"),
            Self::Recent => f.write_str("\\Recent"),
            Self::Wildcard => f.write_str("\\*"),
            Self::Keyword(keyword) => f.write_str(keyword),
        }
    }
}

/// An ordered, duplicate-free collection of flags.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlagSet(Vec<Flag>);

impl FlagSet {
    /// Creates an empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a flag if absent.
    pub fn insert(&mut self, flag: Flag) {
        if !self.0.contains(&flag) {
            self.0.push(flag);
        }
    }

    /// Removes a flag, returning whether it was present.
    pub fn remove(&mut self, flag: &Flag) -> bool {
        let before = self.0.len();
        self.0.retain(|f| f != flag);
        before != self.0.len()
    }

    /// Returns `true` if the flag is present.
    #[must_use]
    pub fn contains(&self, flag: &Flag) -> bool {
        self.0.contains(flag)
    }

    /// Iterates over the flags.
    pub fn iter(&self) -> impl Iterator<Item = &Flag> {
        self.0.iter()
    }

    /// Number of flags.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns `true` when empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Builds the parenthesised wire form, e.g. `(\Seen $Junk)`.
    #[must_use]
    pub fn to_parenthesised(&self) -> String {
        let inner = self
            .0
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" ");
        format!("({inner})")
    }

    /// Parses flags from atoms as returned by the server.
    ///
    /// ```
    /// use fuckthunder_core::{Flag, FlagSet};
    /// let flags = FlagSet::parse(["\\Seen", "\\Flagged"]);
    /// assert!(flags.contains(&Flag::Seen));
    /// assert_eq!(flags.to_parenthesised(), "(\\Seen \\Flagged)");
    /// ```
    #[must_use]
    pub fn parse<I, S>(atoms: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut set = Self::new();
        for atom in atoms {
            set.insert(Flag::parse(atom.as_ref()));
        }
        set
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_flags_are_case_insensitive() {
        assert_eq!(Flag::parse("\\SEEN"), Flag::Seen);
        assert_eq!(Flag::Seen.to_string(), "\\Seen");
        assert_eq!(Flag::parse("\\*"), Flag::Wildcard);
        assert_eq!(Flag::Wildcard.to_string(), "\\*");
    }

    #[test]
    fn keywords_preserve_case() {
        let flag = Flag::parse("$Junk");
        assert_eq!(flag, Flag::Keyword("$Junk".into()));
        assert_eq!(flag.to_string(), "$Junk");
        assert!(flag.is_storable());
        assert!(!Flag::Recent.is_storable());
    }

    #[test]
    fn set_operations() {
        let mut set = FlagSet::parse(["\\Seen", "\\Seen", "custom"]);
        assert_eq!(set.len(), 2);
        assert!(set.contains(&Flag::Seen));
        assert!(set.remove(&Flag::Seen));
        assert!(!set.remove(&Flag::Seen));
        assert_eq!(set.to_parenthesised(), "(custom)");
    }

    proptest::proptest! {
        #[test]
        fn parenthesised_round_trip(atoms in proptest::collection::vec("[\\\\A-Za-z$]{1,12}", 0..8)) {
            let set = FlagSet::parse(atoms.iter());
            let rendered = set.to_parenthesised();
            proptest::prop_assert!(rendered.starts_with('('));
            proptest::prop_assert!(rendered.ends_with(')'));
        }
    }
}
