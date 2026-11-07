//! OAuth 2.0 for IMAP providers.
//!
//! Thunderbird's `OAuth2.sys.mjs` implements the authorization-code flow with
//! PKCE, an optional localhost loopback redirect, token refresh, and a device
//! flow. This crate ports that behaviour to safe Rust:
//!
//! * [`OAuthConfig`] describes a provider (Google, Microsoft, Yahoo, custom).
//! * [`Pkce`] generates and verifies RFC 7636 verifiers/challenges.
//! * [`OAuthClient`] builds authorization URLs and exchanges/refreshes tokens.
//! * [`LoopbackServer`] receives the browser redirect on `127.0.0.1:0`.
//! * [`TokenSet`] stores access/refresh tokens with expiry bookkeeping.
//!
//! # Examples
//!
//! ```
//! use fuckthunder_oauth::{OAuthConfig, Provider};
//!
//! let config = OAuthConfig::new(Provider::Google, "client-id.apps.googleusercontent.com");
//! let request = config.authorization_request("http://127.0.0.1:49152/").unwrap();
//! assert!(request.url.as_str().starts_with("https://accounts.google.com/"));
//! assert!(request.url.as_str().contains("code_challenge_method=S256"));
//! assert!(request.pkce.is_some());
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod config;
pub mod flow;
pub mod loopback;
pub mod pkce;
pub mod token;

pub use config::{OAuthConfig, Provider};
pub use flow::{AuthRequest, DeviceCode, OAuthClient};
pub use loopback::LoopbackServer;
pub use pkce::{random_url_token, sha256_base64url, Pkce};
pub use token::{TokenSet, TokenStore};
