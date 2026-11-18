//! Amazon Appstore App Submission API.
//!
//! [`Credentials`] are a Login with Amazon Security Profile; the client
//! exchanges them for a bearer token (`client_credentials`, scope
//! `appstore::apps:readwrite`). [`edit`] manages the open edit, [`apk`]
//! uploads and replaces packages, [`targeting`] reads and writes device
//! targeting. There is no commit: submitting stays in the Developer Console.

pub mod apk;
pub mod auth;
pub mod client;
pub mod edit;
pub mod error;
pub mod targeting;

pub use auth::Credentials;
pub use client::AmazonClient;
pub use edit::Edit;
pub use error::{Error, Result};
