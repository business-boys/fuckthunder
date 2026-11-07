//! Provider configuration and endpoint discovery.

use fuckthunder_core::{Error, Result};
use url::Url;

use crate::flow::AuthRequest;
use crate::pkce::Pkce;

/// Well-known IMAP OAuth providers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Provider {
    /// Google / Gmail.
    Google,
    /// Microsoft 365 / Outlook.
    Microsoft,
    /// Yahoo Mail.
    Yahoo,
    /// Any RFC 8414-style provider.
    Custom {
        /// Authorization endpoint.
        authorize_url: String,
        /// Token endpoint.
        token_url: String,
        /// Device-authorization endpoint.
        device_url: Option<String>,
    },
}

impl Provider {
    /// Authorization endpoint.
    #[must_use]
    pub fn authorize_url(&self) -> &str {
        match self {
            Self::Google => "https://accounts.google.com/o/oauth2/v2/auth",
            Self::Microsoft => "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
            Self::Yahoo => "https://api.login.yahoo.com/oauth2/request_auth",
            Self::Custom { authorize_url, .. } => authorize_url,
        }
    }

    /// Token endpoint.
    #[must_use]
    pub fn token_url(&self) -> &str {
        match self {
            Self::Google => "https://oauth2.googleapis.com/token",
            Self::Microsoft => "https://login.microsoftonline.com/common/oauth2/v2.0/token",
            Self::Yahoo => "https://api.login.yahoo.com/oauth2/get_token",
            Self::Custom { token_url, .. } => token_url,
        }
    }

    /// Device-authorization endpoint, when supported.
    #[must_use]
    pub fn device_url(&self) -> Option<&str> {
        match self {
            Self::Google => Some("https://oauth2.googleapis.com/device/code"),
            Self::Microsoft => {
                Some("https://login.microsoftonline.com/common/oauth2/v2.0/devicecode")
            }
            Self::Yahoo => None,
            Self::Custom { device_url, .. } => device_url.as_deref(),
        }
    }

    /// Default IMAP scopes for the provider.
    #[must_use]
    pub fn default_scopes(&self) -> Vec<String> {
        match self {
            Self::Google => vec!["https://mail.google.com/".into()],
            Self::Microsoft => vec![
                "https://outlook.office.com/IMAP.AccessAsUser.All".into(),
                "offline_access".into(),
            ],
            Self::Yahoo => vec!["mail-w".into()],
            Self::Custom { .. } => Vec::new(),
        }
    }

    /// Returns `true` when the provider supports the device flow.
    #[must_use]
    pub fn supports_device_flow(&self) -> bool {
        self.device_url().is_some()
    }
}

/// Configuration for one OAuth client.
#[derive(Clone, Debug)]
pub struct OAuthConfig {
    /// Provider endpoints.
    pub provider: Provider,
    /// OAuth client identifier.
    pub client_id: String,
    /// Optional client secret (public clients use PKCE instead).
    pub client_secret: Option<String>,
    /// Requested scopes.
    pub scopes: Vec<String>,
    /// Use PKCE (recommended; required for public clients).
    pub use_pkce: bool,
    /// Extra authorization parameters (`access_type=offline`, `prompt=consent`).
    pub extra_auth_params: Vec<(String, String)>,
    /// Requested token type, when the provider needs it.
    pub token_type_hint: Option<String>,
}

impl OAuthConfig {
    /// Builds a configuration with the provider's default scopes and PKCE on.
    #[must_use]
    pub fn new(provider: Provider, client_id: impl Into<String>) -> Self {
        let scopes = provider.default_scopes();
        Self {
            provider,
            client_id: client_id.into(),
            client_secret: None,
            scopes,
            use_pkce: true,
            extra_auth_params: Vec::new(),
            token_type_hint: None,
        }
    }

    /// Replaces the scope list.
    #[must_use]
    pub fn with_scopes<I, S>(mut self, scopes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.scopes = scopes.into_iter().map(Into::into).collect();
        self
    }

    /// Sets a client secret.
    #[must_use]
    pub fn with_client_secret(mut self, secret: impl Into<String>) -> Self {
        self.client_secret = Some(secret.into());
        self
    }

    /// Disables PKCE (only for providers that reject it).
    #[must_use]
    pub fn without_pkce(mut self) -> Self {
        self.use_pkce = false;
        self
    }

    /// Adds an authorization parameter.
    #[must_use]
    pub fn with_auth_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra_auth_params.push((key.into(), value.into()));
        self
    }

    /// Builds the authorization URL, state and PKCE verifier.
    ///
    /// # Errors
    ///
    /// Fails when an endpoint is not a valid URL.
    pub fn authorization_request(&self, redirect_uri: &str) -> Result<AuthRequest> {
        let mut url = Url::parse(self.provider.authorize_url())
            .map_err(|e| Error::OAuth(format!("invalid authorize URL: {e}")))?;
        let state = crate::pkce::random_url_token(24);
        let pkce = self.use_pkce.then(Pkce::generate);
        {
            let mut query = url.query_pairs_mut();
            query
                .append_pair("response_type", "code")
                .append_pair("client_id", &self.client_id)
                .append_pair("redirect_uri", redirect_uri)
                .append_pair("state", &state);
            if !self.scopes.is_empty() {
                query.append_pair("scope", &self.scopes.join(" "));
            }
            if let Some(pkce) = &pkce {
                query
                    .append_pair("code_challenge", &pkce.challenge)
                    .append_pair("code_challenge_method", "S256");
            }
            for (key, value) in &self.extra_auth_params {
                query.append_pair(key, value);
            }
        }
        Ok(AuthRequest { url, state, pkce })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_endpoints() {
        assert!(Provider::Google
            .authorize_url()
            .starts_with("https://accounts.google.com"));
        assert!(Provider::Microsoft
            .token_url()
            .contains("login.microsoftonline.com"));
        assert!(Provider::Yahoo.device_url().is_none());
        assert!(Provider::Google.supports_device_flow());
        assert_eq!(
            Provider::Google.default_scopes(),
            ["https://mail.google.com/"]
        );
        assert!(Provider::Microsoft
            .default_scopes()
            .contains(&"offline_access".to_string()));
    }

    #[test]
    fn authorization_url_contains_required_parameters() {
        let config = OAuthConfig::new(Provider::Microsoft, "client-123")
            .with_scopes(["https://outlook.office.com/IMAP.AccessAsUser.All"])
            .with_auth_param("prompt", "consent");
        let request = config
            .authorization_request("http://127.0.0.1:8080/")
            .unwrap();
        let text = request.url.as_str();
        assert!(text.contains("response_type=code"));
        assert!(text.contains("client_id=client-123"));
        assert!(text.contains("code_challenge_method=S256"));
        assert!(text.contains("prompt=consent"));
        assert!(text.contains("state="));
        assert!(request.pkce.is_some());
    }

    #[test]
    fn pkce_can_be_disabled() {
        let config = OAuthConfig::new(Provider::Yahoo, "id").without_pkce();
        let request = config.authorization_request("http://localhost/").unwrap();
        assert!(request.pkce.is_none());
        assert!(!request.url.as_str().contains("code_challenge"));
    }

    #[test]
    fn custom_provider() {
        let provider = Provider::Custom {
            authorize_url: "https://id.example.org/authorize".into(),
            token_url: "https://id.example.org/token".into(),
            device_url: None,
        };
        let config = OAuthConfig::new(provider, "id");
        assert!(config.authorization_request("http://localhost/").is_ok());
    }
}
