//! End-to-end test against an in-process mock IMAP server.

use std::time::Duration;

use fuckthunder_core::{FlagSet, MailboxName, SequenceSet, StatusItem};
use fuckthunder_imap::{ClientConfig, ImapClient, State};
use fuckthunder_imap_proto::{FetchItems, StoreAction};
use fuckthunder_sasl::{CramMd5, Credentials};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

const CAPS: &str =
    "IMAP4rev1 UIDPLUS MOVE IDLE ENABLE CONDSTORE ID AUTH=PLAIN AUTH=CRAM-MD5 AUTH=XOAUTH2";

/// Reads one complete command line, handling literals with continuations.
async fn read_command(io: &mut BufReader<TcpStream>) -> (String, String) {
    let mut command = Vec::new();
    loop {
        let mut line = Vec::new();
        io.read_until(b'\n', &mut line).await.unwrap();
        assert!(!line.is_empty(), "client closed the connection");
        command.extend_from_slice(&line);
        let trimmed = line.strip_suffix(b"\r\n").unwrap_or(&line);
        let marker = fuckthunder_imap_proto::fast::literal_length(trimmed);
        match marker {
            Some(length) => {
                io.write_all(b"+ go ahead\r\n").await.unwrap();
                let mut data = vec![0u8; length];
                io.read_exact(&mut data).await.unwrap();
                command.extend_from_slice(&data);
            }
            None => break,
        }
    }
    let text = String::from_utf8_lossy(&command).into_owned();
    let tag = text.split_whitespace().next().unwrap_or("a0").to_owned();
    (tag, text)
}

async fn reply(io: &mut BufReader<TcpStream>, tag: &str, untagged: &[&str], text: &str) {
    for line in untagged {
        io.write_all(line.as_bytes()).await.unwrap();
    }
    io.write_all(format!("{tag} {text}\r\n").as_bytes())
        .await
        .unwrap();
}

async fn serve(socket: TcpStream) {
    let mut io = BufReader::new(socket);

    io.write_all(format!("* OK [CAPABILITY {CAPS}] mock ready\r\n").as_bytes())
        .await
        .unwrap();

    loop {
        let mut line = Vec::new();
        io.read_until(b'\n', &mut line).await.unwrap();
        if line.is_empty() {
            return; // connection closed
        }
        let trimmed = line.strip_suffix(b"\r\n").unwrap_or(&line).to_vec();
        let text = String::from_utf8_lossy(&trimmed).into_owned();
        let mut words = text.splitn(2, ' ');
        let tag = words.next().unwrap_or("a0").to_owned();
        let rest = words.next().unwrap_or("").to_ascii_uppercase();
        let command = rest.split_whitespace().next().unwrap_or("").to_owned();

        match command.as_str() {
            "CAPABILITY" => {
                reply(
                    &mut io,
                    &tag,
                    &[&format!("* CAPABILITY {CAPS}\r\n")],
                    "OK CAPABILITY completed",
                )
                .await;
            }
            "LOGIN" => {
                reply(&mut io, &tag, &[], "OK login completed").await;
            }
            "AUTHENTICATE" => {
                // First AUTHENTICATE line carries the mechanism; respond with a
                // CRAM-MD5 challenge when asked, otherwise accept the initial
                // response immediately.
                if text.to_ascii_uppercase().contains("CRAM-MD5") {
                    io.write_all(b"+ PE1hcmluZyBDb25uZWN0aW9ucw==\r\n")
                        .await
                        .unwrap();
                    let mut response = Vec::new();
                    io.read_until(b'\n', &mut response).await.unwrap();
                    reply(&mut io, &tag, &[], "OK authenticated").await;
                } else {
                    reply(&mut io, &tag, &[], "OK authenticated").await;
                }
            }
            "SELECT" | "EXAMINE" => {
                reply(
                    &mut io,
                    &tag,
                    &[
                        "* 3 EXISTS\r\n",
                        "* 1 RECENT\r\n",
                        "* FLAGS (\\Answered \\Flagged \\Deleted \\Seen \\Draft)\r\n",
                        "* OK [UNSEEN 1] first unseen\r\n",
                        "* OK [UIDVALIDITY 7] UIDs valid\r\n",
                        "* OK [UIDNEXT 8] next\r\n",
                    ],
                    "OK [READ-WRITE] SELECT completed",
                )
                .await;
            }
            "LIST" => {
                reply(
                    &mut io,
                    &tag,
                    &["* LIST (\\HasNoChildren) \"/\" INBOX\r\n"],
                    "OK LIST completed",
                )
                .await;
            }
            "LSUB" => {
                reply(
                    &mut io,
                    &tag,
                    &["* LSUB () \"/\" INBOX\r\n"],
                    "OK LSUB completed",
                )
                .await;
            }
            "ID" => {
                reply(
                    &mut io,
                    &tag,
                    &["* ID (\"name\" \"mock\" \"version\" \"1.0\")\r\n"],
                    "OK ID completed",
                )
                .await;
            }
            "CHECK" => {
                reply(&mut io, &tag, &[], "OK CHECK completed").await;
            }
            "UNSELECT" => {
                reply(&mut io, &tag, &[], "OK UNSELECT completed").await;
            }
            "CLOSE" => {
                reply(&mut io, &tag, &[], "OK CLOSE completed").await;
            }
            c if c.starts_with("STATUS") => {
                reply(
                    &mut io,
                    &tag,
                    &["* STATUS INBOX (MESSAGES 3 UIDNEXT 8 UIDVALIDITY 7 UNSEEN 1)\r\n"],
                    "OK STATUS completed",
                )
                .await;
            }
            c if c == "FETCH" || rest.starts_with("UID FETCH") => {
                reply(
                    &mut io,
                    &tag,
                    &["* 1 FETCH (UID 41 FLAGS (\\Seen) RFC822.SIZE 123 BODY[] {5}\r\nhello)\r\n"],
                    "OK FETCH completed",
                )
                .await;
            }
            c if c.starts_with("APPEND") => {
                // The command announced a literal; acknowledge it.
                io.write_all(b"+ go ahead\r\n").await.unwrap();
                let _rest = read_command(&mut io).await;
                reply(&mut io, &tag, &[], "OK [APPENDUID 7 42] appended").await;
            }
            c if c == "STORE" || rest.starts_with("UID STORE") => {
                reply(
                    &mut io,
                    &tag,
                    &["* 1 FETCH (FLAGS (\\Seen \\Answered))\r\n"],
                    "OK STORE completed",
                )
                .await;
            }
            c if c == "SEARCH" || rest.starts_with("UID SEARCH") => {
                reply(
                    &mut io,
                    &tag,
                    &["* SEARCH 1 2 3\r\n"],
                    "OK SEARCH completed",
                )
                .await;
            }
            c if c == "COPY"
                || c == "MOVE"
                || rest.starts_with("UID COPY")
                || rest.starts_with("UID MOVE") =>
            {
                reply(&mut io, &tag, &[], "OK [COPYUID 7 1:* 42] done").await;
            }
            "EXPUNGE" => {
                reply(&mut io, &tag, &["* 2 EXPUNGE\r\n"], "OK expunge completed").await;
            }
            _ if rest.starts_with("UID EXPUNGE") => {
                reply(&mut io, &tag, &["* 2 EXPUNGE\r\n"], "OK expunge completed").await;
            }
            "ENABLE" => {
                reply(&mut io, &tag, &["* ENABLED CONDSTORE\r\n"], "OK enabled").await;
            }
            "NAMESPACE" => {
                reply(
                    &mut io,
                    &tag,
                    &["* NAMESPACE ((\"\" \"/\")) NIL NIL\r\n"],
                    "OK namespace completed",
                )
                .await;
            }
            "NOOP" => {
                reply(&mut io, &tag, &[], "OK noop completed").await;
            }
            "LOGOUT" => {
                reply(&mut io, &tag, &["* BYE bye\r\n"], "OK logout completed").await;
                return;
            }
            "IDLE" => {
                io.write_all(b"+ idling\r\n").await.unwrap();
                io.write_all(b"* 4 EXISTS\r\n").await.unwrap();
                io.write_all(b"* 2 EXPUNGE\r\n").await.unwrap();
                let mut done = Vec::new();
                io.read_until(b'\n', &mut done).await.unwrap();
                reply(&mut io, &tag, &[], "OK IDLE terminated").await;
            }
            other => panic!("unexpected command from client: {other:?}"),
        }
    }
}

#[tokio::test]
async fn full_session_against_mock_server() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        serve(socket).await;
    });

    let config = ClientConfig::plaintext("127.0.0.1", port).with_timeout(Duration::from_secs(5));
    let mut client = ImapClient::connect(config).await.unwrap();
    assert_eq!(client.state(), &State::NotAuthenticated);
    assert_eq!(client.greeting(), "mock ready");
    assert!(client.capabilities().has("UIDPLUS"));
    assert!(client.capabilities().idle());

    client.capability().await.unwrap();
    client
        .authenticate_oauth2("alice", "token-123")
        .await
        .unwrap();
    client.login("alice", "hunter2").await.unwrap();
    assert_eq!(client.state(), &State::Authenticated);

    let inbox = MailboxName::from_utf8("INBOX").unwrap();
    let info = client.select(&inbox, false).await.unwrap();
    assert_eq!(info.exists, 3);
    assert_eq!(info.recent, 1);
    assert_eq!(info.unseen, Some(1));
    assert_eq!(info.uid_validity, Some(7));
    assert_eq!(info.uid_next, Some(8));
    assert!(!info.read_only);
    assert!(info.flags.contains(&fuckthunder_core::Flag::Seen));

    let list = client.list("*", "*").await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name.as_str(), "INBOX");
    assert!(list[0].is_selectable());

    let status = client
        .status(&inbox, &[StatusItem::Messages, StatusItem::Unseen])
        .await
        .unwrap();
    assert_eq!(status.messages, Some(3));
    assert_eq!(status.unseen, Some(1));

    let fetched = client
        .fetch(&SequenceSet::all(), &FetchItems::summary())
        .await
        .unwrap();
    assert_eq!(fetched.len(), 1);
    assert_eq!(fetched[0].uid, Some(41));
    assert_eq!(fetched[0].rfc822_size, Some(123));
    assert_eq!(fetched[0].sections[0].data, b"hello");

    let appended = client
        .append(
            &inbox,
            Some(&FlagSet::parse(["\\Seen"])),
            None,
            b"hello".to_vec(),
        )
        .await
        .unwrap();
    assert_eq!(appended, Some(42));

    let stored = client
        .store(
            &SequenceSet::all(),
            StoreAction::Add,
            &FlagSet::parse(["\\Answered"]),
            false,
        )
        .await
        .unwrap();
    assert_eq!(stored.len(), 1);
    assert!(stored[0]
        .flags
        .as_ref()
        .unwrap()
        .contains(&fuckthunder_core::Flag::Answered));

    let found = client
        .search(&fuckthunder_imap_proto::SearchQuery::all())
        .await
        .unwrap();
    assert_eq!(found, vec![1, 2, 3]);

    let archive = MailboxName::from_utf8("Archive").unwrap();
    client.copy(&SequenceSet::all(), &archive).await.unwrap();
    client.move_to(&SequenceSet::all(), &archive).await.unwrap();

    let expunged = client.expunge().await.unwrap();
    assert_eq!(expunged, vec![2]);
    let uid_expunged = client.uid_expunge(&SequenceSet::all()).await.unwrap();
    assert_eq!(uid_expunged, vec![2]);

    let enabled = client.enable(&["CONDSTORE"]).await.unwrap();
    assert_eq!(enabled, vec!["CONDSTORE"]);

    let namespaces = client.namespace().await.unwrap();
    assert_eq!(namespaces.personal.len(), 1);
    assert_eq!(namespaces.personal[0].delimiter, Some('/'));

    client.noop().await.unwrap();

    let subscribed = client.lsub("", "*").await.unwrap();
    assert_eq!(subscribed.len(), 1);

    let uid_fetched = client
        .uid_fetch(&SequenceSet::all(), &FetchItems::uid())
        .await
        .unwrap();
    assert_eq!(uid_fetched.len(), 1);
    let uid_stored = client
        .uid_store(
            &SequenceSet::all(),
            StoreAction::Add,
            &FlagSet::parse(["\\Seen"]),
            true,
        )
        .await
        .unwrap();
    assert_eq!(uid_stored.len(), 1);
    let uid_found = client
        .uid_search(&fuckthunder_imap_proto::SearchQuery::seen())
        .await
        .unwrap();
    assert_eq!(uid_found, vec![1, 2, 3]);
    client
        .uid_copy(&SequenceSet::all(), &archive)
        .await
        .unwrap();
    client
        .uid_move(&SequenceSet::all(), &archive)
        .await
        .unwrap();

    let events = client.idle_for(Duration::from_millis(100)).await.unwrap();
    assert!(events.iter().any(|event| matches!(
        event,
        fuckthunder_imap_proto::Response::Untagged(fuckthunder_imap_proto::Untagged::Exists(4))
    )));

    client.logout().await.unwrap();
    assert_eq!(client.state(), &State::Logout);
    server.await.unwrap();
}

#[tokio::test]
async fn sasl_cram_md5_over_mock_server() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut io = BufReader::new(socket);
        io.write_all(format!("* OK [CAPABILITY {CAPS}] mock ready\r\n").as_bytes())
            .await
            .unwrap();
        let (tag, _text) = read_command(&mut io).await; // CAPABILITY
        reply(&mut io, &tag, &[&format!("* CAPABILITY {CAPS}\r\n")], "OK").await;
        let (tag, text) = read_command(&mut io).await; // AUTHENTICATE CRAM-MD5
        assert!(text.contains("CRAM-MD5"));
        io.write_all(b"+ PE1hcmluZyBDb25uZWN0aW9ucw==\r\n")
            .await
            .unwrap();
        let mut response = Vec::new();
        io.read_until(b'\n', &mut response).await.unwrap();
        assert!(!response.starts_with(b"*")); // base64 response, not a command
        reply(&mut io, &tag, &[], "OK authenticated").await;
        loop {
            let (tag, text) = read_command(&mut io).await;
            if text.contains("ID") {
                reply(&mut io, &tag, &["* ID NIL\r\n"], "OK").await;
                continue;
            }
            assert!(text.contains("LOGOUT"));
            reply(&mut io, &tag, &["* BYE bye\r\n"], "OK logout completed").await;
            break;
        }
    });

    let config = ClientConfig::plaintext("127.0.0.1", port).with_timeout(Duration::from_secs(5));
    let mut client = ImapClient::connect(config).await.unwrap();
    let mut mechanism = CramMd5::new(&Credentials::password("alice", "hunter2"));
    client.authenticate(&mut mechanism).await.unwrap();
    assert_eq!(client.state(), &State::Authenticated);
    client.logout().await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn capability_guard_rejects_unadvertised_commands() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut io = BufReader::new(socket);
        io.write_all(b"* OK [CAPABILITY IMAP4rev1] mock ready\r\n")
            .await
            .unwrap();
        let (tag, _text) = read_command(&mut io).await;
        reply(&mut io, &tag, &["* CAPABILITY IMAP4rev1\r\n"], "OK").await;
        let (tag, text) = read_command(&mut io).await; // LOGOUT
        assert!(text.contains("LOGOUT"));
        reply(&mut io, &tag, &["* BYE bye\r\n"], "OK").await;
    });

    let config = ClientConfig::plaintext("127.0.0.1", port).with_timeout(Duration::from_secs(5));
    let mut client = ImapClient::connect(config).await.unwrap();
    let error = client.compress().await.unwrap_err();
    assert!(matches!(error, fuckthunder_core::Error::Unsupported(_)));
    let inbox = MailboxName::from_utf8("INBOX").unwrap();
    let error = client
        .move_to(&SequenceSet::all(), &inbox)
        .await
        .unwrap_err();
    assert!(matches!(error, fuckthunder_core::Error::Unsupported(_)));
    client.logout().await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn connects_through_socks5_proxy() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut io = BufReader::new(socket);

        // SOCKS5 greeting: version 5, one method, no-auth.
        let mut greeting = [0u8; 3];
        io.read_exact(&mut greeting).await.unwrap();
        assert_eq!(greeting, [0x05, 0x01, 0x00]);
        io.write_all(&[0x05, 0x00]).await.unwrap();

        // CONNECT request with a domain name.
        let mut head = [0u8; 5];
        io.read_exact(&mut head).await.unwrap();
        assert_eq!(&head[..4], [0x05, 0x01, 0x00, 0x03]);
        let mut host = vec![0u8; head[4] as usize + 2];
        io.read_exact(&mut host).await.unwrap();
        assert_eq!(&host[..host.len() - 2], b"imap.example.org");
        assert_eq!(&host[host.len() - 2..], &993u16.to_be_bytes());
        io.write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
            .await
            .unwrap();

        // From here on the tunnel carries plain IMAP.
        io.write_all(b"* OK [CAPABILITY IMAP4rev1] via proxy\r\n")
            .await
            .unwrap();
        let (tag, _text) = read_command(&mut io).await;
        reply(&mut io, &tag, &["* CAPABILITY IMAP4rev1\r\n"], "OK").await;
        let (tag, _text) = read_command(&mut io).await; // LOGOUT
        reply(&mut io, &tag, &["* BYE bye\r\n"], "OK").await;
    });

    let config = ClientConfig::tls("imap.example.org")
        .with_socks5("127.0.0.1", port)
        .with_timeout(Duration::from_secs(5));
    let mut config = config;
    config.tls = fuckthunder_imap::TlsMode::Plain;

    let mut client = ImapClient::connect(config).await.unwrap();
    assert_eq!(client.greeting(), "via proxy");
    client.logout().await.unwrap();
    server.await.unwrap();
}
