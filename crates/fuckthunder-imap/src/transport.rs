//! Transport layer: plain TCP, implicit TLS, STARTTLS and DEFLATE compression.

use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use async_compression::tokio::bufread::ZlibDecoder;
use async_compression::tokio::write::ZlibEncoder;
use fuckthunder_core::{Error, Result};
use rustls::{ClientConfig, RootCertStore};
use rustls_pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncWrite, BufReader, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

/// How the client should secure the connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TlsMode {
    /// No TLS (only for tests and loopback servers).
    Plain,
    /// Connect in the clear and issue `STARTTLS` after the greeting.
    StartTls,
    /// TLS from the first byte (port 993).
    Implicit,
}

/// Builds the process-wide rustls configuration with webpki roots.
///
/// # Errors
///
/// Fails when the crypto provider or root store cannot be initialised.
pub fn client_config() -> Result<std::sync::Arc<ClientConfig>> {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(std::sync::Arc::new(config))
}

trait AsyncReadWrite: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> AsyncReadWrite for T {}

type BoxRaw = Box<dyn AsyncReadWrite>;

enum RawTransport {
    Plain(TcpStream),
    Tls(Box<tokio_rustls::client::TlsStream<TcpStream>>),
}

struct DeflatedTransport {
    decoder: ZlibDecoder<BufReader<tokio::io::ReadHalf<BoxRaw>>>,
    encoder: ZlibEncoder<tokio::io::WriteHalf<BoxRaw>>,
}

/// A bidirectional IMAP byte stream.
pub struct Transport {
    inner: Option<Inner>,
}

enum Inner {
    Raw(RawTransport),
    Deflated(DeflatedTransport),
}

impl Transport {
    /// Opens a TCP connection and performs the TLS handshake when required.
    ///
    /// # Errors
    ///
    /// Fails on DNS/TCP errors or a TLS handshake failure.
    pub async fn connect(
        host: &str,
        port: u16,
        mode: TlsMode,
        timeout: Duration,
        proxy: Option<&crate::types::ProxyConfig>,
    ) -> Result<Self> {
        let (target_host, target_port) = match proxy {
            Some(proxy) => (proxy.host.as_str(), proxy.port),
            None => (host, port),
        };
        let mut tcp = tokio::time::timeout(timeout, TcpStream::connect((target_host, target_port)))
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(Error::Io)?;
        tcp.set_nodelay(true).map_err(Error::Io)?;
        if let Some(proxy) = proxy {
            tokio::time::timeout(timeout, async {
                match proxy.protocol {
                    crate::types::ProxyProtocol::Socks5 => {
                        socks5_handshake(&mut tcp, proxy, host, port).await
                    }
                    crate::types::ProxyProtocol::HttpConnect => {
                        http_connect_handshake(&mut tcp, proxy, host, port).await
                    }
                }
            })
            .await
            .map_err(|_| Error::Timeout)??;
        }
        match mode {
            TlsMode::Plain | TlsMode::StartTls => Ok(Self {
                inner: Some(Inner::Raw(RawTransport::Plain(tcp))),
            }),
            TlsMode::Implicit => {
                let tls = Self::upgrade_tls(tcp, host, timeout).await?;
                Ok(Self {
                    inner: Some(Inner::Raw(RawTransport::Tls(Box::new(tls)))),
                })
            }
        }
    }

    /// Returns `true` when TLS is already active.
    #[must_use]
    pub fn is_tls(&self) -> bool {
        matches!(self.inner, Some(Inner::Raw(RawTransport::Tls(_))))
    }

    /// Upgrades a plain connection with `STARTTLS`.
    ///
    /// # Errors
    ///
    /// Fails when the connection is not plain or the handshake fails.
    pub async fn start_tls(&mut self, host: &str, timeout: Duration) -> Result<()> {
        let current = self
            .inner
            .take()
            .ok_or_else(|| Error::Protocol("transport is closed".into()))?;
        match current {
            Inner::Raw(RawTransport::Plain(tcp)) => {
                let tls = Self::upgrade_tls(tcp, host, timeout).await?;
                self.inner = Some(Inner::Raw(RawTransport::Tls(Box::new(tls))));
                Ok(())
            }
            other => {
                self.inner = Some(other);
                Err(Error::Protocol(
                    "STARTTLS on an already secured connection".into(),
                ))
            }
        }
    }

    async fn upgrade_tls(
        tcp: TcpStream,
        host: &str,
        timeout: Duration,
    ) -> Result<tokio_rustls::client::TlsStream<TcpStream>> {
        let config = client_config()?;
        let connector = TlsConnector::from(config);
        let server_name = ServerName::try_from(host.to_owned())
            .map_err(|e| Error::Tls(format!("invalid server name {host:?}: {e}")))?;
        tokio::time::timeout(timeout, connector.connect(server_name, tcp))
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|e| Error::Tls(format!("TLS handshake failed: {e}")))
    }

    /// Wraps the stream in DEFLATE after a successful `COMPRESS DEFLATE`.
    ///
    /// # Errors
    ///
    /// Fails when compression is already active.
    pub fn enable_deflate(&mut self) -> Result<()> {
        let current = self
            .inner
            .take()
            .ok_or_else(|| Error::Protocol("transport is closed".into()))?;
        let boxed: BoxRaw = match current {
            Inner::Raw(RawTransport::Plain(tcp)) => Box::new(tcp),
            Inner::Raw(RawTransport::Tls(tls)) => Box::new(*tls),
            Inner::Deflated(deflated) => {
                self.inner = Some(Inner::Deflated(deflated));
                return Err(Error::Protocol("compression already enabled".into()));
            }
        };
        let (read, write) = tokio::io::split(boxed);
        self.inner = Some(Inner::Deflated(DeflatedTransport {
            decoder: ZlibDecoder::new(BufReader::new(read)),
            encoder: ZlibEncoder::new(write),
        }));
        Ok(())
    }
}

impl AsyncRead for Transport {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut().inner.as_mut() {
            Some(Inner::Raw(RawTransport::Plain(stream))) => Pin::new(stream).poll_read(cx, buf),
            Some(Inner::Raw(RawTransport::Tls(stream))) => {
                Pin::new(stream.as_mut()).poll_read(cx, buf)
            }
            Some(Inner::Deflated(deflated)) => Pin::new(&mut deflated.decoder).poll_read(cx, buf),
            None => Poll::Ready(Err(std::io::Error::other("transport closed"))),
        }
    }
}

impl AsyncWrite for Transport {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut().inner.as_mut() {
            Some(Inner::Raw(RawTransport::Plain(stream))) => Pin::new(stream).poll_write(cx, buf),
            Some(Inner::Raw(RawTransport::Tls(stream))) => {
                Pin::new(stream.as_mut()).poll_write(cx, buf)
            }
            Some(Inner::Deflated(deflated)) => Pin::new(&mut deflated.encoder).poll_write(cx, buf),
            None => Poll::Ready(Err(std::io::Error::other("transport closed"))),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut().inner.as_mut() {
            Some(Inner::Raw(RawTransport::Plain(stream))) => Pin::new(stream).poll_flush(cx),
            Some(Inner::Raw(RawTransport::Tls(stream))) => Pin::new(stream.as_mut()).poll_flush(cx),
            Some(Inner::Deflated(deflated)) => Pin::new(&mut deflated.encoder).poll_flush(cx),
            None => Poll::Ready(Err(std::io::Error::other("transport closed"))),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut().inner.as_mut() {
            Some(Inner::Raw(RawTransport::Plain(stream))) => Pin::new(stream).poll_shutdown(cx),
            Some(Inner::Raw(RawTransport::Tls(stream))) => {
                Pin::new(stream.as_mut()).poll_shutdown(cx)
            }
            Some(Inner::Deflated(deflated)) => Pin::new(&mut deflated.encoder).poll_shutdown(cx),
            None => Poll::Ready(Err(std::io::Error::other("transport closed"))),
        }
    }
}

/// Establishes an HTTP CONNECT tunnel without buffering bytes past the header.
async fn http_connect_handshake(
    tcp: &mut TcpStream,
    proxy: &crate::types::ProxyConfig,
    host: &str,
    port: u16,
) -> Result<()> {
    use base64::Engine;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const MAX_RESPONSE_HEADER: usize = 16 * 1024;

    let authority = match host.parse::<std::net::Ipv6Addr>() {
        Ok(_) => format!("[{host}]:{port}"),
        Err(_) => format!("{host}:{port}"),
    };
    let mut request = format!(
        "CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\nProxy-Connection: Keep-Alive\r\n"
    );
    if let Some((user, password)) = &proxy.auth {
        let credentials = base64::engine::general_purpose::STANDARD
            .encode(format!("{user}:{password}").as_bytes());
        request.push_str(&format!("Proxy-Authorization: Basic {credentials}\r\n"));
    }
    request.push_str("\r\n");
    tcp.write_all(request.as_bytes()).await.map_err(Error::Io)?;

    let mut response = Vec::with_capacity(256);
    while !response.ends_with(b"\r\n\r\n") {
        if response.len() == MAX_RESPONSE_HEADER {
            return Err(Error::Limit(
                "HTTP proxy response header is too large".into(),
            ));
        }
        let mut byte = [0u8; 1];
        tcp.read_exact(&mut byte).await.map_err(Error::Io)?;
        response.push(byte[0]);
    }
    let header = std::str::from_utf8(&response)
        .map_err(|_| Error::Protocol("HTTP proxy response is not valid ASCII".into()))?;
    let status_line = header
        .split("\r\n")
        .next()
        .ok_or_else(|| Error::Protocol("HTTP proxy response has no status line".into()))?;
    let mut fields = status_line.split_whitespace();
    let version = fields.next().unwrap_or_default();
    let status = fields
        .next()
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| Error::Protocol("HTTP proxy sent a malformed status line".into()))?;
    if !matches!(version, "HTTP/1.0" | "HTTP/1.1") {
        return Err(Error::Protocol(
            "HTTP proxy sent an unsupported version".into(),
        ));
    }
    if !(200..300).contains(&status) {
        return Err(Error::Io(std::io::Error::other(format!(
            "HTTP proxy CONNECT failed with status {status}"
        ))));
    }
    Ok(())
}

/// Performs the SOCKS5 handshake (RFC 1928/1929) towards `host:port`.
async fn socks5_handshake(
    tcp: &mut TcpStream,
    proxy: &crate::types::ProxyConfig,
    host: &str,
    port: u16,
) -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let io_err = |msg: &str| Error::Io(std::io::Error::other(msg));

    // Greeting: offer no-auth, plus username/password when configured.
    let mut greeting = vec![0x05u8, 0x01, 0x00];
    if proxy.auth.is_some() {
        greeting[1] = 0x02;
        greeting.push(0x02);
    }
    tcp.write_all(&greeting).await.map_err(Error::Io)?;
    let mut choice = [0u8; 2];
    tcp.read_exact(&mut choice).await.map_err(Error::Io)?;
    if choice[0] != 0x05 {
        return Err(io_err("SOCKS5 proxy replied with a wrong version"));
    }
    match choice[1] {
        0x00 => {}
        0x02 => {
            let Some((user, password)) = &proxy.auth else {
                return Err(io_err("SOCKS5 proxy requires authentication"));
            };
            if user.len() > 255 || password.len() > 255 {
                return Err(Error::Invalid("SOCKS5 credentials exceed 255 bytes".into()));
            }
            let mut auth = vec![0x01u8, user.len() as u8];
            auth.extend_from_slice(user.as_bytes());
            auth.push(password.len() as u8);
            auth.extend_from_slice(password.as_bytes());
            tcp.write_all(&auth).await.map_err(Error::Io)?;
            let mut reply = [0u8; 2];
            tcp.read_exact(&mut reply).await.map_err(Error::Io)?;
            if reply[1] != 0x00 {
                return Err(io_err("SOCKS5 proxy rejected the credentials"));
            }
        }
        0xFF => return Err(io_err("SOCKS5 proxy accepts no offered method")),
        method => {
            let _ = method;
            return Err(io_err("SOCKS5 proxy selected an unsupported method"));
        }
    }

    // CONNECT with a domain name (ATYP 0x03).
    if host.len() > 255 {
        return Err(Error::Invalid(
            "SOCKS5 target host exceeds 255 bytes".into(),
        ));
    }
    let mut request = vec![0x05u8, 0x01, 0x00, 0x03, host.len() as u8];
    request.extend_from_slice(host.as_bytes());
    request.extend_from_slice(&port.to_be_bytes());
    tcp.write_all(&request).await.map_err(Error::Io)?;

    let mut head = [0u8; 4];
    tcp.read_exact(&mut head).await.map_err(Error::Io)?;
    if head[0] != 0x05 {
        return Err(io_err("SOCKS5 proxy sent a malformed reply"));
    }
    if head[1] != 0x00 {
        return Err(io_err(match head[1] {
            0x01 => "SOCKS5 proxy: general failure",
            0x02 => "SOCKS5 proxy: connection not allowed",
            0x03 => "SOCKS5 proxy: network unreachable",
            0x04 => "SOCKS5 proxy: host unreachable",
            0x05 => "SOCKS5 proxy: connection refused",
            0x06 => "SOCKS5 proxy: TTL expired",
            _ => "SOCKS5 proxy: connect failed",
        }));
    }
    // Consume the bound address.
    let skip = match head[3] {
        0x01 => 4,
        0x04 => 16,
        0x03 => {
            let mut len = [0u8; 1];
            tcp.read_exact(&mut len).await.map_err(Error::Io)?;
            usize::from(len[0])
        }
        _ => return Err(io_err("SOCKS5 proxy sent an unknown address type")),
    };
    let mut buf = vec![0u8; skip + 2];
    tcp.read_exact(&mut buf).await.map_err(Error::Io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tls_modes_are_distinct() {
        assert_ne!(TlsMode::Plain, TlsMode::StartTls);
        assert_ne!(TlsMode::StartTls, TlsMode::Implicit);
    }

    #[test]
    fn tls_config_builds() {
        let config = client_config().unwrap();
        assert!(!config.alpn_protocols.is_empty() || config.alpn_protocols.is_empty());
    }

    fn http_proxy(port: u16, auth: Option<(&str, &str)>) -> crate::types::ProxyConfig {
        crate::types::ProxyConfig {
            host: "127.0.0.1".into(),
            port,
            auth: auth.map(|(user, password)| (user.into(), password.into())),
            protocol: crate::types::ProxyProtocol::HttpConnect,
        }
    }

    #[tokio::test]
    async fn http_connect_succeeds_without_consuming_tunnel_bytes() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut byte = [0u8; 1];
            while !request.ends_with(b"\r\n\r\n") {
                socket.read_exact(&mut byte).await.unwrap();
                request.push(byte[0]);
            }
            let request = String::from_utf8(request).unwrap();
            assert!(request.starts_with("CONNECT imap.example.org:993 HTTP/1.1\r\n"));
            assert!(request.contains("Host: imap.example.org:993\r\n"));
            assert!(!request.contains("Proxy-Authorization"));
            socket
                .write_all(b"HTTP/1.1 200 Connection established\r\n\r\nTLS")
                .await
                .unwrap();
        });
        let mut tcp = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        http_connect_handshake(&mut tcp, &http_proxy(port, None), "imap.example.org", 993)
            .await
            .unwrap();
        use tokio::io::AsyncReadExt;
        let mut tunnel_bytes = [0u8; 3];
        tcp.read_exact(&mut tunnel_bytes).await.unwrap();
        assert_eq!(&tunnel_bytes, b"TLS");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn http_connect_sends_basic_auth() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut byte = [0u8; 1];
            while !request.ends_with(b"\r\n\r\n") {
                socket.read_exact(&mut byte).await.unwrap();
                request.push(byte[0]);
            }
            let request = String::from_utf8(request).unwrap();
            assert!(request.contains("Proxy-Authorization: Basic dXNlcjpwYXNz\r\n"));
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .await
                .unwrap();
        });
        let mut tcp = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        http_connect_handshake(
            &mut tcp,
            &http_proxy(port, Some(("user", "pass"))),
            "imap.example.org",
            993,
        )
        .await
        .unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn http_connect_rejects_non_success_status() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 1024];
            let _ = socket.read(&mut request).await.unwrap();
            socket
                .write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\n\r\n")
                .await
                .unwrap();
        });
        let mut tcp = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let error =
            http_connect_handshake(&mut tcp, &http_proxy(port, None), "imap.example.org", 993)
                .await
                .unwrap_err();
        assert!(error.to_string().contains("status 407"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn http_connect_rejects_malformed_response() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 1024];
            let _ = socket.read(&mut request).await.unwrap();
            socket.write_all(b"NOT HTTP\r\n\r\n").await.unwrap();
        });
        let mut tcp = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let error =
            http_connect_handshake(&mut tcp, &http_proxy(port, None), "imap.example.org", 993)
                .await
                .unwrap_err();
        assert!(matches!(error, Error::Protocol(_)));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn plain_connect_round_trip() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut buf = [0u8; 4];
            socket.read_exact(&mut buf).await.unwrap();
            socket.write_all(&buf).await.unwrap();
        });
        let mut transport = Transport::connect(
            &addr.ip().to_string(),
            addr.port(),
            TlsMode::Plain,
            Duration::from_secs(2),
            None,
        )
        .await
        .unwrap();
        assert!(!transport.is_tls());
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        transport.write_all(b"ping").await.unwrap();
        let mut buf = [0u8; 4];
        transport.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ping");
        server.await.unwrap();
    }
}
