//! App Store Connect's
//! [pricing](https://developer.apple.com/documentation/appstoreconnectapi/app-pricing)
//! resources: an app's price schedule, and the per-territory prices and
//! price points that hang off it.
//!
//! Everything here is an extension trait on
//! [`Client`](smbcloud_ascapi_core::Client), because that type belongs to
//! the core crate and Rust only allows inherent impls in the crate that
//! defines a type. Import [`prelude`] to get all of them at once.
//!
//! # Why this crate exists separately from `aso`
//!
//! Reading a price is the one App Metadata question that cannot be
//! answered by fetching a resource and reading its attributes. An
//! `appPrices` row carries no money at all — only `manual`, `startDate`
//! and `endDate`, plus relationships to an `appPricePoint` (which holds
//! `customerPrice` and `proceeds`) and a `territory` (which holds
//! `currency`). Three resources have to be joined before a price is
//! legible, and the join only works if the request asks for
//! `include=appPricePoint,territory` up front.
//!
//! That join is this crate's whole reason for being: callers get
//! [`app_price::TerritoryPrice`], one flat row per territory with the
//! money already attached, instead of a JSON:API document they have to
//! reassemble.
//!
//! ```no_run
//! use smbcloud_ascapi_core::{ApiKey, Client};
//! use smbcloud_ascapi_pricing::app_price::PriceKind;
//! use smbcloud_ascapi_pricing::prelude::*;
//!
//! # async fn example() -> smbcloud_ascapi_core::Result<()> {
//! let api_key = ApiKey::from_p8_file("L84N624YQH", "b4e8d369-…", "AuthKey.p8")?;
//! let client = Client::new(api_key);
//!
//! // What the developer actually set, in the base territory.
//! let manual = client
//!     .list_app_prices("6751143237", PriceKind::Manual, None)
//!     .await?;
//!
//! // What Apple converted that into for one storefront.
//! let indonesia = client
//!     .list_app_prices("6751143237", PriceKind::Automatic, Some("IDN"))
//!     .await?;
//! # Ok(())
//! # }
//! ```
//!
//! # A note on the opaque ids
//!
//! `appPrices` and `appPricePoints` ids are base64 of a small JSON object
//! (`{"s":appId,"t":territory,"p":pricePointId,…}`), so it is tempting to
//! decode one and skip the `include=` round trip. Don't. Apple documents
//! these ids as opaque, the shape has no compatibility guarantee, and the
//! decoded `p` is an internal price-point identifier rather than an
//! amount — the money still has to come from `appPricePoints`. This crate
//! always joins through `include=`.

pub mod app_price;
pub mod app_price_schedule;

/// Every extension trait in this crate, for one glob import.
pub mod prelude {
    pub use crate::app_price::AppPricesApi;
    pub use crate::app_price_schedule::AppPriceSchedulesApi;
}
