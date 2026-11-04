//! `inAppPurchaseAvailabilities`: the storefronts a product is sold in.
//!
//! Posting one replaces the whole set. `availableInNewTerritories` decides
//! whether storefronts Apple opens later are added automatically.

use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use smbcloud_ascapi_core::jsonapi::Resource;
use smbcloud_ascapi_core::{Client, Result};

pub const RESOURCE_TYPE: &str = "inAppPurchaseAvailabilities";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerritoryAttributes {
    #[serde(default)]
    pub currency: Option<String>,
}

/// A storefront; its id is the territory code (`USA`, `SWE`, `IDN`).
pub type Territory = Resource<TerritoryAttributes>;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailabilityAttributes {
    #[serde(default)]
    pub available_in_new_territories: Option<bool>,
}

/// What a product is available in, as read back.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Availability {
    pub id: String,
    pub available_in_new_territories: Option<bool>,
    pub territories: Vec<String>,
}

/// The `POST /v1/inAppPurchaseAvailabilities` body.
pub fn availability_body(
    in_app_purchase_id: &str,
    territories: &[String],
    available_in_new_territories: bool,
) -> serde_json::Value {
    let territories: Vec<_> = territories
        .iter()
        .map(|code| serde_json::json!({ "type": "territories", "id": code }))
        .collect();
    serde_json::json!({
        "data": {
            "type": RESOURCE_TYPE,
            "attributes": { "availableInNewTerritories": available_in_new_territories },
            "relationships": {
                "inAppPurchase": {
                    "data": { "type": crate::in_app_purchase::RESOURCE_TYPE, "id": in_app_purchase_id }
                },
                "availableTerritories": { "data": territories }
            }
        }
    })
}

#[async_trait]
pub trait InAppPurchaseAvailabilityApi {
    /// `GET /v1/territories`: every storefront App Store Connect knows.
    async fn list_territories(&self) -> Result<Vec<Territory>>;

    /// `POST /v1/inAppPurchaseAvailabilities`. Returns the availability id.
    async fn set_in_app_purchase_availability(
        &self,
        in_app_purchase_id: &str,
        territories: &[String],
        available_in_new_territories: bool,
    ) -> Result<String>;

    /// `GET /v2/inAppPurchases/{id}/inAppPurchaseAvailability` and its
    /// territories. `None` when availability was never set.
    async fn get_in_app_purchase_availability(
        &self,
        in_app_purchase_id: &str,
    ) -> Result<Option<Availability>>;
}

#[derive(Debug, Deserialize)]
struct AvailabilityDocument {
    data: Option<Resource<AvailabilityAttributes>>,
}

#[async_trait]
impl InAppPurchaseAvailabilityApi for Client {
    async fn list_territories(&self) -> Result<Vec<Territory>> {
        self.list_all::<TerritoryAttributes>("/v1/territories", &[("limit", "200")])
            .await
    }

    async fn set_in_app_purchase_availability(
        &self,
        in_app_purchase_id: &str,
        territories: &[String],
        available_in_new_territories: bool,
    ) -> Result<String> {
        let body = availability_body(
            in_app_purchase_id,
            territories,
            available_in_new_territories,
        );
        let doc: AvailabilityDocument = self
            .request(
                Method::POST,
                "/v1/inAppPurchaseAvailabilities",
                &[],
                Some(&body),
            )
            .await?;
        Ok(doc.data.map(|d| d.id).unwrap_or_default())
    }

    async fn get_in_app_purchase_availability(
        &self,
        in_app_purchase_id: &str,
    ) -> Result<Option<Availability>> {
        let path = format!("/v2/inAppPurchases/{in_app_purchase_id}/inAppPurchaseAvailability");
        let doc: AvailabilityDocument = self.request(Method::GET, &path, &[], None::<&()>).await?;
        let Some(data) = doc.data else {
            return Ok(None);
        };
        let territories_path = format!(
            "/v1/inAppPurchaseAvailabilities/{}/availableTerritories",
            data.id
        );
        let mut territories: Vec<String> = self
            .list_all::<TerritoryAttributes>(&territories_path, &[("limit", "200")])
            .await?
            .into_iter()
            .map(|t| t.id)
            .collect();
        territories.sort();
        Ok(Some(Availability {
            id: data.id,
            available_in_new_territories: data.attributes.available_in_new_territories,
            territories,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_body_lists_every_territory() {
        let v = availability_body("IAP1", &["SWE".into(), "USA".into()], true);
        assert_eq!(v["data"]["type"], "inAppPurchaseAvailabilities");
        assert_eq!(v["data"]["attributes"]["availableInNewTerritories"], true);
        assert_eq!(
            v["data"]["relationships"]["inAppPurchase"]["data"]["id"],
            "IAP1"
        );
        let list = v["data"]["relationships"]["availableTerritories"]["data"]
            .as_array()
            .unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[1]["type"], "territories");
        assert_eq!(list[1]["id"], "USA");
    }
}
