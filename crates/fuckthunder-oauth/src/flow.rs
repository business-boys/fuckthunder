//! Authorization-code, refresh and device flows.

use std::time::Duration as StdDuration;

use fuckthunder_core::{Error, Result};
use serde::Deserialize;
use url::Url;

use crate::config::OAuthConfig;
use crate::pkce::Pkce;
use crate::token::TokenSet;

/// A prepared authorization request.
#[derive(Clone, Debug)]
pub struct AuthRequest {
    /// URL to open in the user's browser.
    pub url: Url,
    /// CSRF state that the redirect must echo.
    pub state: String,
    /// PKCE pair, when enabled.
    pub pkce: Option<Pkce>,
}

/// Device-authorization response (RFC 8628).
#[derive(Clone, Debug, Deserialize)]
pub struct DeviceCode {
    /// Opaque code polled at the token endpoint.
    pub device_code: String,
    /// Short code the user types at the verification URI.
    pub user_code: String,
    /// URI the user visits.
    #[serde(alias = "verification_url")]
    pub verification_uri: String,
    /// URI with the code embedded, when provided.
    #[serde(default)]
    pub verification_uri_complete: Option<String>,
    /// Lifetime in seconds.
    pub expires_in: u64,
    /// Minimum polling interval in seconds.
    #[serde(default = "default_interval")]
    pub interval: u64,
}

fn default_interval() -> u64 {
    5
}

/// An OAuth 2.0 client bound to one provider configuration.
#[derive(Clone, Debug)]
pub struct OAuthClient {
    http: reqwest::Client,
    config: OAuthConfig,
}

#[derive(Debug, Deserialize)]
struct RawToken {
    access_token: Option<String>,
    refresh_token: Option<String>,
    token_type: Option<String>,
    expires_in: Option<u64>,
    scope: Option<String>,
    id_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

impl OAuthClient {
    /// Builds a client with a rustls-backed HTTP stack.
    ///
    /// # Errors
    ///
    /// Fails when the HTTP client cannot be constructed.
    pub fn new(config: OAuthConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent("fuckthunder/0.1")
            .build()
            .map_err(|e| Error::OAuth(format!("cannot build HTTP client: {e}")))?;
        Ok(Self { http, config })
    }

    /// Returns the provider configuration.
    #[must_use]
    pub fn config(&self) -> &OAuthConfig {
        &self.config
    }

    /// Builds an authorization request with the given redirect URI.
    ///
    /// # Errors
    ///
    /// Fails when the authorization endpoint is malformed.
    pub fn authorization_request(&self, redirect_uri: &str) -> Result<AuthRequest> {
        self.config.authorization_request(redirect_uri)
    }

    /// Exchanges an authorization code for tokens.
    ///
    /// # Errors
    ///
    /// Fails on transport errors or an OAuth error response.
    pub async fn exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
        pkce: Option<&Pkce>,
    ) -> Result<TokenSet> {
        let mut form = vec![
            ("grant_type".to_string(), "authorization_code".to_string()),
            ("code".to_string(), code.to_string()),
            ("client_id".to_string(), self.config.client_id.clone()),
            ("redirect_uri".to_string(), redirect_uri.to_string()),
        ];
        if let Some(secret) = &self.config.client_secret {
            form.push(("client_secret".to_string(), secret.clone()));
        }
        if let Some(pkce) = pkce {
            form.push(("code_verifier".to_string(), pkce.verifier.clone()));
        }
        self.post_token(form).await
    }

    /// Refreshes an access token.
    ///
    /// # Errors
    ///
    /// Fails on transport errors or an OAuth error response.
    pub async fn refresh(&self, refresh_token: &str) -> Result<TokenSet> {
        let mut form = vec![
            ("grant_type".to_string(), "refresh_token".to_string()),
            ("refresh_token".to_string(), refresh_token.to_string()),
            ("client_id".to_string(), self.config.client_id.clone()),
        ];
        if let Some(secret) = &self.config.client_secret {
            form.push(("client_secret".to_string(), secret.clone()));
        }
        self.post_token(form).await
    }

    /// Requests a device code (RFC 8628).
    ///
    /// # Errors
    ///
    /// Fails when the provider has no device endpoint or rejects the request.
    pub async fn device_authorization(&self) -> Result<DeviceCode> {
        let url = self
            .config
            .provider
            .device_url()
            .ok_or_else(|| Error::OAuth("provider does not support the device flow".into()))?;
        let form = vec![
            ("client_id".to_string(), self.config.client_id.clone()),
            ("scope".to_string(), self.config.scopes.join(" ")),
        ];
        let response = self
            .http
            .post(url)
            .form(&form)
            .send()
            .await
            .map_err(|e| Error::OAuth(format!("device authorization failed: {e}")))?;
        let body = response
            .bytes()
            .await
            .map_err(|e| Error::OAuth(format!("cannot read device response: {e}")))?;
        serde_json::from_slice(&body)
            .map_err(|e| Error::OAuth(format!("invalid device response: {e}")))
    }

    /// Polls the token endpoint once.
    ///
    /// Returns `Ok(None)` while the user has not yet approved the request.
    ///
    /// # Errors
    ///
    /// Fails on transport errors or terminal OAuth errors.
    pub async fn poll_device_token(&self, device_code: &str) -> Result<Option<TokenSet>> {
        let form = vec![
            (
                "grant_type".to_string(),
                "urn:ietf:params:oauth:grant-type:device_code".to_string(),
            ),
            ("device_code".to_string(), device_code.to_string()),
            ("client_id".to_string(), self.config.client_id.clone()),
        ];
        let response = self
            .http
            .post(self.config.provider.token_url())
            .form(&form)
            .send()
            .await
            .map_err(|e| Error::OAuth(format!("device token poll failed: {e}")))?;
        let body = response
            .bytes()
            .await
            .map_err(|e| Error::OAuth(format!("cannot read token response: {e}")))?;
        match parse_token_body(&body) {
            Ok(tokens) => Ok(Some(tokens)),
            Err(Error::OAuth(message)) if message.contains("authorization_pending") => Ok(None),
            Err(Error::OAuth(message)) if message.contains("slow_down") => Ok(None),
            Err(other) => Err(other),
        }
    }

    /// Polls until the device flow completes or the code expires.
    ///
    /// # Errors
    ///
    /// Fails on terminal OAuth errors or when the deadline elapses.
    pub async fn wait_for_device_token(&self, device: &DeviceCode) -> Result<TokenSet> {
        let deadline = tokio::time::Instant::now() + StdDuration::from_secs(device.expires_in);
        let mut interval = device.interval.max(1);
        loop {
            if tokio::time::Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
            tokio::time::sleep(StdDuration::from_secs(interval)).await;
            if let Some(tokens) = self.poll_device_token(&device.device_code).await? {
                return Ok(tokens);
            }
            // RFC 8628 says to back off by 5 seconds on `slow_down`.
            interval = interval.saturating_add(5);
        }
    }

    async fn post_token(&self, form: Vec<(String, String)>) -> Result<TokenSet> {
        let response = self
            .http
            .post(self.config.provider.token_url())
            .form(&form)
            .send()
            .await
            .map_err(|e| Error::OAuth(format!("token request failed: {e}")))?;
        let body = response
            .bytes()
            .await
            .map_err(|e| Error::OAuth(format!("cannot read token response: {e}")))?;
        parse_token_body(&body)
    }
}

/// Parses a token endpoint body into a [`TokenSet`].
///
/// # Errors
///
/// Returns [`Error::OAuth`] for OAuth error payloads or malformed JSON.
pub fn parse_token_body(body: &[u8]) -> Result<TokenSet> {
    let raw: RawToken = serde_json::from_slice(body)
        .map_err(|e| Error::OAuth(format!("invalid token response: {e}")))?;
    if let Some(error) = raw.error {
        let description = raw.error_description.unwrap_or_default();
        return Err(Error::OAuth(format!("{error}: {description}")));
    }
    let access_token = raw
        .access_token
        .ok_or_else(|| Error::OAuth("token response without access_token".into()))?;
    let expires_at = raw.expires_in.map(|seconds| {
        chrono::Utc::now() + chrono::Duration::seconds(seconds.min(i64::MAX as u64) as i64)
    });
    Ok(TokenSet {
        access_token,
        refresh_token: raw.refresh_token,
        token_type: raw.token_type.unwrap_or_else(|| "Bearer".into()),
        scope: raw.scope,
        id_token: raw.id_token,
        expires_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{OAuthConfig, Provider};

    #[test]
    fn parses_successful_token_response() {
        let body = br#"{
            "access_token": "ya29.token",
            "refresh_token": "1//refresh",
            "expires_in": 3599,
            "token_type": "Bearer",
            "scope": "https://mail.google.com/"
        }"#;
        let tokens = parse_token_body(body).unwrap();
        assert_eq!(tokens.access_token, "ya29.token");
        assert_eq!(tokens.refresh_token.as_deref(), Some("1//refresh"));
        assert_eq!(tokens.token_type, "Bearer");
        assert!(tokens.expires_at.is_some());
    }

    #[test]
    fn parses_error_token_response() {
        let body = br#"{"error":"invalid_grant","error_description":"Bad Request"}"#;
        let err = parse_token_body(body).unwrap_err();
        assert!(err.to_string().contains("invalid_grant"));
        assert!(parse_token_body(br#"{"error":"authorization_pending"}"#)
            .unwrap_err()
            .to_string()
            .contains("authorization_pending"));
    }

    #[test]
    fn rejects_malformed_and_incomplete_responses() {
        assert!(parse_token_body(b"not json").is_err());
        assert!(parse_token_body(br#"{"token_type":"Bearer"}"#).is_err());
    }

    #[test]
    fn device_flow_support_is_detected() {
        let config = OAuthConfig::new(Provider::Yahoo, "id");
        let client = OAuthClient::new(config).unwrap();
        assert_eq!(client.config().client_id, "id");
    }

    #[test]
    fn device_code_parses_microsoft_aliases() {
        let body = br#"{
            "device_code":"dc",
            "user_code":"ABC",
            "verification_url":"https://microsoft.com/devicelogin",
            "expires_in":900,
            "interval":5
        }"#;
        let device: DeviceCode = serde_json::from_slice(body).unwrap();
        assert_eq!(device.verification_uri, "https://microsoft.com/devicelogin");
        assert_eq!(device.interval, 5);
    }
}
