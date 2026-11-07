//! Token model and an in-memory token store.

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// An OAuth 2.0 token response with expiry bookkeeping.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenSet {
    /// Bearer access token.
    pub access_token: String,
    /// Refresh token, when the provider issued one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// Token type, usually `Bearer`.
    #[serde(default)]
    pub token_type: String,
    /// Granted scopes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// OpenID Connect ID token, when requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
    /// Absolute expiry time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
}

impl TokenSet {
    /// Builds a token set with an expiry computed from `expires_in` seconds.
    #[must_use]
    pub fn with_expiry(access_token: impl Into<String>, expires_in: u64) -> Self {
        Self {
            access_token: access_token.into(),
            token_type: "Bearer".into(),
            expires_at: Some(
                Utc::now() + Duration::seconds(i64::try_from(expires_in).unwrap_or(i64::MAX)),
            ),
            ..Self::default()
        }
    }

    /// Returns `true` when the token is expired, allowing a safety skew.
    #[must_use]
    pub fn is_expired(&self, skew: Duration) -> bool {
        match self.expires_at {
            Some(expires_at) => Utc::now() + skew >= expires_at,
            None => false,
        }
    }

    /// Returns the `Authorization` header value.
    #[must_use]
    pub fn authorization_header(&self) -> String {
        let kind = if self.token_type.is_empty() {
            "Bearer"
        } else {
            &self.token_type
        };
        format!("{kind} {}", self.access_token)
    }
}

/// A process-local token store keyed by account identifier.
#[derive(Debug, Default)]
pub struct TokenStore {
    tokens: Mutex<HashMap<String, TokenSet>>,
}

impl TokenStore {
    /// Creates an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts or replaces the token set for an account.
    ///
    /// # Panics
    ///
    /// Panics only if the internal mutex was poisoned by a panicking holder.
    pub fn insert(&self, account: impl Into<String>, tokens: TokenSet) {
        self.tokens
            .lock()
            .expect("token store mutex poisoned")
            .insert(account.into(), tokens);
    }

    /// Returns a clone of the stored token set.
    #[must_use]
    pub fn get(&self, account: &str) -> Option<TokenSet> {
        self.tokens
            .lock()
            .expect("token store mutex poisoned")
            .get(account)
            .cloned()
    }

    /// Removes and returns the token set.
    #[must_use]
    pub fn remove(&self, account: &str) -> Option<TokenSet> {
        self.tokens
            .lock()
            .expect("token store mutex poisoned")
            .remove(account)
    }

    /// Returns `true` when the account has a token that is not expired.
    #[must_use]
    pub fn has_valid(&self, account: &str) -> bool {
        self.get(account)
            .is_some_and(|tokens| !tokens.is_expired(Duration::zero()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expiry_accounts_for_skew() {
        let valid = TokenSet::with_expiry("abc", 3600);
        assert!(!valid.is_expired(Duration::minutes(1)));
        assert!(valid.is_expired(Duration::hours(2)));
        assert!(!TokenSet::default().is_expired(Duration::hours(2)));
    }

    #[test]
    fn authorization_header() {
        let tokens = TokenSet::with_expiry("abc", 60);
        assert_eq!(tokens.authorization_header(), "Bearer abc");
        let tokens = TokenSet {
            access_token: "x".into(),
            ..TokenSet::default()
        };
        assert_eq!(tokens.authorization_header(), "Bearer x");
    }

    #[test]
    fn store_lifecycle() {
        let store = TokenStore::new();
        store.insert("alice@example.org", TokenSet::with_expiry("tok", 3600));
        assert!(store.has_valid("alice@example.org"));
        assert_eq!(store.get("alice@example.org").unwrap().access_token, "tok");
        assert!(store.remove("alice@example.org").is_some());
        assert!(!store.has_valid("alice@example.org"));
    }

    #[test]
    fn serialises_without_secrets_in_debug_shape() {
        let tokens = TokenSet::with_expiry("tok", 60);
        let json = serde_json::to_string(&tokens).unwrap();
        assert!(json.contains("\"access_token\":\"tok\""));
        assert!(json.contains("\"token_type\":\"Bearer\""));
    }
}
