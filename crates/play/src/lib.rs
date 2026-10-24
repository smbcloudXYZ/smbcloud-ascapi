//! Google Play Developer API (Edits v3) for store-listing work.
//!
//! Localized listings, listing images, and release notes on a track release.
//! Authentication is a service-account key exchanged for an OAuth2 token
//! (RS256 JWT), cached until shortly before it expires. Every read and write
//! happens inside an edit: [`PlayClient::insert_edit`] opens one,
//! [`PlayClient::commit_edit`] publishes it, and [`PlayClient::delete_edit`]
//! discards it.

pub mod auth;
pub mod client;
pub mod error;
pub mod image;
pub mod listing;
pub mod track;

pub use auth::ServiceAccount;
pub use client::{Edit, PlayClient};
pub use error::{Error, Result};
pub use image::{png_dimensions, Image, ImageType};
pub use listing::{
    Listing, ListingFields, FULL_DESCRIPTION_MAX_LEN, RELEASE_NOTES_MAX_LEN,
    SHORT_DESCRIPTION_MAX_LEN, TITLE_MAX_LEN,
};
pub use track::{LocalizedText, Track, TrackRelease};
