//! Server capability handling.
//!
//! IMAP capabilities are a flat set of atoms, but many of them carry a value
//! (`AUTH=PLAIN`, `COMPRESS=DEFLATE`, `IDLE`, `LITERAL+`, `UTF8=ACCEPT`). This
//! module keeps the raw atoms and adds typed predicates so the client never
//! string-compares in a hot path.

use serde::{Deserialize, Serialize};

/// A set of capability atoms advertised by a server.
///
/// Atoms are stored upper-cased, because RFC 3501 declares capability names
/// case-insensitive.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitySet {
    atoms: Vec<String>,
}

impl CapabilitySet {
    /// Creates an empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds a set from arbitrary atoms, normalising case.
    ///
    /// ```
    /// use fuckthunder_core::CapabilitySet;
    /// let caps = CapabilitySet::from_atoms(["imap4rev1", "idle"]);
    /// assert!(caps.has("IDLE"));
    /// ```
    pub fn from_atoms<I, S>(atoms: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut set = Self::new();
        for atom in atoms {
            set.insert(atom.as_ref());
        }
        set
    }

    /// Inserts one atom, ignoring duplicates.
    pub fn insert(&mut self, atom: &str) {
        let upper = atom.to_ascii_uppercase();
        if !upper.is_empty() && !self.atoms.contains(&upper) {
            self.atoms.push(upper);
        }
    }

    /// Returns `true` if the exact atom is present.
    #[must_use]
    pub fn has(&self, atom: &str) -> bool {
        let upper = atom.to_ascii_uppercase();
        self.atoms.contains(&upper)
    }

    /// Returns `true` if an atom with the given prefix exists (`AUTH=`).
    #[must_use]
    pub fn has_prefix(&self, prefix: &str) -> bool {
        let upper = prefix.to_ascii_uppercase();
        self.atoms.iter().any(|a| a.starts_with(&upper))
    }

    /// Lists the SASL mechanisms advertised via `AUTH=...`.
    #[must_use]
    pub fn auth_mechanisms(&self) -> Vec<String> {
        self.atoms
            .iter()
            .filter_map(|a| a.strip_prefix("AUTH=").map(str::to_owned))
            .collect()
    }

    /// Iterates over the raw atoms.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.atoms.iter().map(String::as_str)
    }

    /// Number of atoms in the set.
    #[must_use]
    pub fn len(&self) -> usize {
        self.atoms.len()
    }

    /// Returns `true` when no capability was advertised.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.atoms.is_empty()
    }

    /// `IMAP4REV1` — the server speaks RFC 3501.
    #[must_use]
    pub fn imap4rev1(&self) -> bool {
        self.has("IMAP4REV1")
    }

    /// `IDLE` — RFC 2177 push notifications are available.
    #[must_use]
    pub fn idle(&self) -> bool {
        self.has("IDLE")
    }

    /// `STARTTLS` — the connection can be upgraded in-band.
    #[must_use]
    pub fn starttls(&self) -> bool {
        self.has("STARTTLS")
    }

    /// `LITERAL+` — non-synchronising literals (RFC 7888).
    #[must_use]
    pub fn literal_plus(&self) -> bool {
        self.has("LITERAL+")
    }

    /// `UTF8=ACCEPT` — RFC 6855 internationalised mailbox names.
    #[must_use]
    pub fn utf8_accept(&self) -> bool {
        self.has("UTF8=ACCEPT")
    }

    /// `MOVE` — RFC 6851 atomic move.
    #[must_use]
    pub fn move_ext(&self) -> bool {
        self.has("MOVE")
    }

    /// `UIDPLUS` — RFC 4315 APPENDUID/COPYUID response codes.
    #[must_use]
    pub fn uidplus(&self) -> bool {
        self.has("UIDPLUS")
    }

    /// `CONDSTORE` — RFC 7162 conditional store.
    #[must_use]
    pub fn condstore(&self) -> bool {
        self.has("CONDSTORE")
    }

    /// `ENABLE` — RFC 5161 extension negotiation.
    #[must_use]
    pub fn enable(&self) -> bool {
        self.has("ENABLE")
    }

    /// `COMPRESS=DEFLATE` — RFC 4978 stream compression.
    #[must_use]
    pub fn compress_deflate(&self) -> bool {
        self.has("COMPRESS=DEFLATE")
    }

    /// `NAMESPACE` — RFC 2342 namespace discovery.
    #[must_use]
    pub fn namespace(&self) -> bool {
        self.has("NAMESPACE")
    }

    /// `SPECIAL-USE` — RFC 6154 special-use mailbox attributes.
    #[must_use]
    pub fn special_use(&self) -> bool {
        self.has("SPECIAL-USE")
    }
}

impl std::fmt::Display for CapabilitySet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.atoms.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_case_and_deduplicates() {
        let caps = CapabilitySet::from_atoms(["idle", "IDLE", "Imap4rev1"]);
        assert_eq!(caps.len(), 2);
        assert!(caps.imap4rev1());
        assert!(caps.idle());
        assert!(!caps.starttls());
    }

    #[test]
    fn extracts_auth_mechanisms() {
        let caps = CapabilitySet::from_atoms(["AUTH=PLAIN", "auth=xoauth2", "IDLE"]);
        let mut mechs = caps.auth_mechanisms();
        mechs.sort();
        assert_eq!(mechs, ["PLAIN", "XOAUTH2"]);
    }

    #[test]
    fn prefix_and_display() {
        let caps = CapabilitySet::from_atoms(["COMPRESS=DEFLATE", "ENABLE"]);
        assert!(caps.has_prefix("compress="));
        assert!(caps.compress_deflate());
        assert_eq!(caps.to_string(), "COMPRESS=DEFLATE ENABLE");
    }

    #[test]
    fn empty_set() {
        let caps = CapabilitySet::new();
        assert!(caps.is_empty());
        assert!(!caps.imap4rev1());
        assert_eq!(caps.to_string(), "");
    }
}
