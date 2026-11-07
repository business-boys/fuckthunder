use super::*;
use crate::token::{tokenize, Token};

fn list_from(input: &str) -> Vec<Token<'_>> {
    let owned: &'static str = Box::leak(input.to_owned().into_boxed_str());
    tokenize(owned.as_bytes()).unwrap()
}

#[test]
fn parses_basic_fetch() {
    let tokens = list_from(
        "UID 42 FLAGS (\\Seen $Junk) INTERNALDATE \"17-Jul-1996 02:44:25 -0700\" RFC822.SIZE 1234",
    );
    let data = parse_fetch_items(7, &tokens).unwrap();
    assert_eq!(data.seq, 7);
    assert_eq!(data.uid, Some(42));
    assert_eq!(data.rfc822_size, Some(1234));
    assert_eq!(
        data.internal_date.unwrap().to_wire(),
        "17-Jul-1996 02:44:25 -0700"
    );
    assert!(data.flags.unwrap().to_parenthesised().contains("$Junk"));
}

#[test]
fn parses_envelope() {
    let tokens = list_from(
            "ENVELOPE (\"Mon, 1 Jan 2024 00:00:00 +0000\" \"Hello\" ((\"Alice\" NIL \"alice\" \"example.org\")) NIL NIL ((\"Bob\" NIL \"bob\" \"example.org\")) NIL NIL NIL \"<id@example.org>\")",
        );
    let data = parse_fetch_items(1, &tokens).unwrap();
    let envelope = data.envelope.unwrap();
    assert_eq!(envelope.subject.as_deref(), Some("Hello"));
    assert_eq!(
        envelope.from[0].to_mailbox_string(),
        "Alice <alice@example.org>"
    );
    assert_eq!(envelope.message_id.as_deref(), Some("<id@example.org>"));
}

#[test]
fn parses_body_structure() {
    let tokens = list_from(
        "BODYSTRUCTURE (\"TEXT\" \"PLAIN\" (\"CHARSET\" \"UTF-8\") NIL NIL \"7BIT\" 12 2)",
    );
    let data = parse_fetch_items(1, &tokens).unwrap();
    let body = data.body_structure.unwrap();
    assert_eq!(body.media_type, "TEXT");
    assert_eq!(body.lines, Some(2));
    assert_eq!(body.params[0].1, "UTF-8");
}

#[test]
fn parses_multipart_body_structure() {
    let tokens = list_from(
            "BODYSTRUCTURE ((\"TEXT\" \"PLAIN\" NIL NIL NIL \"7BIT\" 12 2)(\"TEXT\" \"HTML\" NIL NIL NIL \"7BIT\" 34 4) \"ALTERNATIVE\")",
        );
    let body = parse_fetch_items(1, &tokens)
        .unwrap()
        .body_structure
        .unwrap();
    assert!(body.is_multipart());
    assert_eq!(body.children.len(), 2);
    assert_eq!(body.media_subtype, "ALTERNATIVE");
}

#[test]
fn parses_body_section_and_partial() {
    let tokens = list_from("BODY[HEADER.FIELDS (FROM TO)]<0.5> {5}\r\nhello");
    let data = parse_fetch_items(1, &tokens).unwrap();
    assert_eq!(data.sections.len(), 1);
    assert_eq!(data.sections[0].spec, "HEADER.FIELDS (FROM TO)");
    assert_eq!(data.sections[0].partial, Some((0, 5)));
    assert_eq!(data.sections[0].data, b"hello");
}

#[test]
fn rejects_bad_shapes() {
    let tokens = list_from("UID");
    assert!(parse_fetch_items(1, &tokens).is_err());
    let tokens = list_from("ENVELOPE (NIL)");
    assert!(parse_fetch_items(1, &tokens).is_err());
}
