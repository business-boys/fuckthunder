//! Concrete SASL mechanisms.

use fuckthunder_core::{Error, Result};
use hmac::{Hmac, Mac};
use md5::Md5;

use crate::{Credentials, Mechanism};

/// `PLAIN` (RFC 4616): `authzid NUL authcid NUL passwd`.
#[derive(Clone, Debug)]
pub struct Plain {
    authzid: String,
    user: String,
    password: String,
    done: bool,
}

impl Plain {
    /// Builds a `PLAIN` mechanism.
    #[must_use]
    pub fn new(credentials: &Credentials) -> Self {
        Self {
            authzid: credentials.authzid.clone().unwrap_or_default(),
            user: credentials.user.clone(),
            password: credentials.password.clone().unwrap_or_default(),
            done: false,
        }
    }
}

impl Mechanism for Plain {
    fn name(&self) -> &str {
        "PLAIN"
    }

    fn initial_response(&mut self) -> Option<Vec<u8>> {
        self.done = true;
        let mut out =
            Vec::with_capacity(self.authzid.len() + self.user.len() + self.password.len() + 2);
        out.extend_from_slice(self.authzid.as_bytes());
        out.push(0);
        out.extend_from_slice(self.user.as_bytes());
        out.push(0);
        out.extend_from_slice(self.password.as_bytes());
        Some(out)
    }

    fn step(&mut self, _challenge: &[u8]) -> Result<Option<Vec<u8>>> {
        Err(Error::Auth("PLAIN does not accept challenges".into()))
    }

    fn is_complete(&self) -> bool {
        self.done
    }
}

/// `LOGIN`: server prompts for username, then password.
#[derive(Clone, Debug)]
pub struct Login {
    user: String,
    password: String,
    stage: u8,
}

impl Login {
    /// Builds a `LOGIN` mechanism.
    #[must_use]
    pub fn new(credentials: &Credentials) -> Self {
        Self {
            user: credentials.user.clone(),
            password: credentials.password.clone().unwrap_or_default(),
            stage: 0,
        }
    }
}

impl Mechanism for Login {
    fn name(&self) -> &str {
        "LOGIN"
    }

    fn initial_response(&mut self) -> Option<Vec<u8>> {
        None
    }

    fn step(&mut self, _challenge: &[u8]) -> Result<Option<Vec<u8>>> {
        match self.stage {
            0 => {
                self.stage = 1;
                Ok(Some(self.user.as_bytes().to_vec()))
            }
            1 => {
                self.stage = 2;
                Ok(Some(self.password.as_bytes().to_vec()))
            }
            _ => Err(Error::Auth("LOGIN challenge after completion".into())),
        }
    }

    fn is_complete(&self) -> bool {
        self.stage >= 2
    }
}

/// `CRAM-MD5` (RFC 2195): HMAC-MD5 of the challenge keyed by the password.
#[derive(Clone, Debug)]
pub struct CramMd5 {
    user: String,
    password: String,
    done: bool,
}

impl CramMd5 {
    /// Builds a `CRAM-MD5` mechanism.
    ///
    /// # Panics
    ///
    /// Panics only if the credentials lack a password; use
    /// [`crate::mechanism`] for a fallible constructor.
    #[must_use]
    pub fn new(credentials: &Credentials) -> Self {
        Self {
            user: credentials.user.clone(),
            password: credentials.password.clone().unwrap_or_default(),
            done: false,
        }
    }
}

impl Mechanism for CramMd5 {
    fn name(&self) -> &str {
        "CRAM-MD5"
    }

    fn initial_response(&mut self) -> Option<Vec<u8>> {
        None
    }

    fn step(&mut self, challenge: &[u8]) -> Result<Option<Vec<u8>>> {
        if self.done {
            return Err(Error::Auth("CRAM-MD5 challenge after completion".into()));
        }
        let mut mac = Hmac::<Md5>::new_from_slice(self.password.as_bytes())
            .map_err(|e| Error::Auth(format!("invalid CRAM-MD5 key: {e}")))?;
        mac.update(challenge);
        let digest = mac.finalize().into_bytes();
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        self.done = true;
        Ok(Some(format!("{} {}", self.user, hex).into_bytes()))
    }

    fn is_complete(&self) -> bool {
        self.done
    }
}

/// `XOAUTH2`: `user=...\x01auth=Bearer <token>\x01\x01`.
#[derive(Clone, Debug)]
pub struct XOAuth2 {
    user: String,
    token: String,
    done: bool,
}

impl XOAuth2 {
    /// Builds an `XOAUTH2` mechanism.
    ///
    /// # Panics
    ///
    /// Panics only when no token is present; prefer [`crate::mechanism`].
    #[must_use]
    pub fn new(credentials: &Credentials) -> Self {
        Self {
            user: credentials.user.clone(),
            token: credentials.token.clone().unwrap_or_default(),
            done: false,
        }
    }

    /// Builds the initial response directly from parts.
    #[must_use]
    pub fn response(user: &str, token: &str) -> Vec<u8> {
        format!("user={user}\x01auth=Bearer {token}\x01\x01").into_bytes()
    }
}

impl Mechanism for XOAuth2 {
    fn name(&self) -> &str {
        "XOAUTH2"
    }

    fn initial_response(&mut self) -> Option<Vec<u8>> {
        self.done = true;
        Some(Self::response(&self.user, &self.token))
    }

    fn step(&mut self, challenge: &[u8]) -> Result<Option<Vec<u8>>> {
        // Servers send a JSON error challenge; the client must answer with an
        // empty line to complete the failed exchange.
        if challenge.is_empty() {
            return Ok(Some(Vec::new()));
        }
        Err(Error::Auth(format!(
            "XOAUTH2 rejected: {}",
            String::from_utf8_lossy(challenge)
        )))
    }

    fn is_complete(&self) -> bool {
        self.done
    }
}

/// `OAUTHBEARER` (RFC 7628).
#[derive(Clone, Debug)]
pub struct OAuthBearer {
    user: String,
    token: String,
    host: String,
    port: u16,
    done: bool,
}

impl OAuthBearer {
    /// Builds an `OAUTHBEARER` mechanism.
    ///
    /// # Panics
    ///
    /// Panics only when no token is present; prefer [`crate::mechanism`].
    #[must_use]
    pub fn new(credentials: &Credentials) -> Self {
        Self {
            user: credentials.user.clone(),
            token: credentials.token.clone().unwrap_or_default(),
            host: credentials.host.clone().unwrap_or_default(),
            port: credentials.port.unwrap_or(993),
            done: false,
        }
    }
}

impl Mechanism for OAuthBearer {
    fn name(&self) -> &str {
        "OAUTHBEARER"
    }

    fn initial_response(&mut self) -> Option<Vec<u8>> {
        self.done = true;
        let host = if self.host.is_empty() {
            String::new()
        } else {
            format!("\x01host={}\x01port={}", self.host, self.port)
        };
        Some(
            format!(
                "n,a={user},\x01{host}\x01auth=Bearer {token}\x01\x01",
                user = self.user,
                token = self.token
            )
            .into_bytes(),
        )
    }

    fn step(&mut self, challenge: &[u8]) -> Result<Option<Vec<u8>>> {
        if challenge.is_empty() {
            return Ok(Some(Vec::new()));
        }
        Err(Error::Auth(format!(
            "OAUTHBEARER rejected: {}",
            String::from_utf8_lossy(challenge)
        )))
    }

    fn is_complete(&self) -> bool {
        self.done
    }
}

/// `ANONYMOUS` (RFC 4505).
#[derive(Clone, Debug)]
pub struct Anonymous {
    trace: String,
    done: bool,
}

impl Anonymous {
    /// Builds an `ANONYMOUS` mechanism using the user field as trace info.
    #[must_use]
    pub fn new(credentials: &Credentials) -> Self {
        Self {
            trace: credentials.user.clone(),
            done: false,
        }
    }
}

impl Mechanism for Anonymous {
    fn name(&self) -> &str {
        "ANONYMOUS"
    }

    fn initial_response(&mut self) -> Option<Vec<u8>> {
        self.done = true;
        Some(self.trace.as_bytes().to_vec())
    }

    fn step(&mut self, _challenge: &[u8]) -> Result<Option<Vec<u8>>> {
        Err(Error::Auth("ANONYMOUS does not accept challenges".into()))
    }

    fn is_complete(&self) -> bool {
        self.done
    }
}

/// `EXTERNAL` (RFC 4422): authenticate using an out-of-band identity.
#[derive(Clone, Debug)]
pub struct External {
    authzid: String,
    done: bool,
}

impl External {
    /// Builds an `EXTERNAL` mechanism.
    #[must_use]
    pub fn new(credentials: &Credentials) -> Self {
        Self {
            authzid: credentials.authzid.clone().unwrap_or_default(),
            done: false,
        }
    }
}

impl Mechanism for External {
    fn name(&self) -> &str {
        "EXTERNAL"
    }

    fn initial_response(&mut self) -> Option<Vec<u8>> {
        self.done = true;
        Some(self.authzid.as_bytes().to_vec())
    }

    fn step(&mut self, _challenge: &[u8]) -> Result<Option<Vec<u8>>> {
        Err(Error::Auth("EXTERNAL does not accept challenges".into()))
    }

    fn is_complete(&self) -> bool {
        self.done
    }
}

/// Returns the strongest mechanism both sides support.
#[must_use]
pub fn select_best(advertised: &[String]) -> Option<&'static str> {
    const PREFERENCE: [&str; 5] = ["OAUTHBEARER", "XOAUTH2", "CRAM-MD5", "PLAIN", "LOGIN"];
    PREFERENCE.into_iter().find(|candidate| {
        advertised
            .iter()
            .any(|mechanism| mechanism.eq_ignore_ascii_case(candidate))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode_response;

    #[test]
    fn plain_rfc4616_vector() {
        let mut mech = Plain::new(&Credentials::password("brian", "secret"));
        assert_eq!(
            mech.initial_response().unwrap(),
            b"\0brian\0secret".to_vec()
        );
        assert!(mech.is_complete());
    }

    #[test]
    fn plain_with_authzid() {
        let creds = Credentials {
            authzid: Some("admin".into()),
            ..Credentials::password("brian", "secret")
        };
        let mut mech = Plain::new(&creds);
        assert_eq!(mech.initial_response().unwrap(), b"admin\0brian\0secret");
    }

    #[test]
    fn login_alternates_user_and_password() {
        let mut mech = Login::new(&Credentials::password("alice", "hunter2"));
        assert!(mech.initial_response().is_none());
        assert_eq!(mech.step(b"Username:").unwrap().unwrap(), b"alice");
        assert!(!mech.is_complete());
        assert_eq!(mech.step(b"Password:").unwrap().unwrap(), b"hunter2");
        assert!(mech.is_complete());
        assert!(mech.step(b"again").is_err());
    }

    #[test]
    fn cram_md5_rfc2195_vector() {
        let mut mech = CramMd5::new(&Credentials::password("tim", "tanstaaftanstaaf"));
        let challenge = b"<1896.697170952@postoffice.reston.mci.net>";
        let response = mech.step(challenge).unwrap().unwrap();
        assert_eq!(
            String::from_utf8(response).unwrap(),
            "tim b913a602c7eda7a495b4e6e7334d3890"
        );
        assert!(mech.is_complete());
        assert!(mech.step(challenge).is_err());
    }

    #[test]
    fn xoauth2_response_layout() {
        let mut mech = XOAuth2::new(&Credentials::bearer("alice@example.org", "tok123"));
        let response = mech.initial_response().unwrap();
        assert_eq!(
            response,
            b"user=alice@example.org\x01auth=Bearer tok123\x01\x01"
        );
        assert_eq!(
            encode_response(&response),
            "dXNlcj1hbGljZUBleGFtcGxlLm9yZwFhdXRoPUJlYXJlciB0b2sxMjMBAQ=="
        );
        assert!(mech.step(b"{}").is_err());
        assert_eq!(mech.step(b"").unwrap().unwrap(), b"");
    }

    #[test]
    fn oauthbearer_layout() {
        let creds = Credentials {
            host: Some("imap.example.org".into()),
            port: Some(993),
            ..Credentials::bearer("alice", "tok")
        };
        let mut mech = OAuthBearer::new(&creds);
        let response = String::from_utf8(mech.initial_response().unwrap()).unwrap();
        assert!(response.starts_with("n,a=alice,\x01"));
        assert!(response.contains("host=imap.example.org"));
        assert!(response.contains("auth=Bearer tok"));
        assert!(response.ends_with("\x01\x01"));
    }

    #[test]
    fn anonymous_and_external() {
        let mut anonymous = Anonymous::new(&Credentials::default());
        assert_eq!(anonymous.initial_response().unwrap(), b"");
        let mut external = External::new(&Credentials {
            authzid: Some("alice".into()),
            ..Credentials::default()
        });
        assert_eq!(external.initial_response().unwrap(), b"alice");
        assert!(external.step(b"").is_err());
    }

    #[test]
    fn best_mechanism_selection() {
        assert_eq!(
            select_best(&["LOGIN".into(), "PLAIN".into(), "XOAUTH2".into()]),
            Some("XOAUTH2")
        );
        assert_eq!(select_best(&["LOGIN".into()]), Some("LOGIN"));
        assert_eq!(select_best(&["GSSAPI".into()]), None);
    }
}
