//! OAuth flow tests against a local mock token endpoint.

use fuckthunder_oauth::{OAuthClient, OAuthConfig, Provider};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Starts a mock HTTP/1.1 server that answers each request from `bodies`.
async fn mock_http(bodies: Vec<String>) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = tokio::spawn(async move {
        for body in bodies {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = vec![0u8; 65536];
            let _ = socket.read(&mut buffer).await.unwrap();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        }
    });
    (format!("http://127.0.0.1:{port}"), handle)
}

fn config(base: &str) -> OAuthConfig {
    OAuthConfig::new(
        Provider::Custom {
            authorize_url: format!("{base}/authorize"),
            token_url: format!("{base}/token"),
            device_url: Some(format!("{base}/device")),
        },
        "client-id",
    )
    .with_scopes(["mail.read"])
    .with_client_secret("shh")
}

#[tokio::test]
async fn refresh_and_exchange_code() {
    let (base, server) = mock_http(vec![
        r#"{"access_token":"at1","refresh_token":"rt1","token_type":"Bearer","expires_in":3600,"scope":"mail.read"}"#.into(),
        r#"{"access_token":"at2","token_type":"Bearer","expires_in":60}"#.into(),
    ])
    .await;
    let client = OAuthClient::new(config(&base)).unwrap();

    let tokens = client.refresh("rt0").await.unwrap();
    assert_eq!(tokens.access_token, "at1");
    assert_eq!(tokens.refresh_token.as_deref(), Some("rt1"));

    let tokens = client
        .exchange_code("code", "http://localhost/cb", None)
        .await
        .unwrap();
    assert_eq!(tokens.access_token, "at2");
    server.await.unwrap();
}

#[tokio::test]
async fn device_flow_polls_until_approved() {
    let (base, server) = mock_http(vec![
        r#"{"device_code":"dc","user_code":"ABCD","verification_uri":"https://example.org/activate","expires_in":60,"interval":1}"#.into(),
        r#"{"error":"authorization_pending","error_description":"waiting"}"#.into(),
        r#"{"error":"slow_down"}"#.into(),
        r#"{"access_token":"at-device","token_type":"Bearer","expires_in":60}"#.into(),
    ])
    .await;
    let client = OAuthClient::new(config(&base)).unwrap();

    let device = client.device_authorization().await.unwrap();
    assert_eq!(device.device_code, "dc");
    assert_eq!(device.user_code, "ABCD");

    assert!(client.poll_device_token("dc").await.unwrap().is_none());
    assert!(client.poll_device_token("dc").await.unwrap().is_none());
    let tokens = client.poll_device_token("dc").await.unwrap().unwrap();
    assert_eq!(tokens.access_token, "at-device");
    server.await.unwrap();
}

#[tokio::test]
async fn oauth_error_response_is_reported() {
    let (base, server) = mock_http(vec![
        r#"{"error":"invalid_grant","error_description":"expired"}"#.into(),
    ])
    .await;
    let client = OAuthClient::new(config(&base)).unwrap();
    let error = client.refresh("rt").await.unwrap_err();
    let text = format!("{error}");
    assert!(text.contains("invalid_grant"), "{text}");
    server.await.unwrap();
}

#[tokio::test]
async fn device_flow_requires_device_endpoint() {
    let client = OAuthClient::new(OAuthConfig::new(
        Provider::Custom {
            authorize_url: "http://localhost/a".into(),
            token_url: "http://localhost/t".into(),
            device_url: None,
        },
        "id",
    ))
    .unwrap();
    assert!(client.device_authorization().await.is_err());
}

#[test]
fn authorization_request_contains_pkce_and_state() {
    let client = OAuthClient::new(config("http://localhost")).unwrap();
    let request = client.authorization_request("http://localhost/cb").unwrap();
    let url = request.url.as_str();
    assert!(url.contains("response_type=code"), "{url}");
    assert!(url.contains("client_id=client-id"), "{url}");
    assert!(url.contains("code_challenge="), "{url}");
    assert!(!request.state.is_empty());
    assert!(request.pkce.is_some());
}
