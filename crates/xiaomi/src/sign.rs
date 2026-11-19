//! The `Authorization` signature from Xiaomi's "GetApps Interface User
//! Guide" and "Generic GetApps interface request header" pages.
//!
//! The canonical string is the method, then the Base64 MD5 of the body when
//! there is one, then `AppId:…PkgName:…Timestamp:…Nonce:…`, then the request
//! path without the domain, all concatenated with no separators. The
//! signature is the standard (not URL-safe) Base64 of its HMAC-SHA1, keyed
//! with the App Secret.
//!
//! The docs spell the timestamp label both `Timestamp:` and `TimeStamp:`.
//! Both assembled examples use `Timestamp:`, so that is what is signed. If
//! GetApps answers 1002 (signature mismatch), check this first.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use hmac::{Hmac, Mac};
use sha1::Sha1;

use crate::auth::Credentials;

/// Base64 of the MD5 of the exact body bytes sent.
pub fn body_md5(body: &[u8]) -> String {
    STANDARD.encode(md5::compute(body).0)
}

/// The string the HMAC is computed over.
pub fn canonical_string(
    method: &str,
    body: Option<&[u8]>,
    app_id: &str,
    package: &str,
    timestamp: &str,
    nonce: &str,
    path: &str,
) -> String {
    let md5 = body.map(body_md5).unwrap_or_default();
    format!(
        "{}{md5}AppId:{app_id}PkgName:{package}Timestamp:{timestamp}Nonce:{nonce}{path}",
        method.to_ascii_uppercase()
    )
}

/// Base64 (standard alphabet) of HMAC-SHA1(secret, canonical).
pub fn signature(secret: &str, canonical: &str) -> String {
    let mut mac =
        Hmac::<Sha1>::new_from_slice(secret.as_bytes()).expect("HMAC accepts a key of any length");
    mac.update(canonical.as_bytes());
    STANDARD.encode(mac.finalize().into_bytes())
}

/// A fresh nonce: 32 lowercase hex characters, the shape of a UUID without
/// dashes. GetApps refuses a nonce it has seen (3002).
pub fn new_nonce() -> String {
    use rand::Rng;
    let bytes: [u8; 16] = rand::thread_rng().gen();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The full header set for one request. `Debug` and [`SignedHeaders::pairs`]
/// carry the signature, never the secret.
#[derive(Debug, Clone)]
pub struct SignedHeaders {
    pub canonical: String,
    pub authorization: String,
    pub app_id: String,
    pub package: String,
    pub email: String,
    pub developer_id: String,
    pub user_id: String,
    pub nonce: String,
    pub timestamp: String,
}

impl SignedHeaders {
    pub fn new(
        credentials: &Credentials,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
        timestamp: u64,
        nonce: String,
    ) -> Self {
        let timestamp = timestamp.to_string();
        let canonical = canonical_string(
            method,
            body,
            &credentials.app_id,
            &credentials.package,
            &timestamp,
            &nonce,
            path,
        );
        Self {
            authorization: signature(credentials.app_secret(), &canonical),
            canonical,
            app_id: credentials.app_id.clone(),
            package: credentials.package.clone(),
            email: credentials.email.clone(),
            developer_id: credentials.developer_id.clone(),
            user_id: credentials.user_id.clone(),
            nonce,
            timestamp,
        }
    }

    /// Header names exactly as GetApps wants them (initial capitals).
    pub fn pairs(&self) -> [(&'static str, &str); 8] {
        [
            ("Authorization", &self.authorization),
            ("AppId", &self.app_id),
            ("PkgName", &self.package),
            ("Email", &self.email),
            ("DeveloperId", &self.developer_id),
            ("UserId", &self.user_id),
            ("Nonce", &self.nonce),
            ("Timestamp", &self.timestamp),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Vectors computed independently with Python's hmac, hashlib and base64.
    // Xiaomi's own sample cannot be reproduced: its body was edited after the
    // MD5 was taken.
    const BODY: &str =
        r#"{"pkgs":"com.example.app","startTimestamp":1701084799,"endTimestamp":1702684799}"#;
    const NONCE: &str = "8420674727e94e7cabff27ac66db5c5d";

    #[test]
    fn body_md5_is_base64_of_the_raw_digest() {
        assert_eq!(body_md5(BODY.as_bytes()), "FQgbNE045u5FQeSePz8Rbw==");
    }

    #[test]
    fn post_with_body() {
        let canonical = canonical_string(
            "post",
            Some(BODY.as_bytes()),
            "2882303761521427725",
            "com.example.app",
            "1703134938",
            NONCE,
            "/third-api/order/v1/query",
        );
        assert_eq!(
            canonical,
            "POSTFQgbNE045u5FQeSePz8Rbw==AppId:2882303761521427725PkgName:com.example.app\
             Timestamp:1703134938Nonce:8420674727e94e7cabff27ac66db5c5d/third-api/order/v1/query"
        );
        assert_eq!(
            signature("123456", &canonical),
            "chhSRb8ikmOSRWr60iEQPDziodI="
        );
    }

    #[test]
    fn get_without_body() {
        let credentials = Credentials::new(
            "2882303761521427725",
            "123456",
            "com.example.app",
            "dev@example.com",
            "1080405",
            "2410896325",
        );
        let headers = SignedHeaders::new(
            &credentials,
            "GET",
            "/third-api/apk/v1/getUploadUrl",
            None,
            1703134938,
            NONCE.to_string(),
        );
        assert!(headers.canonical.starts_with("GETAppId:"));
        assert_eq!(headers.authorization, "dC0JlSQeQguwnve9J67o82nBPFQ=");
        assert!(!format!("{headers:?}").contains("123456"));
    }

    #[test]
    fn nonce_is_32_hex_and_fresh() {
        let a = new_nonce();
        assert_eq!(a.len(), 32);
        assert!(a.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(a, new_nonce());
    }
}
