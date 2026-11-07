//! Fingerprint coverage for every response kind the parity harness models.

use fuckthunder_imap_proto::parse_response;
use fuckthunder_tools::fingerprint;

#[test]
fn fingerprints_tagged_and_continuation() {
    let tagged = fingerprint(&parse_response(b"a1 OK [READ-WRITE] done\r\n").unwrap());
    assert_eq!(tagged.kind, "tagged");
    assert_eq!(tagged.status.as_deref(), Some("OK"));
    assert_eq!(tagged.code.as_deref(), Some("READ-WRITE"));
    assert_eq!(tagged.keyword.as_deref(), Some("done"));

    let cont = fingerprint(&parse_response(b"+ Ready for literal data\r\n").unwrap());
    assert_eq!(cont.kind, "continuation");
    assert_eq!(cont.keyword.as_deref(), Some("Ready for literal data"));
}

#[test]
fn fingerprints_untagged_kinds() {
    let exists = fingerprint(&parse_response(b"* 4 EXISTS\r\n").unwrap());
    assert_eq!(exists.kind, "untagged");
    assert_eq!(exists.keyword.as_deref(), Some("EXISTS"));
    assert_eq!(exists.number, Some(4));

    let expunge = fingerprint(&parse_response(b"* 2 EXPUNGE\r\n").unwrap());
    assert_eq!(expunge.keyword.as_deref(), Some("EXPUNGE"));
    assert_eq!(expunge.number, Some(2));

    let fetch = fingerprint(
        &parse_response(b"* 1 FETCH (UID 41 FLAGS (\\Seen) BODY[] {5}\r\nhello)\r\n").unwrap(),
    );
    assert_eq!(fetch.keyword.as_deref(), Some("FETCH"));
    assert_eq!(fetch.seq, Some(1));
    assert_eq!(fetch.uid, Some(41));
    assert_eq!(fetch.count, Some(1));

    let search = fingerprint(&parse_response(b"* SEARCH 1 2 3\r\n").unwrap());
    assert_eq!(search.keyword.as_deref(), Some("SEARCH"));
    assert_eq!(search.count, Some(3));

    let caps = fingerprint(&parse_response(b"* CAPABILITY IMAP4rev1 IDLE\r\n").unwrap());
    assert_eq!(caps.keyword.as_deref(), Some("CAPABILITY"));

    let list = fingerprint(&parse_response(b"* LIST (\\HasNoChildren) \"/\" INBOX\r\n").unwrap());
    assert_eq!(list.keyword.as_deref(), Some("LIST"));
    assert_eq!(list.mailbox.as_deref(), Some("INBOX"));

    let lsub = fingerprint(&parse_response(b"* LSUB () \"/\" Sent\r\n").unwrap());
    assert_eq!(lsub.keyword.as_deref(), Some("LSUB"));
    assert_eq!(lsub.mailbox.as_deref(), Some("Sent"));

    let status = fingerprint(
        &parse_response(b"* STATUS INBOX (MESSAGES 3 UIDNEXT 8 UIDVALIDITY 7 UNSEEN 1)\r\n")
            .unwrap(),
    );
    assert_eq!(status.keyword.as_deref(), Some("STATUS"));
    assert_eq!(status.mailbox.as_deref(), Some("INBOX"));
    assert_eq!(status.messages, Some(3));

    let enabled = fingerprint(&parse_response(b"* ENABLED CONDSTORE UTF8=ACCEPT\r\n").unwrap());
    assert_eq!(enabled.keyword.as_deref(), Some("ENABLED"));
    assert_eq!(enabled.count, Some(2));

    let id = fingerprint(&parse_response(b"* ID (\"name\" \"fuckthunder\")\r\n").unwrap());
    assert_eq!(id.keyword.as_deref(), Some("ID"));
    assert_eq!(id.count, Some(1));

    let quota = fingerprint(&parse_response(b"* QUOTA \"\" (STORAGE 10 2048)\r\n").unwrap());
    assert_eq!(quota.keyword.as_deref(), Some("QUOTA"));

    let namespace =
        fingerprint(&parse_response(b"* NAMESPACE ((\"\" \"/\")) NIL NIL\r\n").unwrap());
    assert_eq!(namespace.keyword.as_deref(), Some("NAMESPACE"));

    let flags = fingerprint(&parse_response(b"* FLAGS (\\Seen \\Answered)\r\n").unwrap());
    assert_eq!(flags.keyword.as_deref(), Some("FLAGS"));

    let bye = fingerprint(&parse_response(b"* BYE [UNAVAILABLE] off\r\n").unwrap());
    assert_eq!(bye.keyword.as_deref(), Some("BYE"));
    assert_eq!(bye.code.as_deref(), Some("UNAVAILABLE"));

    let preauth = fingerprint(&parse_response(b"* PREAUTH ready\r\n").unwrap());
    assert_eq!(preauth.keyword.as_deref(), Some("PREAUTH"));
    assert_eq!(preauth.status.as_deref(), Some("PREAUTH"));

    let ok = fingerprint(&parse_response(b"* OK [UIDNEXT 9] hi\r\n").unwrap());
    assert_eq!(ok.keyword.as_deref(), Some("OK"));
    assert_eq!(ok.code.as_deref(), Some("UIDNEXT"));

    let esearch = fingerprint(&parse_response(b"* ESEARCH (TAG \"a1\") UID ALL 1:5\r\n").unwrap());
    assert_eq!(esearch.kind, "untagged");
    assert_eq!(esearch.keyword.as_deref(), Some("ESEARCH"));
}
