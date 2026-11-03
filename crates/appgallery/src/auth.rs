use std::path::Path;

use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// The `aud` Huawei requires in the JWT.
pub const TOKEN_URI: &str = "https://oauth-login.cloud.huawei.com/oauth2/v3/token";
/// Huawei's JWT lifetime: `exp` is 3600 seconds after `iat`.
const JWT_LIFETIME_SECS: u64 = 3600;

/// The fields of an AppGallery Connect Service Account credential file
/// (`*private.json`) this crate uses.
#[derive(Clone, Deserialize)]
pub struct ServiceAccount {
    pub key_id: String,
    private_key: String,
    pub sub_account: String,
}

// Hand-written so `{:?}` can never print the private key.
impl std::fmt::Debug for ServiceAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceAccount")
            .field("key_id", &self.key_id)
            .field("sub_account", &self.sub_account)
            .field("private_key", &"<redacted>")
            .finish()
    }
}

impl ServiceAccount {
    pub fn from_json(json: &str) -> Result<Self> {
        let account: Self = serde_json::from_str(json).map_err(|_| Error::ServiceAccountInvalid)?;
        if account.key_id.is_empty()
            || account.private_key.is_empty()
            || account.sub_account.is_empty()
        {
            return Err(Error::ServiceAccountInvalid);
        }
        Ok(account)
    }

    pub fn from_json_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let json = std::fs::read_to_string(path).map_err(|source| Error::ServiceAccountRead {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_json(&json)
    }

    /// Sign the PS256 JWT that serves as the bearer token. `now_secs` is Unix
    /// time in seconds; Huawei needs the local clock to be accurate.
    pub fn assertion(&self, now_secs: u64) -> Result<String> {
        let claims = Claims::new(&self.sub_account, now_secs);
        let mut header = Header::new(Algorithm::PS256);
        header.typ = Some("JWT".to_string());
        header.kid = Some(self.key_id.clone());
        let key = EncodingKey::from_rsa_pem(self.private_key.as_bytes())?;
        Ok(jsonwebtoken::encode(&header, &claims, &key)?)
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Claims {
    pub aud: String,
    pub iss: String,
    pub exp: u64,
    pub iat: u64,
}

impl Claims {
    pub(crate) fn new(sub_account: &str, now_secs: u64) -> Self {
        Self {
            aud: TOKEN_URI.to_string(),
            iss: sub_account.to_string(),
            exp: now_secs + JWT_LIFETIME_SECS,
            iat: now_secs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{decode, decode_header, DecodingKey, Validation};
    use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};
    use rsa::RsaPrivateKey;

    fn json_with_key(pem: &str) -> String {
        serde_json::json!({
            "key_id": "kid-1",
            "private_key": pem,
            "sub_account": "1099",
        })
        .to_string()
    }

    #[test]
    fn claims_carry_audience_issuer_and_one_hour_lifetime() {
        let claims = Claims::new("1099", 1000);
        assert_eq!(
            claims.aud,
            "https://oauth-login.cloud.huawei.com/oauth2/v3/token"
        );
        assert_eq!(claims.iss, "1099");
        assert_eq!(claims.iat, 1000);
        assert_eq!(claims.exp, 4600);
    }

    #[test]
    fn the_jwt_is_ps256_with_kid_and_verifies_against_the_public_key() {
        let mut rng = rand::thread_rng();
        let key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let private_pem = key.to_pkcs8_pem(LineEnding::LF).unwrap();
        let public_pem = key
            .to_public_key()
            .to_public_key_pem(LineEnding::LF)
            .unwrap();

        let account = ServiceAccount::from_json(&json_with_key(&private_pem)).unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let jwt = account.assertion(now).unwrap();

        let header = decode_header(&jwt).unwrap();
        assert_eq!(header.alg, Algorithm::PS256);
        assert_eq!(header.kid.as_deref(), Some("kid-1"));
        assert_eq!(header.typ.as_deref(), Some("JWT"));

        let mut validation = Validation::new(Algorithm::PS256);
        validation.set_audience(&[TOKEN_URI]);
        let decoded = decode::<Claims>(
            &jwt,
            &DecodingKey::from_rsa_pem(public_pem.as_bytes()).unwrap(),
            &validation,
        )
        .unwrap();
        assert_eq!(decoded.claims.iss, "1099");
    }

    #[test]
    fn a_key_file_missing_the_key_is_rejected_without_echoing_it() {
        let err = ServiceAccount::from_json(&json_with_key(""))
            .unwrap_err()
            .to_string();
        assert!(!err.contains("kid-1"));
        assert!(ServiceAccount::from_json("not json").is_err());
    }

    #[test]
    fn debug_output_never_contains_the_private_key() {
        let account = ServiceAccount::from_json(&json_with_key("SECRET")).unwrap();
        let shown = format!("{account:?}");
        assert!(!shown.contains("SECRET"));
        assert!(shown.contains("kid-1"));
    }

    #[test]
    fn a_malformed_pem_fails_to_sign_instead_of_panicking() {
        let account = ServiceAccount::from_json(&json_with_key("nope")).unwrap();
        assert!(account.assertion(0).is_err());
    }
}
