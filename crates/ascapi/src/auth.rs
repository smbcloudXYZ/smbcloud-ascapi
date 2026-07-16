use crate::error::{Error, Result};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::Serialize;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const AUDIENCE: &str = "appstoreconnect-v1";
// Apple rejects tokens whose `exp` is more than 20 minutes past `iat`. See
// https://developer.apple.com/documentation/appstoreconnectapi/generating-tokens-for-api-requests
const MAX_TOKEN_LIFETIME_SECS: u64 = 20 * 60;

#[derive(Debug, Clone, Serialize)]
struct Claims {
    iss: String,
    iat: u64,
    exp: u64,
    aud: String,
}

/// An App Store Connect API key: the (key ID, issuer ID, private key)
/// triple from App Store Connect → Users and Access → Integrations → App
/// Store Connect API. Signs the short-lived ES256 JWTs the API requires as
/// a bearer token on every request.
#[derive(Clone)]
pub struct ApiKey {
    key_id: String,
    issuer_id: String,
    encoding_key: EncodingKey,
    lifetime_secs: u64,
}

impl ApiKey {
    /// Load a key from a PEM-encoded `.p8` private key file — the file App
    /// Store Connect lets you download once, at key-creation time.
    pub fn from_p8_file(
        key_id: impl Into<String>,
        issuer_id: impl Into<String>,
        p8_path: impl AsRef<Path>,
    ) -> Result<Self> {
        let path = p8_path.as_ref();
        let pem = std::fs::read_to_string(path).map_err(|source| Error::PrivateKeyRead {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_p8_pem(key_id, issuer_id, &pem)
    }

    /// Load a key from an already-in-memory PEM string.
    pub fn from_p8_pem(
        key_id: impl Into<String>,
        issuer_id: impl Into<String>,
        pem: &str,
    ) -> Result<Self> {
        let encoding_key = EncodingKey::from_ec_pem(pem.as_bytes())?;
        Ok(Self {
            key_id: key_id.into(),
            issuer_id: issuer_id.into(),
            encoding_key,
            lifetime_secs: MAX_TOKEN_LIFETIME_SECS,
        })
    }

    /// Override the token lifetime in seconds. Values above Apple's 20
    /// minute ceiling are clamped down to it.
    pub fn with_lifetime_secs(mut self, secs: u64) -> Self {
        self.lifetime_secs = secs.min(MAX_TOKEN_LIFETIME_SECS);
        self
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    pub fn lifetime_secs(&self) -> u64 {
        self.lifetime_secs
    }

    /// Mint a fresh bearer token. `Client` caches and reuses the result of
    /// this until shortly before it expires rather than calling it per
    /// request.
    pub fn token(&self) -> Result<String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default() // a clock before 1970 isn't a real failure mode worth propagating
            .as_secs();

        let claims = Claims {
            iss: self.issuer_id.clone(),
            iat: now,
            exp: now + self.lifetime_secs,
            aud: AUDIENCE.to_string(),
        };

        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(self.key_id.clone());

        Ok(encode(&header, &claims, &self.encoding_key)?)
    }
}
