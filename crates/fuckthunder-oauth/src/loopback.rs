//! Localhost loopback redirect listener for the authorization-code flow.

use std::time::Duration;

use fuckthunder_core::{Error, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// A one-shot HTTP listener bound to `127.0.0.1:0`.
///
/// ```
/// # tokio::runtime::Runtime::new().unwrap().block_on(async {
/// use fuckthunder_oauth::LoopbackServer;
/// let server = LoopbackServer::bind().await.unwrap();
/// assert!(server.redirect_uri().starts_with("http://127.0.0.1:"));
/// # });
/// ```
#[derive(Debug)]
pub struct LoopbackServer {
    listener: TcpListener,
    redirect_uri: String,
}

impl LoopbackServer {
    /// Binds an ephemeral port on the loopback interface.
    ///
    /// # Errors
    ///
    /// Fails when the socket cannot be bound.
    pub async fn bind() -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|e| Error::OAuth(format!("cannot bind loopback listener: {e}")))?;
        let port = listener
            .local_addr()
            .map_err(|e| Error::OAuth(format!("cannot read listener address: {e}")))?
            .port();
        Ok(Self {
            listener,
            redirect_uri: format!("http://127.0.0.1:{port}/"),
        })
    }

    /// Returns the redirect URI to register with the provider.
    #[must_use]
    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    /// Waits for the browser redirect and returns the authorization code.
    ///
    /// # Errors
    ///
    /// Fails on timeout, malformed HTTP, an `error` parameter, or a `state`
    /// mismatch.
    pub async fn wait_for_code(self, expected_state: &str, timeout: Duration) -> Result<String> {
        let accepted = tokio::time::timeout(timeout, self.listener.accept())
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|e| Error::OAuth(format!("loopback accept failed: {e}")))?;
        Self::handle(accepted.0, expected_state).await
    }

    async fn handle(mut stream: TcpStream, expected_state: &str) -> Result<String> {
        let mut buffer = vec![0u8; 8192];
        let read = stream
            .read(&mut buffer)
            .await
            .map_err(|e| Error::OAuth(format!("cannot read redirect: {e}")))?;
        let request = String::from_utf8_lossy(&buffer[..read]);
        let target = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .ok_or_else(|| Error::OAuth("malformed HTTP request line".into()))?;
        let url = url::Url::parse(&format!("http://127.0.0.1{target}"))
            .map_err(|e| Error::OAuth(format!("malformed redirect URL: {e}")))?;
        let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();

        let outcome = if let Some(error) = params.get("error") {
            Err(Error::OAuth(format!(
                "authorization denied: {error} ({})",
                params
                    .get("error_description")
                    .map(String::as_str)
                    .unwrap_or("no description")
            )))
        } else if params.get("state").map(String::as_str) != Some(expected_state) {
            Err(Error::OAuth("state mismatch on redirect".into()))
        } else {
            params
                .get("code")
                .cloned()
                .ok_or_else(|| Error::OAuth("redirect without code".into()))
        };

        let (status, body) = match &outcome {
            Ok(_) => (
                "200 OK",
                "Authorization complete. You can close this window.",
            ),
            Err(_) => (
                "400 Bad Request",
                "Authorization failed. You can close this window.",
            ),
        };
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.shutdown().await;
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    async fn hit(url: &str, path: &str) -> String {
        let addr = url
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_owned();
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        response
    }

    #[tokio::test]
    async fn receives_code() {
        let server = LoopbackServer::bind().await.unwrap();
        let uri = server.redirect_uri().to_owned();
        let handle = tokio::spawn(async move {
            server
                .wait_for_code("state-1", Duration::from_secs(5))
                .await
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        let response = hit(&uri, "/?code=abc123&state=state-1").await;
        assert!(response.contains("200 OK"));
        assert_eq!(handle.await.unwrap().unwrap(), "abc123");
    }

    #[tokio::test]
    async fn rejects_state_mismatch() {
        let server = LoopbackServer::bind().await.unwrap();
        let uri = server.redirect_uri().to_owned();
        let handle = tokio::spawn(async move {
            server
                .wait_for_code("expected", Duration::from_secs(5))
                .await
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        let response = hit(&uri, "/?code=abc&state=wrong").await;
        assert!(response.contains("400 Bad Request"));
        assert!(handle.await.unwrap().is_err());
    }

    #[tokio::test]
    async fn reports_provider_error() {
        let server = LoopbackServer::bind().await.unwrap();
        let uri = server.redirect_uri().to_owned();
        let handle =
            tokio::spawn(async move { server.wait_for_code("s", Duration::from_secs(5)).await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        hit(&uri, "/?error=access_denied&state=s").await;
        let err = handle.await.unwrap().unwrap_err();
        assert!(err.to_string().contains("access_denied"));
    }

    #[tokio::test]
    async fn times_out() {
        let server = LoopbackServer::bind().await.unwrap();
        let err = server
            .wait_for_code("s", Duration::from_millis(10))
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Timeout));
    }
}
