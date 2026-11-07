//! Coverage-oriented tests for command encoding, FETCH items and SEARCH.

use std::str::FromStr;

use fuckthunder_core::{FlagSet, ImapDateTime, MailboxName, SequenceSet, StatusItem};
use fuckthunder_imap_proto::{Command, FetchItems, SearchQuery, StoreAction};

fn mailbox(name: &str) -> MailboxName {
    MailboxName::from_utf8(name).unwrap()
}

fn wire(command: Command) -> String {
    String::from_utf8(command.encode("a9", false).unwrap().to_bytes()).unwrap()
}

#[test]
fn encodes_session_commands() {
    assert_eq!(wire(Command::noop()), "a9 NOOP\r\n");
    assert_eq!(wire(Command::logout()), "a9 LOGOUT\r\n");
    assert_eq!(wire(Command::starttls()), "a9 STARTTLS\r\n");
    assert_eq!(wire(Command::compress_deflate()), "a9 COMPRESS DEFLATE\r\n");
    assert_eq!(wire(Command::idle()), "a9 IDLE\r\n");
    assert_eq!(wire(Command::unselect()), "a9 UNSELECT\r\n");
    assert_eq!(wire(Command::close()), "a9 CLOSE\r\n");
    assert_eq!(wire(Command::check()), "a9 CHECK\r\n");
    assert_eq!(wire(Command::namespace()), "a9 NAMESPACE\r\n");
    assert_eq!(wire(Command::expunge()), "a9 EXPUNGE\r\n");
    assert_eq!(
        wire(Command::enable(&["CONDSTORE", "QRESYNC"])),
        "a9 ENABLE CONDSTORE QRESYNC\r\n"
    );
    assert_eq!(
        wire(Command::id(&[("name", "fuckthunder"), ("version", "0.1")])),
        "a9 ID (\"name\" \"fuckthunder\" \"version\" \"0.1\")\r\n"
    );
}

#[test]
fn encodes_mailbox_commands() {
    assert_eq!(
        wire(Command::select(&mailbox("INBOX"), false)),
        "a9 SELECT INBOX\r\n"
    );
    assert_eq!(
        wire(Command::select(&mailbox("INBOX"), true)),
        "a9 EXAMINE INBOX\r\n"
    );
    assert_eq!(wire(Command::create(&mailbox("A"))), "a9 CREATE A\r\n");
    assert_eq!(
        wire(Command::create(&mailbox("New Box"))),
        "a9 CREATE \"New Box\"\r\n"
    );
    assert_eq!(wire(Command::delete(&mailbox("Old"))), "a9 DELETE Old\r\n");
    assert_eq!(
        wire(Command::rename(&mailbox("Old"), &mailbox("New"))),
        "a9 RENAME Old New\r\n"
    );
    assert_eq!(
        wire(Command::subscribe(&mailbox("List"))),
        "a9 SUBSCRIBE List\r\n"
    );
    assert_eq!(
        wire(Command::unsubscribe(&mailbox("List"))),
        "a9 UNSUBSCRIBE List\r\n"
    );
    assert_eq!(wire(Command::list("", "*")), "a9 LIST \"\" \"*\"\r\n");
    assert_eq!(wire(Command::lsub("", "*")), "a9 LSUB \"\" \"*\"\r\n");
    assert_eq!(
        wire(Command::status(
            &mailbox("INBOX"),
            &[StatusItem::Messages, StatusItem::Unseen]
        )),
        "a9 STATUS INBOX (MESSAGES UNSEEN)\r\n"
    );
}

#[test]
fn encodes_message_commands() {
    let set = SequenceSet::all();
    let items = FetchItems::summary();
    let wire_fetch = wire(Command::fetch(&set, &items));
    assert!(wire_fetch.starts_with("a9 FETCH 1:* ("), "{wire_fetch}");
    let wire_uid_fetch = wire(Command::uid_fetch(&set, &items));
    assert!(
        wire_uid_fetch.starts_with("a9 UID FETCH 1:* ("),
        "{wire_uid_fetch}"
    );

    let flags = FlagSet::parse(["\\Seen"]);
    assert_eq!(
        wire(Command::store(&set, StoreAction::Add, &flags, false)),
        "a9 STORE 1:* +FLAGS (\\Seen)\r\n"
    );
    assert_eq!(
        wire(Command::uid_store(&set, StoreAction::Remove, &flags, true)),
        "a9 UID STORE 1:* -FLAGS.SILENT (\\Seen)\r\n"
    );
    assert_eq!(
        wire(Command::copy(&set, &mailbox("Archive"))),
        "a9 COPY 1:* Archive\r\n"
    );
    assert_eq!(
        wire(Command::uid_copy(&set, &mailbox("Archive"))),
        "a9 UID COPY 1:* Archive\r\n"
    );
    assert_eq!(
        wire(Command::move_to(&set, &mailbox("Archive"))),
        "a9 MOVE 1:* Archive\r\n"
    );
    assert_eq!(
        wire(Command::uid_move(&set, &mailbox("Archive"))),
        "a9 UID MOVE 1:* Archive\r\n"
    );
    assert_eq!(wire(Command::uid_expunge(&set)), "a9 UID EXPUNGE 1:*\r\n");

    let query = SearchQuery::unseen();
    assert_eq!(wire(Command::search(&query)), "a9 SEARCH UNSEEN\r\n");
    assert_eq!(
        wire(Command::uid_search(&query)),
        "a9 UID SEARCH UNSEEN\r\n"
    );

    let auth = wire(Command::authenticate("PLAIN", Some("dGVzdA==")).unwrap());
    assert_eq!(auth, "a9 AUTHENTICATE PLAIN dGVzdA==\r\n");
    let auth = wire(Command::authenticate("XOAUTH2", None).unwrap());
    assert_eq!(auth, "a9 AUTHENTICATE XOAUTH2\r\n");
    assert!(Command::authenticate("BAD\r\n", None).is_err());
}

#[test]
fn store_action_display() {
    assert_eq!(StoreAction::Replace.to_string(), "FLAGS");
    assert_eq!(StoreAction::Add.to_string(), "+FLAGS");
    assert_eq!(StoreAction::Remove.to_string(), "-FLAGS");
}

#[test]
fn fetch_items_wire_format() {
    assert_eq!(FetchItems::flags().to_wire(), "(FLAGS)");
    assert_eq!(FetchItems::uid().to_wire(), "(UID)");
    assert_eq!(FetchItems::envelope().to_wire(), "(ENVELOPE)");
    assert_eq!(FetchItems::internal_date().to_wire(), "(INTERNALDATE)");
    assert_eq!(FetchItems::rfc822_size().to_wire(), "(RFC822.SIZE)");
    assert_eq!(FetchItems::body_structure().to_wire(), "(BODYSTRUCTURE)");
    assert_eq!(FetchItems::body().to_wire(), "(BODY[])");
    assert_eq!(
        FetchItems::peek_section("HEADER").to_wire(),
        "(BODY.PEEK[HEADER])"
    );
    assert_eq!(FetchItems::section("TEXT").to_wire(), "(BODY[TEXT])");
    assert_eq!(
        FetchItems::header_fields(&["From", "Subject"]).to_wire(),
        "(BODY.PEEK[HEADER.FIELDS (FROM SUBJECT)])"
    );
    assert_eq!(FetchItems::mod_seq().to_wire(), "(MODSEQ)");
    let mut items = FetchItems::new();
    items.push("FLAGS").push("UID");
    assert_eq!(items.to_wire(), "(FLAGS UID)");
    assert!(FetchItems::uid().wants_uid());
    assert!(!FetchItems::flags().wants_uid());
    let summary = FetchItems::summary().to_wire();
    for needle in ["FLAGS", "UID", "ENVELOPE", "INTERNALDATE", "RFC822.SIZE"] {
        assert!(summary.contains(needle), "{summary}");
    }
}

#[test]
fn search_criteria_wire_format() {
    let date = ImapDateTime::parse_date("1-Feb-2024").unwrap();
    assert_eq!(SearchQuery::seen().to_wire(), "SEEN");
    assert_eq!(SearchQuery::answered().to_wire(), "ANSWERED");
    assert_eq!(SearchQuery::unanswered().to_wire(), "UNANSWERED");
    assert_eq!(SearchQuery::flagged().to_wire(), "FLAGGED");
    assert_eq!(SearchQuery::deleted().to_wire(), "DELETED");
    assert_eq!(SearchQuery::draft().to_wire(), "DRAFT");
    assert_eq!(SearchQuery::recent().to_wire(), "RECENT");
    assert_eq!(SearchQuery::new_messages().to_wire(), "NEW");
    assert_eq!(SearchQuery::to("bob").to_wire(), "TO bob");
    assert_eq!(SearchQuery::cc("bob").to_wire(), "CC bob");
    assert_eq!(SearchQuery::bcc("bob").to_wire(), "BCC bob");
    assert_eq!(
        SearchQuery::subject("hi there").to_wire(),
        "SUBJECT \"hi there\""
    );
    assert_eq!(SearchQuery::body("text").to_wire(), "BODY text");
    assert_eq!(SearchQuery::text("text").to_wire(), "TEXT text");
    assert_eq!(SearchQuery::keyword("$Junk").to_wire(), "KEYWORD $Junk");
    assert_eq!(SearchQuery::unkeyword("$Junk").to_wire(), "UNKEYWORD $Junk");
    assert_eq!(SearchQuery::before(date).to_wire(), "BEFORE 01-Feb-2024");
    assert_eq!(SearchQuery::on(date).to_wire(), "ON 01-Feb-2024");
    assert_eq!(
        SearchQuery::sent_before(date).to_wire(),
        "SENTBEFORE 01-Feb-2024"
    );
    assert_eq!(SearchQuery::sent_on(date).to_wire(), "SENTON 01-Feb-2024");
    assert_eq!(
        SearchQuery::sent_since(date).to_wire(),
        "SENTSINCE 01-Feb-2024"
    );
    assert_eq!(SearchQuery::smaller(64).to_wire(), "SMALLER 64");
    assert_eq!(SearchQuery::uid(&SequenceSet::all()).to_wire(), "UID 1:*");
    assert_eq!(
        SearchQuery::sequence(&"2:4".parse().unwrap()).to_wire(),
        "2:4"
    );
    assert_eq!(SearchQuery::all().to_wire(), "ALL");
}

#[test]
fn search_from_str_and_validate() {
    let query = SearchQuery::from_str("  UNSEEN  ").unwrap();
    assert_eq!(query.to_wire(), "UNSEEN");
    assert!(SearchQuery::from_str("BAD\r\n").is_err());
    assert!(fuckthunder_imap_proto::search::validate(&query).is_ok());
}

#[test]
fn parses_extended_response_variants() {
    use fuckthunder_imap_proto::{parse_response, Response, Untagged};

    // NAMESPACE with nested lists.
    let response = parse_response(
        b"* NAMESPACE ((\"\" \"/\") (\"#mh/\" \"/\")) ((\"~\" \"/\")) ((\"#shared/\" \"/\"))\r\n",
    )
    .unwrap();
    match response {
        Response::Untagged(Untagged::Namespace(ns)) => {
            assert_eq!(ns.personal.len(), 2);
            assert_eq!(ns.other_users.len(), 1);
            assert_eq!(ns.shared.len(), 1);
        }
        other => panic!("unexpected {other:?}"),
    }

    // LIST with NIL delimiter.
    let response = parse_response(b"* LIST (\\Noselect) \"/\" \"[Gmail]\"\r\n").unwrap();
    match response {
        Response::Untagged(Untagged::List(entry)) => {
            assert!(!entry.is_selectable());
            assert_eq!(entry.name.as_str(), "[Gmail]");
        }
        other => panic!("unexpected {other:?}"),
    }

    // ESEARCH.
    let response =
        parse_response(b"* ESEARCH (TAG \"a282\") MIN 2 MAX 4 ALL 2:4 COUNT 3\r\n").unwrap();
    assert!(matches!(
        response,
        Response::Untagged(Untagged::Other { ref atom, .. }) if atom == "ESEARCH"
    ));

    // QUOTA and QUOTAROOT.
    let response = parse_response(b"* QUOTA \"\" (STORAGE 10 512)\r\n").unwrap();
    match response {
        Response::Untagged(Untagged::Quota {
            resource,
            usage,
            limit,
            ..
        }) => {
            assert_eq!(resource, "STORAGE");
            assert_eq!(usage, 10);
            assert_eq!(limit, 512);
        }
        other => panic!("unexpected {other:?}"),
    }
    let response = parse_response(b"* QUOTAROOT INBOX \"\" \"#user/alice\"\r\n").unwrap();
    assert!(matches!(
        response,
        Response::Untagged(Untagged::Other { ref atom, .. }) if atom == "QUOTAROOT"
    ));

    // Continuation with a response code.
    let response = parse_response(b"+ [ALERT] say more\r\n").unwrap();
    match response {
        Response::Continuation { code, text } => {
            assert_eq!(code, Some(fuckthunder_core::ResponseCode::Alert));
            assert_eq!(text, "say more");
        }
        other => panic!("unexpected {other:?}"),
    }

    // FETCH with BODYSTRUCTURE, partial BODY and MODSEQ.
    let response = parse_response(
        b"* 7 FETCH (UID 99 MODSEQ (12345) BODYSTRUCTURE (\"TEXT\" \"PLAIN\" (\"CHARSET\" \"UTF-8\") NIL NIL \"7BIT\" 12 1) BODY[HEADER.FIELDS (FROM)]<0> {3}\r\nabc)\r\n",
    )
    .unwrap();
    match response {
        Response::Untagged(Untagged::Fetch(data)) => {
            assert_eq!(data.uid, Some(99));
            assert_eq!(data.mod_seq, Some(12345));
            let body = data.body_structure.unwrap();
            assert_eq!(body.media_type, "TEXT");
            assert_eq!(body.media_subtype, "PLAIN");
        }
        other => panic!("unexpected {other:?}"),
    }

    // Multipart BODYSTRUCTURE.
    let response = parse_response(
        b"* 1 FETCH (BODYSTRUCTURE ((\"TEXT\" \"PLAIN\" NIL NIL NIL \"7BIT\" 5 1)(\"TEXT\" \"HTML\" NIL NIL NIL \"7BIT\" 6 1) \"ALTERNATIVE\"))\r\n",
    )
    .unwrap();
    match response {
        Response::Untagged(Untagged::Fetch(data)) => {
            let body = data.body_structure.unwrap();
            assert_eq!(body.media_type, "MULTIPART");
            assert_eq!(body.media_subtype, "ALTERNATIVE");
            assert_eq!(body.children.len(), 2);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn rejects_malformed_responses() {
    use fuckthunder_imap_proto::parse_response;
    assert!(parse_response(b"").is_err());
    assert!(parse_response(b"\r\n").is_err());
    assert!(parse_response(b"a1\r\n").is_err());
    assert!(parse_response(b"a1 WAT text\r\n").is_err());
    assert!(parse_response(b"* 1 FETCH (UID)\r\n").is_err());
    assert!(parse_response(b"* STATUS\r\n").is_err());
}

#[test]
fn parses_envelope_and_body_details() {
    use fuckthunder_imap_proto::{parse_response, Response, Untagged};

    let response = parse_response(
        b"* 3 FETCH (UID 7 ENVELOPE (\"Wed, 17 Jul 1996 02:23:25 -0700\" \"Hello\" ((\"Alice\" NIL \"alice\" \"example.org\")) ((\"Alice\" NIL \"alice\" \"example.org\")) ((\"Alice\" NIL \"alice\" \"example.org\")) ((NIL NIL \"bob\" \"example.org\")) NIL NIL NIL \"<id@example.org>\") INTERNALDATE \"17-Jul-1996 02:44:25 -0700\" RFC822.SIZE 1234)\r\n",
    )
    .unwrap();
    match response {
        Response::Untagged(Untagged::Fetch(data)) => {
            let envelope = data.envelope.unwrap();
            assert_eq!(envelope.subject.as_deref(), Some("Hello"));
            assert_eq!(envelope.from[0].mailbox.as_deref(), Some("alice"));
            assert_eq!(envelope.to[0].mailbox.as_deref(), Some("bob"));
            assert_eq!(envelope.message_id.as_deref(), Some("<id@example.org>"));
            assert_eq!(data.rfc822_size, Some(1234));
            assert!(data.internal_date.is_some());
        }
        other => panic!("unexpected {other:?}"),
    }

    // Single-part with disposition and extra params.
    let response = parse_response(
        b"* 4 FETCH (BODYSTRUCTURE (\"IMAGE\" \"PNG\" (\"NAME\" \"logo.png\") NIL NIL \"BASE64\" 2048 NIL (\"ATTACHMENT\" (\"FILENAME\" \"logo.png\"))))\r\n",
    )
    .unwrap();
    match response {
        Response::Untagged(Untagged::Fetch(data)) => {
            let body = data.body_structure.unwrap();
            assert_eq!(body.media_type, "IMAGE");
            assert_eq!(body.media_subtype, "PNG");
            assert_eq!(body.size, 2048);
            assert!(body.disposition.is_some());
        }
        other => panic!("unexpected {other:?}"),
    }

    // Unknown FETCH items land in `extra`.
    let response = parse_response(b"* 5 FETCH (X-GM-MSGID 123 X-FOO \"bar\")\r\n").unwrap();
    match response {
        Response::Untagged(Untagged::Fetch(data)) => {
            assert_eq!(data.extra.len(), 2);
        }
        other => panic!("unexpected {other:?}"),
    }
}
