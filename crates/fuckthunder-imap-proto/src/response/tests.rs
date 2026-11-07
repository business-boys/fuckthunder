use super::*;
use fuckthunder_core::Flag;

#[test]
fn parses_tagged_ok() {
    let response = parse_response(b"a1 OK [READ-WRITE] SELECT completed\r\n").unwrap();
    match response {
        Response::Tagged {
            tag,
            status,
            code,
            text,
        } => {
            assert_eq!(tag, "a1");
            assert_eq!(status, TaggedStatus::Ok);
            assert_eq!(code, Some(ResponseCode::ReadWrite));
            assert_eq!(text, "SELECT completed");
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn parses_tagged_no_with_code() {
    let response = parse_response(b"a2 NO [TRYCREATE] no such mailbox\r\n").unwrap();
    assert_eq!(response.tagged_status(), Some(TaggedStatus::No));
    assert!(matches!(
        response,
        Response::Tagged {
            code: Some(ResponseCode::TryCreate),
            ..
        }
    ));
}

#[test]
fn parses_capability_greeting() {
    let response =
        parse_response(b"* OK [CAPABILITY IMAP4rev1 IDLE AUTH=PLAIN] ready\r\n").unwrap();
    match response {
        Response::Untagged(Untagged::Ok { code, text }) => {
            assert_eq!(text, "ready");
            match code {
                Some(ResponseCode::Capability(caps)) => {
                    assert!(caps.idle());
                    assert_eq!(caps.auth_mechanisms(), ["PLAIN"]);
                }
                other => panic!("unexpected code {other:?}"),
            }
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn parses_list_entry() {
    let response =
        parse_response(b"* LIST (\\HasNoChildren \\Sent) \"/\" \"Sent Items\"\r\n").unwrap();
    match response {
        Response::Untagged(Untagged::List(entry)) => {
            assert_eq!(entry.delimiter, Some('/'));
            assert_eq!(entry.name.as_str(), "Sent Items");
            assert!(entry.is_selectable());
            assert_eq!(entry.special_use(), Some("\\Sent"));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn parses_status() {
    let response =
        parse_response(b"* STATUS INBOX (MESSAGES 12 UIDNEXT 44 UIDVALIDITY 7 UNSEEN 3)\r\n")
            .unwrap();
    match response {
        Response::Untagged(Untagged::Status { mailbox, data }) => {
            assert_eq!(mailbox.as_str(), "INBOX");
            assert_eq!(data.messages, Some(12));
            assert_eq!(data.uid_next, Some(44));
            assert_eq!(data.unseen, Some(3));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn parses_fetch_with_literal() {
    let response =
        parse_response(b"* 3 FETCH (UID 9 FLAGS (\\Seen) BODY[] {5}\r\nhello)\r\n").unwrap();
    match response {
        Response::Untagged(Untagged::Fetch(data)) => {
            assert_eq!(data.seq, 3);
            assert_eq!(data.uid, Some(9));
            assert_eq!(data.sections[0].data, b"hello");
            assert!(data.flags.unwrap().contains(&Flag::Seen));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn parses_namespace() {
    let response =
        parse_response(b"* NAMESPACE ((\"\" \"/\")) NIL ((\"Shared/\" \"/\"))\r\n").unwrap();
    match response {
        Response::Untagged(Untagged::Namespace(ns)) => {
            assert_eq!(ns.personal[0].prefix.as_str(), "");
            assert_eq!(ns.personal[0].delimiter, Some('/'));
            assert!(ns.other_users.is_empty());
            assert_eq!(ns.shared[0].prefix.as_str(), "Shared/");
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn parses_enabled_and_id() {
    let response = parse_response(b"* ENABLED CONDSTORE UTF8=ACCEPT\r\n").unwrap();
    assert!(matches!(
        response,
        Response::Untagged(Untagged::Enabled(items)) if items.len() == 2
    ));
    let response = parse_response(b"* ID (\"name\" \"Cyrus\" \"version\" NIL)\r\n").unwrap();
    match response {
        Response::Untagged(Untagged::Id(pairs)) => {
            assert_eq!(pairs[0], ("name".into(), Some("Cyrus".into())));
            assert_eq!(pairs[1], ("version".into(), None));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn parses_quota_and_bye() {
    let response = parse_response(b"* QUOTA \"\" (STORAGE 512 1024)\r\n").unwrap();
    assert!(matches!(
        response,
        Response::Untagged(Untagged::Quota {
            usage: 512,
            limit: 1024,
            ..
        })
    ));
    let response = parse_response(b"* BYE [UNAVAILABLE] shutting down\r\n").unwrap();
    assert!(matches!(
        response,
        Response::Untagged(Untagged::Bye {
            code: Some(ResponseCode::Unavailable),
            ..
        })
    ));
}

#[test]
fn parses_continuation() {
    let response = parse_response(b"+ Ready for literal data\r\n").unwrap();
    assert!(matches!(
        response,
        Response::Continuation { text, .. } if text == "Ready for literal data"
    ));
}

#[test]
fn unknown_untagged_is_preserved() {
    let response = parse_response(b"* ESEARCH (TAG \"a1\") UID ALL 1:5\r\n").unwrap();
    match response {
        Response::Untagged(Untagged::Other { atom, .. }) => assert_eq!(atom, "ESEARCH"),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn malformed_responses_fail() {
    assert!(parse_response(b"").is_err());
    assert!(parse_response(b"a1\n").is_err());
    assert!(parse_response(b"a1 MAYBE nope\r\n").is_err());
    assert!(parse_response(b"* STATUS INBOX (BOGUS 1)\r\n").is_err());
    assert!(parse_response(b"* SEARCH 1 x\r\n").is_err());
}
