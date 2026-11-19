//! Xiaomi GetApps developer interface (`/third-api/…` on
//! global.developer.mi.com).
//!
//! Every request carries the same header set (`AppId`, `PkgName`, `Email`,
//! `DeveloperId`, `UserId`, `Nonce`, `Timestamp`) and an `Authorization`
//! header: an HMAC-SHA1 over a canonical string, keyed with the App Secret
//! from the app's page in the console. See [`sign`] for the string.
//!
//! The public docs describe two endpoints: the order query, which needs
//! allowlisting by Xiaomi, and the APK upload address. The upload and submit
//! steps that follow are documented on a private Xiaomi wiki, so they are not
//! here yet.

pub mod auth;
pub mod client;
pub mod error;
pub mod sign;

pub use auth::{parse_dotenv, Credentials};
pub use client::{
    OrderQuery, Prepared, XiaomiClient, API_ROOT, ORDER_QUERY_MAX_SECS, ORDER_QUERY_PATH,
    UPLOAD_URL_PATH,
};
pub use error::{Error, Result};
pub use sign::{body_md5, canonical_string, signature, SignedHeaders};
