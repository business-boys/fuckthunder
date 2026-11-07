//! Message sequence sets (`1:5,7,9:*`) used by `FETCH`, `STORE`, `SEARCH`.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// One end of a sequence range.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Bound {
    /// A concrete message sequence number or UID.
    Number(u32),
    /// The `*` wildcard (highest number in the mailbox).
    Star,
}

impl Bound {
    fn parse(text: &str) -> Result<Self> {
        if text == "*" {
            return Ok(Self::Star);
        }
        text.parse::<u32>()
            .map(Self::Number)
            .map_err(|_| Error::Parse(format!("invalid sequence bound: {text}")))
    }
}

impl fmt::Display for Bound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(n) => write!(f, "{n}"),
            Self::Star => f.write_str("*"),
        }
    }
}

/// A contiguous inclusive range within a [`SequenceSet`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    /// Lower bound as written by the caller.
    pub from: Bound,
    /// Upper bound as written by the caller.
    pub to: Bound,
}

impl fmt::Display for Range {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.from, self.to)
    }
}

/// A comma-separated list of sequence ranges.
///
/// ```
/// use fuckthunder_core::{Bound, SequenceSet};
/// let set: SequenceSet = "1:3,7,9:*".parse().unwrap();
/// assert_eq!(set.ranges().len(), 3);
/// assert_eq!(set.to_string(), "1:3,7,9:*");
/// assert_eq!(set.ranges()[2].to, Bound::Star);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SequenceSet {
    ranges: Vec<Range>,
}

impl SequenceSet {
    /// Creates an empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a set containing a single number.
    #[must_use]
    pub fn single(n: u32) -> Self {
        Self {
            ranges: vec![Range {
                from: Bound::Number(n),
                to: Bound::Number(n),
            }],
        }
    }

    /// Creates a set covering everything (`1:*`).
    #[must_use]
    pub fn all() -> Self {
        Self {
            ranges: vec![Range {
                from: Bound::Number(1),
                to: Bound::Star,
            }],
        }
    }

    /// Creates a set from an inclusive numeric range.
    #[must_use]
    pub fn range(from: u32, to: u32) -> Self {
        Self {
            ranges: vec![Range {
                from: Bound::Number(from),
                to: Bound::Number(to),
            }],
        }
    }

    /// Adds a range.
    pub fn push(&mut self, range: Range) {
        self.ranges.push(range);
    }

    /// Returns the ranges in insertion order.
    #[must_use]
    pub fn ranges(&self) -> &[Range] {
        &self.ranges
    }

    /// Returns `true` when the set has no ranges.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    /// Renders the set for the wire.
    #[must_use]
    pub fn to_wire(&self) -> String {
        self.ranges
            .iter()
            .map(|r| {
                if r.from == r.to {
                    r.from.to_string()
                } else {
                    r.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(",")
    }
}

impl FromStr for SequenceSet {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self> {
        if text.is_empty() {
            return Err(Error::Parse("empty sequence set".into()));
        }
        let mut ranges = Vec::new();
        for part in text.split(',') {
            let part = part.trim();
            if part.is_empty() {
                return Err(Error::Parse("empty range in sequence set".into()));
            }
            let mut sides = part.split(':');
            let first = sides.next().unwrap_or_default();
            let from = Bound::parse(first)?;
            let to = match sides.next() {
                Some(second) => {
                    if sides.next().is_some() {
                        return Err(Error::Parse(format!("too many ':' in range: {part}")));
                    }
                    Bound::parse(second)?
                }
                None => from,
            };
            ranges.push(Range { from, to });
        }
        Ok(Self { ranges })
    }
}

impl fmt::Display for SequenceSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_wire())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_renders() {
        let set: SequenceSet = "1:5,7,9:*".parse().unwrap();
        assert_eq!(set.to_wire(), "1:5,7,9:*");
        assert_eq!(set.ranges()[1].from, Bound::Number(7));
        assert_eq!(set.ranges()[1].to, Bound::Number(7));
    }

    #[test]
    fn helpers() {
        assert_eq!(SequenceSet::single(42).to_string(), "42");
        assert_eq!(SequenceSet::all().to_string(), "1:*");
        assert_eq!(SequenceSet::range(3, 9).to_string(), "3:9");
        assert!(SequenceSet::new().is_empty());
    }

    #[test]
    fn rejects_malformed() {
        for bad in ["", ",", "1::2", "1:2:3", "a", "1,", "1:"] {
            assert!(bad.parse::<SequenceSet>().is_err(), "{bad} should fail");
        }
    }

    proptest::proptest! {
        #[test]
        fn round_trip_single(n in 1u32..1_000_000) {
            let set = SequenceSet::single(n);
            let parsed: SequenceSet = set.to_string().parse().unwrap();
            proptest::prop_assert_eq!(set, parsed);
        }

        #[test]
        fn round_trip_range(a in 1u32..1000, b in 1u32..1000) {
            let set = SequenceSet::range(a, b);
            let parsed: SequenceSet = set.to_string().parse().unwrap();
            proptest::prop_assert_eq!(set, parsed);
        }
    }
}
