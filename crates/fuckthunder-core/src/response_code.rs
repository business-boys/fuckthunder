//! IMAP response codes (`[UIDVALIDITY 42]`, `[APPENDUID ...]`, ...).

use serde::{Deserialize, Serialize};

use crate::capability::CapabilitySet;
use crate::error::{Error, Result};
use crate::flags::FlagSet;
use crate::seq::SequenceSet;

/// Tagged completion status of a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaggedStatus {
    /// `OK`
    Ok,
    /// `NO`
    No,
    /// `BAD`
    Bad,
}

impl TaggedStatus {
    /// Parses the status keyword.
    #[must_use]
    pub fn parse(atom: &str) -> Option<Self> {
        match atom.to_ascii_uppercase().as_str() {
            "OK" => Some(Self::Ok),
            "NO" => Some(Self::No),
            "BAD" => Some(Self::Bad),
            _ => None,
        }
    }
}

impl std::fmt::Display for TaggedStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Ok => "OK",
            Self::No => "NO",
            Self::Bad => "BAD",
        })
    }
}

/// The machine-readable part of a response, e.g. `[READ-WRITE]`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ResponseCode {
    /// `ALERT`
    Alert,
    /// `BADCHARSET`
    BadCharset,
    /// `CAPABILITY ...`
    Capability(CapabilitySet),
    /// `PARSE`
    Parse,
    /// `PERMANENTFLAGS (...)`
    PermanentFlags(FlagSet),
    /// `READ-ONLY`
    ReadOnly,
    /// `READ-WRITE`
    ReadWrite,
    /// `TRYCREATE`
    TryCreate,
    /// `UIDNEXT n`
    UidNext(u32),
    /// `UIDVALIDITY n`
    UidValidity(u32),
    /// `UNSEEN n`
    Unseen(u32),
    /// `APPENDUID uidvalidity uid`
    AppendUid {
        /// Mailbox UID validity.
        validity: u32,
        /// UID assigned to the appended message.
        uid: u32,
    },
    /// `COPYUID uidvalidity source-set target-set`
    CopyUid {
        /// Mailbox UID validity.
        validity: u32,
        /// Source UIDs.
        source: SequenceSet,
        /// Destination UIDs.
        target: SequenceSet,
    },
    /// `HIGHESTMODSEQ n`
    HighestModSeq(u64),
    /// `NOMODSEQ`
    NoModSeq,
    /// `MODIFIED n`
    Modified(u64),
    /// `CLOSED`
    Closed,
    /// `OVERQUOTA` (RFC 2087)
    OverQuota,
    /// `CONTACTADMIN`
    ContactAdmin,
    /// `REFERRAL` (RFC 2221)
    Referral(String),
    /// `UNAVAILABLE` (RFC 5530)
    Unavailable,
    /// `AUTHENTICATIONFAILED`
    AuthenticationFailed,
    /// `AUTHORIZATIONFAILED`
    AuthorizationFailed,
    /// `EXPIRED`
    Expired,
    /// `PRIVACYREQUIRED`
    PrivacyRequired,
    /// Any code this crate does not model, preserved verbatim.
    Other(String),
}

impl ResponseCode {
    /// Parses a response code from the token stream found between `[` and `]`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Parse`] when the code needs arguments and they are
    /// malformed.
    pub fn parse<S: AsRef<str>>(tokens: &[S]) -> Result<Self> {
        let words: Vec<&str> = tokens.iter().map(AsRef::as_ref).collect();
        let Some((name, args)) = words.split_first() else {
            return Err(Error::Parse("empty response code".into()));
        };
        let upper = name.to_ascii_uppercase();
        Ok(match upper.as_str() {
            "ALERT" => Self::Alert,
            "BADCHARSET" => Self::BadCharset,
            "CAPABILITY" => Self::Capability(CapabilitySet::from_atoms(args.iter().copied())),
            "PARSE" => Self::Parse,
            "PERMANENTFLAGS" => {
                let cleaned: Vec<&str> = args
                    .iter()
                    .map(|t| t.trim_matches(|c| c == '(' || c == ')'))
                    .filter(|t| !t.is_empty())
                    .collect();
                Self::PermanentFlags(FlagSet::parse(cleaned))
            }
            "READ-ONLY" => Self::ReadOnly,
            "READ-WRITE" => Self::ReadWrite,
            "TRYCREATE" => Self::TryCreate,
            "UIDNEXT" => Self::UidNext(parse_u32(args.first())?),
            "UIDVALIDITY" => Self::UidValidity(parse_u32(args.first())?),
            "UNSEEN" => Self::Unseen(parse_u32(args.first())?),
            "APPENDUID" => Self::AppendUid {
                validity: parse_u32(args.first())?,
                uid: parse_u32(args.get(1))?,
            },
            "COPYUID" => Self::CopyUid {
                validity: parse_u32(args.first())?,
                source: parse_set(args.get(1))?,
                target: parse_set(args.get(2))?,
            },
            "HIGHESTMODSEQ" => Self::HighestModSeq(parse_u64(args.first())?),
            "NOMODSEQ" => Self::NoModSeq,
            "MODIFIED" => Self::Modified(parse_u64(args.first())?),
            "CLOSED" => Self::Closed,
            "OVERQUOTA" => Self::OverQuota,
            "CONTACTADMIN" => Self::ContactAdmin,
            "REFERRAL" => Self::Referral(args.join(" ")),
            "UNAVAILABLE" => Self::Unavailable,
            "AUTHENTICATIONFAILED" => Self::AuthenticationFailed,
            "AUTHORIZATIONFAILED" => Self::AuthorizationFailed,
            "EXPIRED" => Self::Expired,
            "PRIVACYREQUIRED" => Self::PrivacyRequired,
            _ => Self::Other(words.join(" ")),
        })
    }
}

fn parse_u32(value: Option<&&str>) -> Result<u32> {
    value
        .ok_or_else(|| Error::Parse("missing numeric response-code argument".into()))?
        .parse()
        .map_err(|_| Error::Parse("invalid numeric response-code argument".into()))
}

fn parse_u64(value: Option<&&str>) -> Result<u64> {
    value
        .ok_or_else(|| Error::Parse("missing numeric response-code argument".into()))?
        .parse()
        .map_err(|_| Error::Parse("invalid numeric response-code argument".into()))
}

fn parse_set(value: Option<&&str>) -> Result<SequenceSet> {
    value
        .ok_or_else(|| Error::Parse("missing sequence-set response-code argument".into()))?
        .parse()
}

impl std::fmt::Display for ResponseCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Alert => f.write_str("ALERT"),
            Self::BadCharset => f.write_str("BADCHARSET"),
            Self::Capability(caps) => write!(f, "CAPABILITY {caps}"),
            Self::Parse => f.write_str("PARSE"),
            Self::PermanentFlags(flags) => write!(f, "PERMANENTFLAGS {}", flags.to_parenthesised()),
            Self::ReadOnly => f.write_str("READ-ONLY"),
            Self::ReadWrite => f.write_str("READ-WRITE"),
            Self::TryCreate => f.write_str("TRYCREATE"),
            Self::UidNext(n) => write!(f, "UIDNEXT {n}"),
            Self::UidValidity(n) => write!(f, "UIDVALIDITY {n}"),
            Self::Unseen(n) => write!(f, "UNSEEN {n}"),
            Self::AppendUid { validity, uid } => write!(f, "APPENDUID {validity} {uid}"),
            Self::CopyUid {
                validity,
                source,
                target,
            } => write!(f, "COPYUID {validity} {source} {target}"),
            Self::HighestModSeq(n) => write!(f, "HIGHESTMODSEQ {n}"),
            Self::NoModSeq => f.write_str("NOMODSEQ"),
            Self::Modified(n) => write!(f, "MODIFIED {n}"),
            Self::Closed => f.write_str("CLOSED"),
            Self::OverQuota => f.write_str("OVERQUOTA"),
            Self::ContactAdmin => f.write_str("CONTACTADMIN"),
            Self::Referral(text) => write!(f, "REFERRAL {text}"),
            Self::Unavailable => f.write_str("UNAVAILABLE"),
            Self::AuthenticationFailed => f.write_str("AUTHENTICATIONFAILED"),
            Self::AuthorizationFailed => f.write_str("AUTHORIZATIONFAILED"),
            Self::Expired => f.write_str("EXPIRED"),
            Self::PrivacyRequired => f.write_str("PRIVACYREQUIRED"),
            Self::Other(text) => f.write_str(text),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flags::Flag;

    #[test]
    fn parses_simple_codes() {
        assert_eq!(
            ResponseCode::parse(&["READ-WRITE"]).unwrap(),
            ResponseCode::ReadWrite
        );
        assert_eq!(
            ResponseCode::parse(&["TRYCREATE"]).unwrap(),
            ResponseCode::TryCreate
        );
        assert_eq!(
            ResponseCode::parse(&["ALERT"]).unwrap(),
            ResponseCode::Alert
        );
    }

    #[test]
    fn parses_capabilities_and_flags() {
        let caps = ResponseCode::parse(&["CAPABILITY", "IMAP4REV1", "IDLE"]).unwrap();
        assert_eq!(
            caps,
            ResponseCode::Capability(CapabilitySet::from_atoms(["IMAP4REV1", "IDLE"]))
        );
        let flags = ResponseCode::parse(&["PERMANENTFLAGS", "(\\Seen", "\\*", "$Junk)"]).unwrap();
        match flags {
            ResponseCode::PermanentFlags(set) => {
                assert!(set.contains(&Flag::Seen));
                assert!(set.contains(&Flag::Wildcard));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parses_uidplus_codes() {
        assert_eq!(
            ResponseCode::parse(&["APPENDUID", "1234", "42"]).unwrap(),
            ResponseCode::AppendUid {
                validity: 1234,
                uid: 42
            }
        );
        match ResponseCode::parse(&["COPYUID", "1234", "1:3", "10:12"]).unwrap() {
            ResponseCode::CopyUid { source, target, .. } => {
                assert_eq!(source.to_string(), "1:3");
                assert_eq!(target.to_string(), "10:12");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn unknown_codes_are_preserved() {
        let code = ResponseCode::parse(&["SOMETHING", "NEW"]).unwrap();
        assert_eq!(code, ResponseCode::Other("SOMETHING NEW".into()));
        assert_eq!(code.to_string(), "SOMETHING NEW");
    }

    #[test]
    fn malformed_numeric_codes_fail() {
        assert!(ResponseCode::parse(&["UIDNEXT"]).is_err());
        assert!(ResponseCode::parse(&["UIDNEXT", "abc"]).is_err());
        assert!(ResponseCode::parse::<&str>(&[]).is_err());
    }
}
