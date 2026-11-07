//! Coverage-oriented tests for response codes, status data and flags.

use fuckthunder_core::{Flag, ResponseCode, StatusData, StatusItem, TaggedStatus};

#[test]
fn parses_every_response_code() {
    assert_eq!(
        ResponseCode::parse(&["ALERT"]).unwrap(),
        ResponseCode::Alert
    );
    assert_eq!(
        ResponseCode::parse(&["BADCHARSET"]).unwrap(),
        ResponseCode::BadCharset
    );
    assert_eq!(
        ResponseCode::parse(&["BADCHARSET", "UTF-8"]).unwrap(),
        ResponseCode::BadCharset
    );
    assert!(matches!(
        ResponseCode::parse(&["CAPABILITY", "IMAP4rev1", "IDLE"]).unwrap(),
        ResponseCode::Capability(_)
    ));
    assert_eq!(
        ResponseCode::parse(&["PARSE"]).unwrap(),
        ResponseCode::Parse
    );
    assert!(matches!(
        ResponseCode::parse(&["PERMANENTFLAGS", "(\\Seen", "\\Answered)"]).unwrap(),
        ResponseCode::PermanentFlags(_)
    ));
    assert_eq!(
        ResponseCode::parse(&["READ-ONLY"]).unwrap(),
        ResponseCode::ReadOnly
    );
    assert_eq!(
        ResponseCode::parse(&["READ-WRITE"]).unwrap(),
        ResponseCode::ReadWrite
    );
    assert_eq!(
        ResponseCode::parse(&["TRYCREATE"]).unwrap(),
        ResponseCode::TryCreate
    );
    assert_eq!(
        ResponseCode::parse(&["UIDNEXT", "42"]).unwrap(),
        ResponseCode::UidNext(42)
    );
    assert_eq!(
        ResponseCode::parse(&["UIDVALIDITY", "7"]).unwrap(),
        ResponseCode::UidValidity(7)
    );
    assert_eq!(
        ResponseCode::parse(&["UNSEEN", "3"]).unwrap(),
        ResponseCode::Unseen(3)
    );
    assert_eq!(
        ResponseCode::parse(&["APPENDUID", "7", "42"]).unwrap(),
        ResponseCode::AppendUid {
            validity: 7,
            uid: 42
        }
    );
    assert_eq!(
        ResponseCode::parse(&["COPYUID", "7", "1:3", "4:6"]).unwrap(),
        ResponseCode::CopyUid {
            validity: 7,
            source: "1:3".parse().unwrap(),
            target: "4:6".parse().unwrap(),
        }
    );
    assert_eq!(
        ResponseCode::parse(&["HIGHESTMODSEQ", "99"]).unwrap(),
        ResponseCode::HighestModSeq(99)
    );
    assert_eq!(
        ResponseCode::parse(&["NOMODSEQ"]).unwrap(),
        ResponseCode::NoModSeq
    );
    assert_eq!(
        ResponseCode::parse(&["MODIFIED", "5"]).unwrap(),
        ResponseCode::Modified(5)
    );
    assert_eq!(
        ResponseCode::parse(&["CLOSED"]).unwrap(),
        ResponseCode::Closed
    );
    assert_eq!(
        ResponseCode::parse(&["OVERQUOTA"]).unwrap(),
        ResponseCode::OverQuota
    );
    assert_eq!(
        ResponseCode::parse(&["CONTACTADMIN"]).unwrap(),
        ResponseCode::ContactAdmin
    );
    assert_eq!(
        ResponseCode::parse(&["REFERRAL", "imap://other/"]).unwrap(),
        ResponseCode::Referral("imap://other/".into())
    );
    assert_eq!(
        ResponseCode::parse(&["UNAVAILABLE"]).unwrap(),
        ResponseCode::Unavailable
    );
    assert_eq!(
        ResponseCode::parse(&["AUTHENTICATIONFAILED"]).unwrap(),
        ResponseCode::AuthenticationFailed
    );
    assert_eq!(
        ResponseCode::parse(&["AUTHORIZATIONFAILED"]).unwrap(),
        ResponseCode::AuthorizationFailed
    );
    assert_eq!(
        ResponseCode::parse(&["EXPIRED"]).unwrap(),
        ResponseCode::Expired
    );
    assert_eq!(
        ResponseCode::parse(&["PRIVACYREQUIRED"]).unwrap(),
        ResponseCode::PrivacyRequired
    );
    assert_eq!(
        ResponseCode::parse(&["X-CUSTOM", "arg"]).unwrap(),
        ResponseCode::Other("X-CUSTOM arg".into())
    );
}

#[test]
fn rejects_malformed_response_codes() {
    assert!(ResponseCode::parse(&["UIDNEXT"]).is_err());
    assert!(ResponseCode::parse(&["UIDNEXT", "nan"]).is_err());
    assert!(ResponseCode::parse(&["APPENDUID", "7"]).is_err());
    assert!(ResponseCode::parse(&["COPYUID", "7", "1:2"]).is_err());
    assert!(ResponseCode::parse::<&str>(&[]).is_err());
}

#[test]
fn tagged_status_round_trip() {
    assert_eq!(TaggedStatus::parse("ok"), Some(TaggedStatus::Ok));
    assert_eq!(TaggedStatus::parse("NO"), Some(TaggedStatus::No));
    assert_eq!(TaggedStatus::parse("bad"), Some(TaggedStatus::Bad));
    assert_eq!(TaggedStatus::parse("WAT"), None);
    assert_eq!(TaggedStatus::Ok.to_string(), "OK");
    assert_eq!(TaggedStatus::No.to_string(), "NO");
    assert_eq!(TaggedStatus::Bad.to_string(), "BAD");
}

#[test]
fn status_data_accessors() {
    let mut data = StatusData::default();
    data.set(StatusItem::Messages, 7).unwrap();
    data.set(StatusItem::Recent, 2).unwrap();
    data.set(StatusItem::UidNext, 9).unwrap();
    data.set(StatusItem::UidValidity, 3).unwrap();
    data.set(StatusItem::Unseen, 1).unwrap();
    data.set(StatusItem::Size, 4096).unwrap();
    data.set(StatusItem::HighestModSeq, 77).unwrap();
    assert_eq!(data.messages, Some(7));
    assert_eq!(data.recent, Some(2));
    assert_eq!(data.uid_next, Some(9));
    assert_eq!(data.uid_validity, Some(3));
    assert_eq!(data.unseen, Some(1));
    assert_eq!(data.size, Some(4096));
    assert_eq!(data.highest_mod_seq, Some(77));
    assert_eq!(StatusItem::Messages.to_string(), "MESSAGES");
    assert_eq!(StatusItem::Recent.to_string(), "RECENT");
    assert_eq!(StatusItem::UidNext.to_string(), "UIDNEXT");
    assert_eq!(StatusItem::UidValidity.to_string(), "UIDVALIDITY");
    assert_eq!(StatusItem::Unseen.to_string(), "UNSEEN");
    assert_eq!(StatusItem::Size.to_string(), "SIZE");
    assert_eq!(StatusItem::HighestModSeq.to_string(), "HIGHESTMODSEQ");
    assert_eq!(StatusItem::parse("size"), Some(StatusItem::Size));
    assert!(data.set(StatusItem::Messages, u64::MAX).is_err());
}

#[test]
fn flag_parsing_and_rendering() {
    use fuckthunder_core::FlagSet;
    let set = FlagSet::parse(["\\Seen", "\\Answered", "$Custom", "plain"]);
    assert!(set.contains(&Flag::Seen));
    assert!(set.contains(&Flag::Answered));
    assert!(set.contains(&Flag::Keyword("$Custom".into())));
    assert!(set.contains(&Flag::Keyword("plain".into())));
    let wire = set.to_parenthesised();
    assert!(wire.starts_with('(') && wire.ends_with(')'));
    assert!(Flag::parse("\\Deleted") == Flag::Deleted);
    assert!(Flag::parse("\\Draft") == Flag::Draft);
    assert!(Flag::parse("\\Flagged") == Flag::Flagged);
    assert!(Flag::parse("\\Recent") == Flag::Recent);
    assert!(Flag::parse("\\*") == Flag::Wildcard);
}
