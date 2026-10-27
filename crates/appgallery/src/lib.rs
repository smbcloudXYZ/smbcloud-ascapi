//! Huawei AppGallery Connect API.
//!
//! Service Account mode: the credential file's `private_key` signs a PS256 JWT
//! that is sent as the bearer token (see [`ServiceAccount::assertion`]).
//! [`AppGalleryClient`] holds the account and the HTTP client the Publishing
//! API calls are built on.

pub mod auth;
pub mod client;
pub mod error;

pub use auth::ServiceAccount;
pub use client::AppGalleryClient;
pub use error::{Error, Result};
