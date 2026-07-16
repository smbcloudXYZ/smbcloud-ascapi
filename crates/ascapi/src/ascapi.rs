//! Embeddable Rust client (and CLI, in the sibling `smbcloud-ascapi-cli`
//! crate / `ascapi` binary) for the App Store Connect API's
//! [App Metadata](https://developer.apple.com/documentation/appstoreconnectapi/app-metadata)
//! resources: apps, app infos and their localizations, app store versions
//! and their localizations, and bundle IDs.
//!
//! ```no_run
//! use smbcloud_ascapi::{ApiKey, Client};
//! use smbcloud_ascapi::app_store_version::{AppStoreVersionCreateAttributes, Platform};
//!
//! # async fn example() -> smbcloud_ascapi::Result<()> {
//! let api_key = ApiKey::from_p8_file(
//!     "L84N624YQH",
//!     "b4e8d369-8b7d-4538-8435-643b73237575",
//!     "/Users/me/private_keys/AuthKey_L84N624YQH.p8",
//! )?;
//! let client = Client::new(api_key);
//!
//! let apps = client.list_apps(Some("ai.siti.Siti")).await?;
//! let app = &apps[0];
//!
//! client
//!     .create_app_store_version(
//!         &app.id,
//!         AppStoreVersionCreateAttributes {
//!             platform: Platform::VisionOs,
//!             version_string: "1.0.0".to_string(),
//!             copyright: None,
//!         },
//!     )
//!     .await?;
//! # Ok(())
//! # }
//! ```
//!
//! Not covered (yet): app preview (video) binary uploads, age rating
//! declarations, in-app purchases, TestFlight, and the rest of the wider
//! App Store Connect API surface outside "App Metadata".

pub mod app;
pub mod app_info;
pub mod app_info_localization;
pub mod app_screenshot;
pub mod app_screenshot_set;
pub mod app_store_version;
pub mod app_store_version_localization;
pub mod auth;
pub mod build;
pub mod bundle_id;
pub mod client;
pub mod error;
pub mod jsonapi;

pub use auth::ApiKey;
pub use client::Client;
pub use error::{Error, Result};
