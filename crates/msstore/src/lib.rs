//! Microsoft Store submission API v1.0 for store-listing work.
//!
//! Reads an MSIX app and its submissions, and edits the listing text on a
//! submission: title, short description, description, release notes, features
//! and search terms. Authentication is Entra client credentials, cached until
//! shortly before the token expires. Submissions travel as
//! [`serde_json::Value`] so only the listing keys are touched and everything
//! else round-trips. Committing a submission is not implemented.

pub mod auth;
pub mod client;
pub mod error;
pub mod listing;

pub use auth::{parse_dotenv, Credentials};
pub use client::MsStoreClient;
pub use error::{Error, Result};
pub use listing::{
    ListingFields, DESCRIPTION_MAX_LEN, FEATURES_MAX_COUNT, FEATURE_MAX_LEN, KEYWORDS_MAX_COUNT,
    KEYWORDS_MAX_WORDS, KEYWORD_MAX_LEN, RELEASE_NOTES_MAX_LEN, SHORT_DESCRIPTION_MAX_LEN,
    TITLE_MAX_LEN,
};
