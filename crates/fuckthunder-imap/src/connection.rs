//! Low-level connection: framing, continuations, tagged command execution.

use std::time::Duration;

use fuckthunder_core::{Error, MailboxName, ResponseCode, Result, TaggedStatus};
use fuckthunder_imap_proto::fast::literal_length;
use fuckthunder_imap_proto::{parse_response, Command, Piece, Response, Untagged};
use fuckthunder_sasl::{decode_challenge, encode_response, Mechanism};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use crate::transport::Transport;
use crate::types::{ClientConfig, State};

/// Result of a successfully completed command.
#[derive(Clone, Debug, Default)]
pub struct CommandOutcome {
    /// Untagged responses received while the command ran.
    pub untagged: Vec<Response>,
    /// Response code attached to the tagged `OK`.
    pub code: Option<ResponseCode>,
    /// Human-readable completion text.
    pub text: String,
}

impl CommandOutcome {
    /// Returns the first response code of the requested kind.
    #[must_use]
    pub fn find_code(&self, predicate: impl Fn(&ResponseCode) -> bool) -> Option<&ResponseCode> {
        self.code.as_ref().filter(|code| predicate(code))
    }

    /// Returns the first untagged response matching a predicate.
    #[must_use]
    pub fn find_untagged(&self, predicate: impl Fn(&Untagged) -> bool) -> Option<&Untagged> {
        self.untagged.iter().find_map(|response| match response {
            Response::Untagged(untagged) if predicate(untagged) => Some(untagged),
            _ => None,
        })
    }
}

/// A live IMAP connection.
pub struct Connection {
    io: BufReader<Transport>,
    host: String,
    timeout: Duration,
    max_literal: usize,
    tag_counter: u32,
    capabilities: fuckthunder_core::CapabilitySet,
    literal_plus: bool,
    state: State,
    greeting: String,
    pending: Vec<Response>,
}

impl Connection {
    /// Opens the transport and consumes the server greeting.
    ///
    /// # Errors
    ///
    /// Fails on transport errors or a malformed greeting.
    pub async fn open(config: &ClientConfig) -> Result<Self> {
        let transport = Transport::connect(
            &config.host,
            config.port,
            config.tls,
            config.timeout,
            config.proxy.as_ref(),
        )
        .await?;
        let mut connection = Self {
            io: BufReader::new(transport),
            host: config.host.clone(),
            timeout: config.timeout,
            max_literal: config.max_literal,
            tag_counter: 0,
            capabilities: fuckthunder_core::CapabilitySet::new(),
            literal_plus: false,
            state: State::NotAuthenticated,
            greeting: String::new(),
            pending: Vec::new(),
        };
        connection.read_greeting().await?;
        Ok(connection)
    }

    /// Returns the negotiated capabilities.
    #[must_use]
    pub fn capabilities(&self) -> &fuckthunder_core::CapabilitySet {
        &self.capabilities
    }

    /// Returns the current state.
    #[must_use]
    pub fn state(&self) -> &State {
        &self.state
    }

    /// Marks a mailbox as selected.
    pub fn set_selected(&mut self, mailbox: MailboxName) {
        self.state = State::Selected(mailbox);
    }

    /// Marks the session as logged out.
    pub fn set_logout(&mut self) {
        self.state = State::Logout;
    }

    /// Returns the greeting text.
    #[must_use]
    pub fn greeting(&self) -> &str {
        &self.greeting
    }

    /// Returns `true` when `LITERAL+` is active.
    #[must_use]
    pub fn literal_plus(&self) -> bool {
        self.literal_plus
    }

    /// Returns `true` when TLS is active.
    #[must_use]
    pub fn is_tls(&self) -> bool {
        self.io.get_ref().is_tls()
    }

    /// Returns the configured timeout.
    #[must_use]
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    async fn read_greeting(&mut self) -> Result<()> {
        let buffer = self.read_response().await?;
        match parse_response(&buffer)? {
            Response::Untagged(Untagged::Ok { code, text }) => {
                self.greeting = text;
                if let Some(code) = code {
                    self.apply_code(&code);
                }
                Ok(())
            }
            Response::Untagged(Untagged::PreAuth { code, text }) => {
                self.state = State::Authenticated;
                self.greeting = text;
                if let Some(code) = code {
                    self.apply_code(&code);
                }
                Ok(())
            }
            Response::Untagged(Untagged::Bye { text, .. }) => {
                Err(Error::Closed(format!("server refused connection: {text}")))
            }
            other => Err(Error::Protocol(format!("unexpected greeting: {other:?}"))),
        }
    }

    /// Sends `STARTTLS` and upgrades the transport.
    ///
    /// # Errors
    ///
    /// Fails when the server rejects the command or the handshake fails.
    pub async fn start_tls(&mut self) -> Result<()> {
        if self.is_tls() {
            return Err(Error::Protocol("TLS already active".into()));
        }
        self.execute(Command::starttls()).await?;
        if !self.io.buffer().is_empty() {
            return Err(Error::Protocol(
                "server pipelined data before the TLS handshake".into(),
            ));
        }
        let host = self.host.clone();
        let timeout = self.timeout;
        self.io.get_mut().start_tls(&host, timeout).await?;
        // Capabilities must be re-read after the handshake.
        self.capabilities = fuckthunder_core::CapabilitySet::new();
        self.literal_plus = false;
        Ok(())
    }

    /// Enables DEFLATE compression after a successful `COMPRESS DEFLATE`.
    ///
    /// # Errors
    ///
    /// Fails when the server rejects the command.
    pub async fn compress(&mut self) -> Result<()> {
        self.execute(Command::compress_deflate()).await?;
        self.io.get_mut().enable_deflate()
    }

    /// Issues `CAPABILITY` and stores the result.
    ///
    /// # Errors
    ///
    /// Fails on transport or protocol errors.
    pub async fn capability(&mut self) -> Result<fuckthunder_core::CapabilitySet> {
        let outcome = self.execute(Command::capability()).await?;
        for response in &outcome.untagged {
            if let Response::Untagged(Untagged::Capability(caps)) = response {
                self.capabilities = caps.clone();
                self.literal_plus = caps.literal_plus();
            }
        }
        Ok(self.capabilities.clone())
    }

    /// Authenticates with `LOGIN`.
    ///
    /// # Errors
    ///
    /// Fails when the server rejects the credentials.
    pub async fn login(&mut self, user: &str, password: &str) -> Result<()> {
        self.execute(Command::login(user, password)?).await?;
        self.state = State::Authenticated;
        Ok(())
    }

    /// Authenticates with any SASL mechanism.
    ///
    /// # Errors
    ///
    /// Fails when the exchange is rejected or malformed.
    pub async fn authenticate(&mut self, mechanism: &mut dyn Mechanism) -> Result<()> {
        let initial = mechanism.initial_response().map(|r| encode_response(&r));
        let command = Command::authenticate(mechanism.name(), initial.as_deref())?;
        let tag = self.next_tag();
        let encoded = command.encode(&tag, false)?;
        for piece in encoded.pieces {
            self.write_piece(piece).await?;
        }

        loop {
            let buffer = self.read_response().await?;
            match parse_response(&buffer)? {
                Response::Continuation { text, .. } => {
                    let challenge = if text.is_empty() {
                        Vec::new()
                    } else {
                        decode_challenge(&text)?
                    };
                    match mechanism.step(&challenge) {
                        Ok(Some(response)) => {
                            let payload = if response.is_empty() {
                                "=\r\n".to_string()
                            } else {
                                format!("{}\r\n", encode_response(&response))
                            };
                            self.write_all(payload.as_bytes()).await?;
                        }
                        Ok(None) => self.write_all(b"\r\n").await?,
                        Err(error) => {
                            // Complete the exchange so the server can send its
                            // tagged NO instead of leaving the socket stuck.
                            let _ = self.write_all(b"\r\n").await;
                            let _ = self.drain_tagged(&tag).await;
                            return Err(error);
                        }
                    }
                }
                Response::Untagged(untagged) => {
                    self.observe(&untagged);
                    self.pending.push(Response::Untagged(untagged));
                }
                Response::Tagged {
                    tag: response_tag,
                    status,
                    code,
                    text,
                } => {
                    self.check_tag(&tag, &response_tag)?;
                    if status == TaggedStatus::Ok {
                        self.state = State::Authenticated;
                        return Ok(());
                    }
                    return Err(Error::Server { status, code, text });
                }
            }
        }
    }

    /// Runs a command and collects its untagged responses.
    ///
    /// # Errors
    ///
    /// Fails on transport errors, malformed responses, or `NO`/`BAD`.
    pub async fn execute(&mut self, command: Command) -> Result<CommandOutcome> {
        let tag = self.next_tag();
        let encoded = command.encode(&tag, self.literal_plus)?;
        let mut untagged = std::mem::take(&mut self.pending);
        for piece in encoded.pieces {
            self.write_piece(piece).await?;
        }
        loop {
            let buffer = self.read_response().await?;
            match parse_response(&buffer)? {
                Response::Untagged(item) => {
                    self.observe(&item);
                    untagged.push(Response::Untagged(item));
                }
                Response::Continuation { .. } => {
                    return Err(Error::Protocol("unexpected continuation request".into()));
                }
                Response::Tagged {
                    tag: response_tag,
                    status,
                    code,
                    text,
                } => {
                    self.check_tag(&tag, &response_tag)?;
                    if status == TaggedStatus::Ok {
                        return Ok(CommandOutcome {
                            untagged,
                            code,
                            text,
                        });
                    }
                    return Err(Error::Server { status, code, text });
                }
            }
        }
    }

    /// Reads the next response without sending anything (used by `IDLE`).
    ///
    /// # Errors
    ///
    /// Fails on transport or protocol errors.
    pub async fn read_untagged(&mut self) -> Result<Response> {
        let buffer = self.read_response().await?;
        let response = parse_response(&buffer)?;
        if let Response::Untagged(item) = &response {
            self.observe(item);
        }
        Ok(response)
    }

    /// Writes raw bytes to the server.
    ///
    /// # Errors
    ///
    /// Fails on transport errors.
    pub async fn write_all(&mut self, bytes: &[u8]) -> Result<()> {
        tokio::time::timeout(self.timeout, self.io.write_all(bytes))
            .await
            .map_err(|_| Error::Timeout)??;
        tokio::time::timeout(self.timeout, self.io.flush())
            .await
            .map_err(|_| Error::Timeout)??;
        Ok(())
    }

    /// Sends a command tag and waits for the matching completion.
    ///
    /// # Errors
    ///
    /// Fails when the response tag does not match or the server sends `NO`.
    pub async fn send_tagged(&mut self, tag: &str) -> Result<()> {
        loop {
            let buffer = self.read_response().await?;
            match parse_response(&buffer)? {
                Response::Untagged(item) => self.observe(&item),
                Response::Tagged {
                    tag: response_tag,
                    status,
                    code,
                    text,
                } => {
                    self.check_tag(tag, &response_tag)?;
                    if status == TaggedStatus::Ok {
                        return Ok(());
                    }
                    return Err(Error::Server { status, code, text });
                }
                Response::Continuation { .. } => {
                    return Err(Error::Protocol("unexpected continuation".into()));
                }
            }
        }
    }

    /// Next command tag (`a1`, `a2`, ...).
    #[must_use]
    pub fn next_tag(&mut self) -> String {
        self.tag_counter += 1;
        self.tag_counter.to_string()
    }

    fn check_tag(&self, expected: &str, actual: &str) -> Result<()> {
        if expected == actual {
            Ok(())
        } else {
            Err(Error::Protocol(format!(
                "response tag {actual:?} does not match command tag {expected:?}"
            )))
        }
    }

    async fn write_piece(&mut self, piece: Piece) -> Result<()> {
        match piece {
            Piece::Bytes(bytes) => self.write_all(&bytes).await,
            Piece::Literal { data, non_sync } => {
                if !non_sync {
                    self.wait_continuation().await?;
                }
                self.write_all(&data).await
            }
        }
    }

    async fn wait_continuation(&mut self) -> Result<()> {
        loop {
            let buffer = self.read_response().await?;
            match parse_response(&buffer)? {
                Response::Continuation { .. } => return Ok(()),
                Response::Untagged(item) => {
                    self.observe(&item);
                    self.pending.push(Response::Untagged(item));
                }
                Response::Tagged {
                    status, code, text, ..
                } => return Err(Error::Server { status, code, text }),
            }
        }
    }

    async fn drain_tagged(&mut self, tag: &str) -> Result<()> {
        loop {
            let buffer = self.read_response().await?;
            match parse_response(&buffer)? {
                Response::Tagged {
                    tag: response_tag, ..
                } => {
                    return self.check_tag(tag, &response_tag);
                }
                Response::Untagged(item) => self.observe(&item),
                Response::Continuation { .. } => {}
            }
        }
    }

    fn observe(&mut self, response: &Untagged) {
        match response {
            Untagged::Capability(caps) => {
                self.capabilities = caps.clone();
                self.literal_plus = caps.literal_plus();
            }
            Untagged::PreAuth { .. } => self.state = State::Authenticated,
            Untagged::Bye { .. } => self.state = State::Logout,
            Untagged::Ok {
                code: Some(code), ..
            } => self.apply_code(code),
            Untagged::Ok { code: None, .. } => {}
            _ => {}
        }
    }

    fn apply_code(&mut self, code: &ResponseCode) {
        if let ResponseCode::Capability(caps) = code {
            self.capabilities = caps.clone();
            self.literal_plus = caps.literal_plus();
        }
    }

    async fn read_response(&mut self) -> Result<Vec<u8>> {
        let timeout = self.timeout;
        let max_literal = self.max_literal;
        tokio::time::timeout(timeout, async move {
            let mut buffer = Vec::new();
            loop {
                let mut line = Vec::new();
                let read = self.io.read_until(b'\n', &mut line).await?;
                if read == 0 {
                    return Err(Error::Closed("server closed the connection".into()));
                }
                let trimmed = line
                    .strip_suffix(b"\r\n")
                    .or_else(|| line.strip_suffix(b"\n"))
                    .unwrap_or(&line);
                let literal = literal_length(trimmed);
                buffer.extend_from_slice(&line);
                if let Some(length) = literal {
                    if length > max_literal {
                        return Err(Error::Limit(format!(
                            "literal of {length} bytes exceeds limit {max_literal}"
                        )));
                    }
                    let mut data = vec![0u8; length];
                    self.io.read_exact(&mut data).await?;
                    buffer.extend_from_slice(&data);
                } else {
                    return Ok(buffer);
                }
            }
        })
        .await
        .map_err(|_| Error::Timeout)?
    }
}
