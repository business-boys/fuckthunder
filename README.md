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

## Hakus mail client and account checker

```bash
# Full GUI; no file or other CLI argument is required.
cargo run -p fuckthunder-gui --bin fuckthunder-gui
# Optional startup import and custom database:
cargo run -p fuckthunder-gui --bin fuckthunder-gui -- accounts.txt \
  --database ./workspace.db --concurrency 8
# Headless batch-only driver:
cargo run -p fuckthunder-gui --bin fuckthunder-check -- accounts.txt --concurrency 10
```

### Install Hakus on macOS

Build and install the native app for your Mac's architecture:

```bash
tools/macos/package.sh --install
open "$HOME/Applications/Hakus.app"
```

The script builds a release binary, creates `Hakus.app` with an icon, applies an
ad-hoc signature, and installs it in `~/Applications`. Launch it from Finder or
Spotlight, or choose **Options → Keep in Dock** from its Dock menu. Quit Hakus
before reinstalling. The installer keeps an existing app as a timestamped backup.

Without `--install`, the script writes the bundle and a ZIP to `target/macos/`.
This is a local ad-hoc build, not an Apple-notarized public release. The bundle
requires macOS 12 or later. Building requires Rust, Xcode with the Metal toolchain,
Python 3, and the Xcode command-line tools.

The app keeps existing workspace data in
`~/Library/Application Support/FuckThunder/workspace.db`. Installing or replacing
the app does not remove accounts or settings. Passwords remain in local SQLite,
not macOS Keychain.

The desktop app opens in compact **Mail client** mode with separate **Email**
and **Password** fields. Enter them separately and press **Enter**, or paste an
`email:password` row into either field with **Cmd+V** on macOS or **Ctrl+V**
elsewhere. A combined row splits on the first colon and connects immediately;
passwords may contain additional colons. The login screen has no sidebar. After
connecting, a narrow folder panel appears and INBOX opens automatically.
**Advanced** exposes a server override, port, TLS/STARTTLS and an optional
SOCKS5/HTTP CONNECT proxy. Successful connections are saved in the local SQLite
workspace, including the password. **+ Add mailbox** opens an independent tab.
Keep several mailboxes open and switch tabs without resetting their folder,
page, or open message. The tab strip scrolls and includes an overflow menu.
Close a tab with **×**. Closing the last tab opens a fresh connection form.
Saved mailboxes appear below the tab strip and reopen without duplicate tabs.
**Refresh** reloads the current folder, **Disconnect** returns to the connection
form, and **Cancel** stops an active request in that tab.

At the default window size, folders, messages and the reader stay in separate
columns. **+ Add mailbox** stays at the top beside the tabs. The compact toolbar
contains search, pagination, Refresh and Disconnect. **Cmd+Q** quits on macOS
and **Ctrl+Q** quits elsewhere, cancelling active requests.

The reader uses one authenticated read-only IMAP session per tab, rejects stale
request results. Drag the dividers to resize the folder, message and reader
panels. **Search this folder** searches all matching messages on the server.
**Retry** repeats a failed request, and **Copy message** copies the visible text.
It does not send mail, change flags, or provide OAuth login. Providers that
disable password login require a supported app password or another client.

Select **Account checker** for file imports, batch checks, history, proxies and
workspace settings. Client mode does not run a batch before opening a mailbox.

The checker imports UTF-8 `email:password` files through a native file picker
(CRLF accepted, passwords may contain colons, files up to 16 MiB). Paste account
rows with **Ctrl+V** on Linux/Windows or **Cmd+V** on macOS. In **Proxy routes**,
the shortcut imports proxies. In client text fields, it pastes text normally.
The proxy importer accepts `socks5://`/`http://` URLs and
`host:port:user:password` rows. Accounts are checked with bounded, adjustable
concurrency and round-robin proxy assignment; batches can be stopped, failed
accounts retried and verdicts reset.
The account workspace includes keyboard search, status/route filters, 25-row
pages, quick `4 / 8 / 16 / 32 / 64` thread presets and live determinate progress.
Its sidebar navigation and subtle GPUI animations do not require a background
redraw timer; large account, message, proxy and history lists stay virtualized.
The interface uses the bundled Onest variable font (SIL Open Font License; see
`crates/fuckthunder-gui/assets/OFL-Onest.txt`).

The local SQLite workspace stores accounts, proxy settings, per-account verdicts
and timestamps, plus the latest 100 aggregated runs shown in **History**. The
default path is `~/Library/Application Support/FuckThunder/workspace.db` on
macOS and `~/.local/share/fuckthunder/workspace.db` elsewhere. Credentials are
stored locally in plaintext for subsequent IMAP sessions: protect the database
like a password file. On Unix the GUI restricts it to mode `0600`; proxy
credentials are never rendered in the UI.

**Settings & API** persists thread count, IMAP timeout and API preferences. The
embedded server binds only to `127.0.0.1` (default port `8765`), can be started
at application launch or from the GUI, and provides URL/token copy controls.
`GET /health` is public; all other routes require `Authorization: Bearer <token>`.
Available routes are `GET /api/v1/stats`, account/proxy list, import, delete and
clear operations, `GET /api/v1/runs`, `POST /api/v1/checks`,
`POST /api/v1/results/reset`, and `DELETE /api/v1/history`. Imports accept raw
UTF-8 text, a JSON string or `{ "text": "..." }`. Responses omit account
passwords and proxy credentials. Destructive operations return HTTP 409 while a
check is active. The same screen provides guarded account, proxy, history and
verdict cleanup controls.

Folders, 50-message pages and lazy MIME/plain-text rendering remain read-only.
Common provider hosts are inferred automatically. Unknown domains default to
`imap.<domain>` and can use an explicit host in the client form.
TLS certificates remain verified, including for explicit servers and proxies.
All mailbox selection uses `EXAMINE`, and full messages use
`BODY.PEEK[]`: there are no delete, flag-update, send or IMAP-enable operations.
The message viewer uses a native WebView for HTML/CSS, selectable wrapped text
and message source. Its header stays outside the body. Text, HTML, source, full headers, `.eml`
export, attachment save/open, and detached viewing share one viewer component.
Mailbox panels have draggable dividers. CID images use decoded MIME resources.
External images stay blocked until explicitly enabled. Scripts and forms remain
blocked. Links open in the system browser. Open a local message with
`fuckthunder-gui --message message.eml`.

Opening an attachment writes a temporary copy for its default application.
The account checker searches subject, sender and body across folders.

## Development

```bash
cargo test --workspace                                  # 162 tests
cargo clippy --workspace --all-targets -- -D warnings   # 0 warnings
python3 tools/parity.py --seeds 20                      # A/B differential parity
./target/release/fh-bench --corpus fixtures/bench_corpus.imap --mode fast
```

Architecture blueprint and legacy bug analysis: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).
Migration report: `migration_infographic.html` (open in a browser).
