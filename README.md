# fuckthunder

Thunderbird's IMAP client, SASL mechanisms and OAuth 2.0 flows, migrated to
**100% safe Rust** (`#![forbid(unsafe_code)]` in every crate). No XPCOM, no
manual memory management — memory safety is enforced by the compiler.

| | Legacy (Thunderbird C++) | fuckthunder (Rust) |
| --- | --- | --- |
| Code | 29,435 LOC (`nsImapProtocol.cpp` alone: 10,262) | 7,460 LOC, 7 crates |
| Unsafe | raw `PR_Malloc`/`strcpy`, static buffers | 0 unsafe blocks |
| Tests | — | 162 (unit + property + integration) |
| Clippy | — | 0 warnings (`-D warnings`) |
| Parse speed | 23.66 ms/225 KB (reference model) | 0.059 ms (PGO+LTO, **394×**) |

## Crates

| Crate | What it does |
| --- | --- |
| `fuckthunder-core` | Errors, capabilities, modified UTF-7 mailbox names, flags, sequence sets, STATUS, response codes |
| `fuckthunder-imap-proto` | Zero-copy tokenizer, typed response parser, command encoder, SEARCH builder, FETCH/BODYSTRUCTURE, allocation-free scanner |
| `fuckthunder-sasl` | PLAIN, LOGIN, CRAM-MD5, XOAUTH2, OAUTHBEARER, ANONYMOUS, EXTERNAL |
| `fuckthunder-oauth` | PKCE, authorization-code/refresh/device flows, loopback redirect listener, token store |
| `fuckthunder-imap` | Transport (TLS / STARTTLS / DEFLATE / **SOCKS5 / HTTP CONNECT**), connection framing, high-level `ImapClient` |
| `fuckthunder-cli` | `fuckthunder` command-line client |
| `fuckthunder-gui` | GPUI batch account checker and read-only, paginated mail viewer |
| `fuckthunder-tools` | `fh-parity` (A/B differential harness) + `fh-bench` |

## Usage

Add to `Cargo.toml` (from GitHub):

```toml
[dependencies]
fuckthunder-imap = { git = "https://github.com/business-boys/fuckthunder" }
fuckthunder-core = { git = "https://github.com/business-boys/fuckthunder" }
tokio = { version = "1", features = ["full"] }
```

### Connect (TLS, default Thunderbird identity)

```rust
use fuckthunder_imap::{ClientConfig, ImapClient};
use fuckthunder_core::MailboxName;

let config = ClientConfig::tls("imap.example.org");          // :993, implicit TLS
// ClientConfig::starttls("imap.example.org")                // :143 + STARTTLS
let mut client = ImapClient::connect(config).await?;
client.login("alice@example.org", "app-password").await?;
```

After `login`/`authenticate` the client automatically sends
`ID ("name" "Thunderbird" "version" "128.5.0esr")` when the server advertises
`ID` — the wire profile (numeric tags, ID fields) matches Thunderbird.
Override with `.with_identity([("name", "MyApp")])` or disable with
`.without_identity()`.

### Through a SOCKS5 proxy

DNS is resolved **on the proxy side** (`ATYP 0x03`, i.e. `socks5h` semantics);
TLS is terminated at the real host inside the tunnel — the proxy never sees
plaintext or credentials.

```rust
let config = ClientConfig::tls("imap.example.org")
    .with_socks5("127.0.0.1", 1080);                                  // no auth
let config = ClientConfig::tls("imap.example.org")
    .with_socks5_auth("proxy.host", 1080, "user", "password");        // RFC 1929
```

### Through an HTTP CONNECT proxy

The proxy establishes a TCP tunnel to the IMAP endpoint; implicit TLS still
terminates at that endpoint. Optional Basic authentication is sent only to the
proxy during `CONNECT`.

```rust
let config = ClientConfig::tls("imap.example.org")
    .with_http_connect("127.0.0.1", 8080);                             // no auth
let config = ClientConfig::tls("imap.example.org")
    .with_http_connect_auth("proxy.host", 8080, "user", "password");  // Basic auth
```

### Reading mail

```rust
use fuckthunder_core::{MailboxName, SequenceSet};
use fuckthunder_imap_proto::FetchItems;

let inbox = MailboxName::from_utf8("INBOX")?;
let info = client.select(&inbox, false).await?;
println!("{} messages, {:?} unseen", info.exists, info.unseen);

let set: SequenceSet = "1:10".parse()?;
for msg in client.fetch(&set, &FetchItems::summary()).await? {
    let subject = msg.envelope.as_ref().and_then(|e| e.subject.clone());
    println!("#{} uid={:?} {:?}", msg.seq, msg.uid, subject);
}
```

### Waiting for a message: `idle_until`

Blocks inside `IDLE` and returns the moment your predicate matches
(or `Err(Error::Timeout)` after `max_wait`):

```rust
use std::time::Duration;
use fuckthunder_imap_proto::{Response, Untagged};

// Wait until a new message arrives in the selected mailbox.
let new_count = client
    .idle_until(Duration::from_secs(300), |event| match event {
        Response::Untagged(Untagged::Exists(n)) => Some(*n),
        _ => None,
    })
    .await?;
println!("mailbox now holds {new_count} messages");
```

Loop it for a watch daemon:

```rust
loop {
    match client.idle_until(Duration::from_secs(1800), |e| match e {
        Response::Untagged(Untagged::Exists(_)) => Some(()),
        _ => None,
    }).await {
        Ok(()) => { /* fetch the new message */ }
        Err(fuckthunder_core::Error::Timeout) => { /* re-issue / heartbeat */ }
        Err(e) => return Err(e),
    }
}
```

### OAuth 2.0 (Gmail / Microsoft / Yahoo)

```rust
use fuckthunder_oauth::{OAuthClient, OAuthConfig, Provider};

let config = OAuthConfig::new(Provider::Google, "your-client-id");
let oauth = OAuthClient::new(config)?;
let request = oauth.authorization_request("http://localhost:8080/callback")?;
// open request.url, catch the code on the loopback listener, then:
let tokens = oauth.exchange_code(&code, redirect_uri, request.pkce.as_ref()).await?;

client.authenticate_oauth2("alice@gmail.com", &tokens.access_token).await?;
```

## CLI

```bash
cargo install --git https://github.com/business-boys/fuckthunder fuckthunder-cli

fuckthunder --host secureimap.t-online.de \
  --proxy socks5://user:pass@proxy:1080 \
  --user alice@example.org --password '...' \
  list            # capability | login | list | select | fetch 1:10 | search UNSEEN | idle

fuckthunder --host secureimap.t-online.de \
  --proxy http://user:pass@proxy:8080 \
  --user alice@example.org --password '...' \
  list            # capability | login | list | select | fetch 1:10 | search UNSEEN | idle
```

Env vars: `FUCKTHUNDER_HOST`, `FUCKTHUNDER_PORT`, `FUCKTHUNDER_USER`,
`FUCKTHUNDER_PASSWORD`, `FUCKTHUNDER_PROXY`.

## GPUI checker and mail viewer

```bash
cargo run -p fuckthunder-gui -- accounts.txt --concurrency 8
# Optional per-process SOCKS5H or HTTP CONNECT proxy:
cargo run -p fuckthunder-gui -- accounts.txt --proxy socks5://user:pass@proxy:1080
```

The input is UTF-8 `email:password` text (CRLF accepted, passwords may contain
colons), limited to 8 MiB. Credentials stay in process memory and are redacted
from `Debug`; the application does not create an account/result database.

The first vertical slice provides bounded account checks, selectable folders,
50-message pages and lazy MIME/plain-text message rendering. GMX and WEB.DE IMAP
hosts are inferred automatically; other domains use `imap.<domain>`. All mailbox
selection uses `EXAMINE`, and full messages use `BODY.PEEK[]`: the GUI has no
delete, flag-update, send or IMAP-enable operations. HTML is deliberately not
executed or rendered. Account lists use GPUI virtualized rendering.

## Development

```bash
cargo test --workspace                                  # 162 tests
cargo clippy --workspace --all-targets -- -D warnings   # 0 warnings
python3 tools/parity.py --seeds 20                      # A/B differential parity
./target/release/fh-bench --corpus fixtures/bench_corpus.imap --mode fast
```

Architecture blueprint and legacy bug analysis: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).
Migration report: `migration_infographic.html` (open in a browser).
