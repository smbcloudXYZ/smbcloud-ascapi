//! Huawei AppGallery Connect API.
//!
//! Service Account mode: the credential file's `private_key` signs a PS256 JWT
//! that is sent as the bearer token (see [`ServiceAccount::assertion`]).
//! [`AppGalleryClient`] holds the account and the HTTP client the Publishing
//! API calls are built on. [`publish`] has the Publishing API calls for
//! listing text: resolve an app id from a package name, read app info, and
//! update per-language text and release notes.

pub mod auth;
pub mod client;
pub mod error;
pub mod publish;
pub mod upload;

pub use auth::ServiceAccount;
pub use client::AppGalleryClient;
pub use error::{Error, Result};
pub use publish::{
    AppIdPair, AppInfo, AppInfoFields, AppInfoResponse, LanguageFields, LanguageInfo, RetCode,
    APP_DESC_MAX_LEN, APP_NAME_MAX_LEN, BRIEF_INFO_MAX_LEN, LANG_MAX_LEN, NEW_FEATURES_MAX_LEN,
};
pub use upload::{FileType, ImageOrientation, UploadUrl};
