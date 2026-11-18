use std::path::Path;

use serde::Deserialize;

use crate::error::{Error, Result};

/// Login with Amazon token endpoint.
pub const TOKEN_URL: &str = "https://api.amazon.com/auth/o2/token";
/// The scope the App Submission API asks for.
pub const SCOPE: &str = "appstore::apps:readwrite";

/// The Security Profile credentials (`{"client_id": …, "client_secret": …}`).
#[derive(Clone, Deserialize)]
pub struct Credentials {
    pub client_id: String,
    client_secret: String,
}

// Hand-written so `{:?}` can never print the secret.
impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .finish()
    }
}

impl Credentials {
    pub fn from_json(json: &str) -> Result<Self> {
        let credentials: Self =
            serde_json::from_str(json).map_err(|_| Error::CredentialsInvalid)?;
        if credentials.client_id.is_empty() || credentials.client_secret.is_empty() {
            return Err(Error::CredentialsInvalid);
        }
        Ok(credentials)
    }

    pub fn from_json_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let json = std::fs::read_to_string(path).map_err(|source| Error::CredentialsRead {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_json(&json)
    }

    /// The form fields of the `client_credentials` token request.
    pub fn token_form(&self) -> [(&'static str, &str); 4] {
        [
            ("grant_type", "client_credentials"),
            ("client_id", &self.client_id),
            ("client_secret", &self.client_secret),
            ("scope", SCOPE),
        ]
    }
}

#[derive(Deserialize)]
pub(crate) struct TokenResponse {
    pub access_token: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_request_is_a_client_credentials_form_with_the_appstore_scope() {
        let c = Credentials::from_json(r#"{"client_id":"amzn1.id","client_secret":"s3"}"#).unwrap();
        let form = c.token_form();
        assert!(form.contains(&("grant_type", "client_credentials")));
        assert!(form.contains(&("scope", "appstore::apps:readwrite")));
        assert!(form.contains(&("client_id", "amzn1.id")));
        assert!(form.contains(&("client_secret", "s3")));
        assert_eq!(TOKEN_URL, "https://api.amazon.com/auth/o2/token");
    }

    #[test]
    fn debug_output_never_contains_the_secret() {
        let c = Credentials::from_json(r#"{"client_id":"id-1","client_secret":"SECRET"}"#).unwrap();
        let shown = format!("{c:?}");
        assert!(!shown.contains("SECRET"));
        assert!(shown.contains("id-1"));
    }

    #[test]
    fn invalid_credentials_are_rejected_without_echoing_them() {
        let err = Credentials::from_json(r#"{"client_id":"id-1","client_secret":""}"#)
            .unwrap_err()
            .to_string();
        assert!(!err.contains("id-1"));
        assert!(Credentials::from_json("nope").is_err());
    }
}
