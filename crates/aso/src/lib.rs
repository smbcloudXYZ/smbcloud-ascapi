//! App Store Connect's
//! [App Metadata](https://developer.apple.com/documentation/appstoreconnectapi/app-metadata)
//! resources: apps, app infos and their localizations, app store versions
//! and their localizations, builds, bundle IDs, and screenshots.
//!
//! Everything here is an extension trait on
//! [`Client`](smbcloud_ascapi_core::Client), because that type belongs to
//! the core crate and Rust only allows inherent impls in the crate that
//! defines a type. Import [`prelude`] to get all of them at once.
//!
//! ```no_run
//! use smbcloud_ascapi_core::{ApiKey, Client};
//! use smbcloud_ascapi_aso::prelude::*;
//!
//! # async fn example() -> smbcloud_ascapi_core::Result<()> {
//! let api_key = ApiKey::from_p8_file("L84N624YQH", "b4e8d369-…", "AuthKey.p8")?;
//! let client = Client::new(api_key);
//! let apps = client.list_apps(Some("xyz.smbcloud.mailx")).await?;
//! # Ok(())
//! # }
//! ```

pub mod app;
pub mod app_info;
pub mod app_info_localization;
pub mod app_screenshot;
pub mod app_screenshot_set;
pub mod app_store_version;
pub mod app_store_version_localization;
pub mod build;
pub mod bundle_id;

/// Every extension trait in this crate, for one glob import.
pub mod prelude {
    pub use crate::app::AppsApi;
    pub use crate::app_info::AppInfosApi;
    pub use crate::app_info_localization::AppInfoLocalizationsApi;
    pub use crate::app_screenshot::AppScreenshotsApi;
    pub use crate::app_screenshot_set::AppScreenshotSetsApi;
    pub use crate::app_store_version::AppStoreVersionsApi;
    pub use crate::app_store_version_localization::AppStoreVersionLocalizationsApi;
    pub use crate::build::BuildsApi;
    pub use crate::bundle_id::BundleIdsApi;
}
