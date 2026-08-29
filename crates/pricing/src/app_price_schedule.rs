//! `GET /v1/appPriceSchedules/{id}`.
//!
//! A price schedule is the container for everything an app costs. There is
//! no `POST` here: the schedule comes into existence with the app, and its
//! id **is** the app's id — `/v1/appPriceSchedules/6751143237` and
//! `/v1/apps/6751143237` describe the same product. Callers therefore pass
//! an app id, not a separately-looked-up schedule id.
//!
//! The resource carries no attributes at all. Its only payload is
//! relationships: the base territory, the manual prices, and the automatic
//! ones. This module reads the base territory — the storefront whose price
//! the developer set by hand, and from which Apple converts every other
//! storefront. Reading the prices themselves is [`crate::app_price`].

use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use smbcloud_ascapi_core::Client;
use smbcloud_ascapi_core::Result;

pub const RESOURCE_TYPE: &str = "appPriceSchedules";

/// An app's price schedule, flattened to the one fact it carries.
///
/// Not [`Resource`](smbcloud_ascapi_core::jsonapi::Resource), because that
/// type requires an `attributes` object and `appPriceSchedules` returns
/// none — the resource is relationships and links only.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPriceSchedule {
    /// The schedule id, which is also the app's App Store Connect id.
    pub id: String,
    /// Territory code whose price was set manually and from which Apple
    /// converts the rest, e.g. `USA`. `None` if Apple did not return the
    /// relationship.
    pub base_territory: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ScheduleDocument {
    data: ScheduleResource,
}

#[derive(Debug, Clone, Deserialize)]
struct ScheduleResource {
    id: String,
    #[serde(default)]
    relationships: ScheduleRelationships,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScheduleRelationships {
    #[serde(default)]
    base_territory: Option<crate::app_price::ToOne>,
}

/// App price schedules: the container an app's prices hang off.
///
/// An extension trait rather than inherent methods, because `Client`
/// lives in the core crate and Rust only allows inherent impls in the
/// crate that defines the type. Import it, or the crate's `prelude`,
/// to call these on a `Client`.
#[async_trait]
pub trait AppPriceSchedulesApi {
    /// `GET /v1/appPriceSchedules/{app_id}` — the app's base territory.
    ///
    /// Takes the **app** id; schedule and app share one id.
    async fn get_app_price_schedule(&self, app_id: &str) -> Result<AppPriceSchedule>;
}

#[async_trait]
impl AppPriceSchedulesApi for Client {
    async fn get_app_price_schedule(&self, app_id: &str) -> Result<AppPriceSchedule> {
        let path = format!("/v1/appPriceSchedules/{app_id}");
        let doc: ScheduleDocument = self
            .request(
                Method::GET,
                &path,
                &[("include", "baseTerritory")],
                None::<&()>,
            )
            .await?;
        Ok(AppPriceSchedule {
            id: doc.data.id,
            base_territory: doc
                .data
                .relationships
                .base_territory
                .and_then(|rel| rel.data)
                .map(|reference| reference.id),
        })
    }
}
