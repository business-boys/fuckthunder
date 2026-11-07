# fuckthunder — Architectural Blueprint

Migration of Thunderbird's IMAP client, OAuth 2.0 flow and SASL stack into a
`#![forbid(unsafe_code)]` Rust workspace. Legacy source:
`legacy/thunderbird/mailnews/imap`, `mailnews/base`, `mailnews/intl` (read-only
reference checkout).

## 1. Codebase Audit, Legacy Census & Latent Bugs Discovered

Automated census of the scanned legacy directories:

| Metric | Legacy value |
| --- | ---: |
| Files | 39 |
| Code LOC | 29,435 |
| Comment density | 22.3% |
| Largest file | 9,309 code LOC (`nsImapProtocol.cpp`, 10,262 physical lines) |
| Average file | 923 LOC |
| Files ≤ 400 LOC | 59.0% |
| Maximum nesting depth | 11 |
| Average function length | 13 LOC |
| Raw memory / state hazards | 3,701 |

The audit found the classic XPCOM profile: one 10k-line protocol object, manual
`PR_Malloc`/`PR_Free`, mutable static buffers, and a parser that mutates a
shared C string in place.

### Latent bugs and undefined behaviour

1. **Overlapping `strcpy` is undefined behaviour.**
   `mailnews/imap/src/nsImapServerResponseParser.cpp:329` strips escapes with
   `PL_strcpy(currentChar, currentChar + 1)`. The source and destination
   overlap; ISO C leaves this undefined, and only the accidental forward-copy
   implementation makes it work.
2. **Raw allocation without space for the terminator.**
   `mailnews/imap/src/nsImapProtocol.cpp:1716-1721` allocates
   `PR_Malloc(PL_strlen(serverResponse))`, then writes a prefix plus suffix with
   `strcpy`/`strcat` and an explicit `'\0'`. It happens to fit only because the
   replacement prefix is shorter than the removed one; any prefix-length change
   becomes a heap overflow.
3. **Lenient modified-UTF-7 decoding accepts unpaired surrogates.**
   `mailnews/intl/nsUTF7ToUnicode.cpp` accumulates UTF-16 code units without a
   surrogate-pair check. Malformed mailbox names therefore reach the UI as
   invalid UTF-16 instead of being rejected.
4. **Global mutable buffers.**
   `mailnews/imap/src/nsImapProtocol.cpp:361-362` keeps `static char
   gAppName[kAppBufSize]` and `gAppVersion[kAppBufSize]`; the protocol object is
   not re-entrant and test isolation depends on process-global state.
5. **Modified UTF-7 collision hazard.**
   The encoder must keep `+` in the BASE64 alphabet and replace only `/` with
   `,`. Replacing `+` with `-` (an easy mistake; it was made and caught in this
   port) turns `U+F800` into `&-AA-`, which is indistinguishable from the
   literal mailbox name `&AA-`. `nsUnicodeToUTF7.cpp` confirms the correct
   alphabet.

## 2. Target Safe Rust Architecture & Module Graph

```
fuckthunder-core        typed errors, capabilities, mailbox names (modified
                        UTF-7), flags, sequence sets, STATUS, response codes,
                        dates, envelopes
fuckthunder-imap-proto  zero-copy tokenizer, typed response parser, command
                        encoder, SEARCH builder, FETCH/BODYSTRUCTURE,
                        allocation-free fast scanner
fuckthunder-sasl        PLAIN, LOGIN, CRAM-MD5, XOAUTH2, OAUTHBEARER,
                        ANONYMOUS, EXTERNAL
fuckthunder-oauth       providers, PKCE, token store, auth-code/refresh/device
                        flows, loopback redirect server
fuckthunder-imap        transport (plain/TLS/STARTTLS/DEFLATE), connection
                        framing and literals, high-level ImapClient
fuckthunder-cli         small command-line client
fuckthunder-tools       fh-parity fingerprint harness and fh-bench
```

Every crate root has `#![forbid(unsafe_code)]` and `#![deny(missing_docs)]`.
Files target ≤ 400 LOC; the large legacy units were split by responsibility.

| Legacy file & symbol | Target Rust module & type/fn | Transformation |
| --- | --- | --- |
| `nsImapProtocol.cpp::nsImapProtocol` (10,262 lines) | `fuckthunder-imap::{transport,connection,client}` | Split transport, framing, SASL and high-level commands; owned buffers; async I/O |
| `nsImapServerResponseParser.cpp` (2,690 lines) | `fuckthunder-imap-proto::{token,response}` | Zero-copy tokens → typed `Response`/`Untagged`; no in-place string mutation |
| `nsImapServerResponseParser::ParseIMAPServerResponse` | `response::parse_response` | Returns `Result<Response>` instead of mutating parser state |
| `nsMsgI18N.cpp::CopyUTF16toMUTF7/CopyMUTF7toUTF16` | `fuckthunder-core::mailbox` | Safe `String` round-trip; strict UTF-16 validation; `/`→`,` only |
| `nsImapCore.h` flags / sequence sets | `fuckthunder-core::{flags,seq}` | Strong `FlagSet`/`SequenceSet` types |
| `nsImapProtocol` SASL branches | `fuckthunder-sasl` | Trait `Mechanism`; no credential strings in logs |
| Thunderbird OAuth2 modules | `fuckthunder-oauth` | PKCE, token store, device flow, loopback listener |
| `nsImapService` URL/command plumbing | `fuckthunder-imap::client::ImapClient` | Async typed methods, capability guards |

## 3. API Ergonomics & Call-Site Modernization

Legacy response handling mutates parser state and exposes raw C strings:

```cpp
// nsImapServerResponseParser.cpp (abridged)
PL_strcpy(currentChar, currentChar + 1);   // overlapping copy
fSelectedMailboxName = PL_strdup(openQuote + 1);
```

Rust returns an owned, typed value and never mutates caller memory:

```rust
let response = parse_response(b"* 3 FETCH (UID 9 BODY[] {5}\r\nhello)\r\n")?;
if let Response::Untagged(Untagged::Fetch(data)) = response {
    assert_eq!(data.uid, Some(9));
    assert_eq!(data.sections[0].data, b"hello");
}
```

Legacy command construction concatenates into fixed buffers. Rust builds a
segment list, so literal framing and `LITERAL+` are structural:

```rust
let command = Command::append(&mailbox, None, None, body);
let encoded = command.encode("a3", false)?;
assert!(encoded.needs_continuation());
```

## 4. Test Harness, Property Fuzzing & Parity Strategy

- **Unit tests** per module, including RFC vectors: modified UTF-7 example
  (`~peter/mail/&U,BTFw-/&ZeVnLIqe-`), CRAM-MD5 RFC 2195, PKCE RFC 7636,
  SASL PLAIN RFC 4616, OAuth error shapes.
- **Doc tests** on every public entry point (`cargo test --doc`).
- **Property tests** (`proptest`): arbitrary mailbox names round-trip through
  modified UTF-7; sequence-set render/parse round-trip; tokenizer never panics
  on arbitrary bytes.
- **Integration test** with an in-process mock IMAP server covering greeting,
  `CAPABILITY`, `LOGIN`, `SELECT`, `FETCH` with a literal, `APPEND`
  continuation, `IDLE`, and `LOGOUT`.
- **A/B parity**: a deterministic Python model derived from the legacy grammar
  generates an IMAP corpus for N seeds; `fh-parity` fingerprints both sides and
  compares them exactly (string/integer domain, epsilon `0`).

Coverage target ≥ 95% branch coverage; clippy target 0 warnings with
`-D warnings`.

## 5. 4-Stage Benchmarking & PGO Pipeline Plan

1. `legacy_baseline` — Python reference model of the legacy parser (documented
   stand-in; a full Thunderbird build is out of scope for this workspace).
2. `safe_rust` — idiomatic safe port, zero hot-loop allocations in the
   tokenizer/`fast` scanner.
3. `soa_simd` — allocation-free `fast` scanner over byte slices, designed for
   auto-vectorisation; no per-response heap growth beyond the owned parse.
4. `pgo_lto` — `-Cprofile-generate` → representative workload → `llvm-profdata
   merge` → `-Cprofile-use` with `lto = "fat"` and `codegen-units = 1`.
   Fallback: `-C target-cpu=native -C opt-level=3 -C lto=fat`.

## 6. Phased Execution Roadmap

1. Scaffold seven crates, enforce `#![forbid(unsafe_code)]`, pin workspace
   dependencies (rustls, webpki-roots, tokio, async-compression, reqwest).
2. Implement `fuckthunder-core` (errors, capabilities, mailbox, flags, seq,
   status, response codes, dates, envelope).
3. Implement `fuckthunder-imap-proto` (tokenizer, response parser, command
   encoder, SEARCH, FETCH/BODYSTRUCTURE, fast scanner).
4. Implement SASL and OAuth (PKCE, flows, loopback server, token store).
5. Implement transport/connection/client; add mock-server integration test.
6. Run the verification crucible (tests + clippy), fix and log each iteration.
7. Run deterministic parity across seeds via `fh-parity`.
8. Run the 4-stage benchmark staircase, PGO, final census and infographic.

### Open questions

- Full Thunderbird parity would require building the XPCOM stack and a live
  IMAP server; the deterministic Python model is the documented substitute.
- TLS trust roots are `webpki-roots` (no system store); a future feature flag
  can add `rustls-native-certs`.
- `IDLE` is exposed as a bounded `idle_for(Duration)` rather than a long-lived
  callback stream; this keeps cancellation explicit.
