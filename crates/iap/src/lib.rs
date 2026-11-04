//! App Store Connect's
//! [in-app purchases](https://developer.apple.com/documentation/appstoreconnectapi/in-app-purchases):
//! the v2 `inAppPurchases` resource and what hangs off it.
//!
//! A product is not sellable until five things exist, each its own
//! resource:
//!
//! 1. the product itself ([`in_app_purchase`]): reference name, product id
//!    and type;
//! 2. at least one localization ([`localization`]): the display name and
//!    description the store sheet shows;
//! 3. a price schedule ([`price`]): one manual price in a base territory,
//!    from which Apple derives every other storefront;
//! 4. territory availability ([`availability`]);
//! 5. an App Review screenshot ([`review_screenshot`]).
//!
//! With those in place the product reaches `READY_TO_SUBMIT`. An app's
//! first in-app purchase is reviewed together with an app version, which is
//! done on the version page, so this crate has no submit call.
//!
//! Everything is an extension trait on
//! [`Client`](smbcloud_ascapi_core::Client); import [`prelude`].

pub mod availability;
pub mod in_app_purchase;
pub mod localization;
pub mod price;
pub mod review_screenshot;

/// Every extension trait in this crate, for one glob import.
pub mod prelude {
    pub use crate::availability::InAppPurchaseAvailabilityApi;
    pub use crate::in_app_purchase::InAppPurchasesApi;
    pub use crate::localization::InAppPurchaseLocalizationsApi;
    pub use crate::price::InAppPurchasePricesApi;
    pub use crate::review_screenshot::InAppPurchaseReviewScreenshotApi;
}
