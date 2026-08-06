//! Shared transport for the App Store Connect API.
//!
//! Everything the domain crates need and nothing they disagree about: the
//! ES256 JWT auth, the HTTP client that caches those tokens, the JSON:API
//! envelope generics, and the error type.
//!
//! The domain crates ([`smbcloud-ascapi-aso`] for App Metadata,
//! [`smbcloud-ascapi-signing`] for certificates) add their resources as
//! extension traits on [`Client`], since Rust only allows inherent impls
//! in the crate that defines a type. Import a domain crate's `prelude` to
//! call its methods on a client.
//!
//! [`smbcloud-ascapi-aso`]: https://docs.rs/smbcloud-ascapi-aso
//! [`smbcloud-ascapi-signing`]: https://docs.rs/smbcloud-ascapi-signing

pub mod auth;
pub mod client;
pub mod error;
pub mod jsonapi;

pub use auth::ApiKey;
pub use client::Client;
pub use error::{Error, Result};
