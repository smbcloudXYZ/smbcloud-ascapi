//! What an in-app purchase costs.
//!
//! A price is set by choosing one of Apple's price points in a base
//! territory and posting an `inAppPurchasePriceSchedules` resource that
//! names it as the only manual price. Apple derives the other storefronts
//! from it. The manual price itself is created inline, in the request's
//! `included` array, under a local id of the form `${name}`, which is how
//! Apple documents this endpoint.
//!
//! Price points are opaque ids. [`InAppPurchasePricesApi::find_price_point`]
//! looks one up by its customer price ("4.99") in a territory, matching the
//! string Apple sends exactly rather than parsing money as a float.

use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use smbcloud_ascapi_core::jsonapi::{path_and_query, Links, Resource};
use smbcloud_ascapi_core::{Client, Result};
use std::collections::HashMap;

pub const SCHEDULE_TYPE: &str = "inAppPurchasePriceSchedules";
pub const PRICE_TYPE: &str = "inAppPurchasePrices";
pub const PRICE_POINT_TYPE: &str = "inAppPurchasePricePoints";

/// The local id of the inline manual price in a schedule request.
const LOCAL_PRICE_ID: &str = "${price1}";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PricePointAttributes {
    /// What the customer pays, as Apple formats it ("4.99").
    #[serde(default)]
    pub customer_price: Option<String>,
    /// What the developer receives after commission and withheld tax.
    #[serde(default)]
    pub proceeds: Option<String>,
}

pub type PricePoint = Resource<PricePointAttributes>;

/// One manual or automatic price of a schedule, with its point joined in.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledPrice {
    pub territory: Option<String>,
    pub customer_price: Option<String>,
    pub proceeds: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub price_point_id: Option<String>,
}

/// Build the `POST /v1/inAppPurchasePriceSchedules` body: `territory` is the
/// base territory, `price_point_id` the point to charge there, from today.
pub fn schedule_body(
    in_app_purchase_id: &str,
    territory: &str,
    price_point_id: &str,
) -> serde_json::Value {
    serde_json::json!({
        "data": {
            "type": SCHEDULE_TYPE,
            "relationships": {
                "inAppPurchase": {
                    "data": { "type": crate::in_app_purchase::RESOURCE_TYPE, "id": in_app_purchase_id }
                },
                "baseTerritory": { "data": { "type": "territories", "id": territory } },
                "manualPrices": { "data": [ { "type": PRICE_TYPE, "id": LOCAL_PRICE_ID } ] }
            }
        },
        "included": [ {
            "type": PRICE_TYPE,
            "id": LOCAL_PRICE_ID,
            "attributes": { "startDate": null },
            "relationships": {
                "inAppPurchaseV2": {
                    "data": { "type": crate::in_app_purchase::RESOURCE_TYPE, "id": in_app_purchase_id }
                },
                "inAppPurchasePricePoint": {
                    "data": { "type": PRICE_POINT_TYPE, "id": price_point_id }
                }
            }
        } ]
    })
}

/// The price point whose customer price is exactly `customer_price`.
pub fn pick_price_point<'a>(
    points: &'a [PricePoint],
    customer_price: &str,
) -> Option<&'a PricePoint> {
    points
        .iter()
        .find(|point| point.attributes.customer_price.as_deref() == Some(customer_price))
}

#[async_trait]
pub trait InAppPurchasePricesApi {
    /// `GET /v2/inAppPurchases/{id}/pricePoints?filter[territory]=…`: every
    /// price point offered in one territory (several hundred; all pages).
    async fn list_price_points(
        &self,
        in_app_purchase_id: &str,
        territory: &str,
    ) -> Result<Vec<PricePoint>>;

    /// The price point charging exactly `customer_price` in `territory`,
    /// or `None` when that amount is not one of Apple's points there.
    async fn find_price_point(
        &self,
        in_app_purchase_id: &str,
        territory: &str,
        customer_price: &str,
    ) -> Result<Option<PricePoint>>;

    /// `POST /v1/inAppPurchasePriceSchedules`: replace the product's prices
    /// with one manual price in `territory`. Returns the schedule id.
    async fn set_price_schedule(
        &self,
        in_app_purchase_id: &str,
        territory: &str,
        price_point_id: &str,
    ) -> Result<String>;

    /// The schedule's manual prices, with customer price and proceeds
    /// joined in. Empty when no price has been set.
    async fn list_manual_prices(&self, in_app_purchase_id: &str) -> Result<Vec<ScheduledPrice>>;
}

#[async_trait]
impl InAppPurchasePricesApi for Client {
    async fn list_price_points(
        &self,
        in_app_purchase_id: &str,
        territory: &str,
    ) -> Result<Vec<PricePoint>> {
        let path = format!("/v2/inAppPurchases/{in_app_purchase_id}/pricePoints");
        self.list_all::<PricePointAttributes>(
            &path,
            &[("filter[territory]", territory), ("limit", "200")],
        )
        .await
    }

    async fn find_price_point(
        &self,
        in_app_purchase_id: &str,
        territory: &str,
        customer_price: &str,
    ) -> Result<Option<PricePoint>> {
        let points = self
            .list_price_points(in_app_purchase_id, territory)
            .await?;
        Ok(pick_price_point(&points, customer_price).cloned())
    }

    async fn set_price_schedule(
        &self,
        in_app_purchase_id: &str,
        territory: &str,
        price_point_id: &str,
    ) -> Result<String> {
        let body = schedule_body(in_app_purchase_id, territory, price_point_id);
        let doc: IdDocument = self
            .request(
                Method::POST,
                "/v1/inAppPurchasePriceSchedules",
                &[],
                Some(&body),
            )
            .await?;
        Ok(doc.data.id)
    }

    async fn list_manual_prices(&self, in_app_purchase_id: &str) -> Result<Vec<ScheduledPrice>> {
        let schedule_path = format!("/v2/inAppPurchases/{in_app_purchase_id}/iapPriceSchedule");
        let schedule: OptionalIdDocument = self
            .request(Method::GET, &schedule_path, &[], None::<&()>)
            .await?;
        let Some(schedule) = schedule.data else {
            return Ok(Vec::new());
        };

        let query = [
            ("include", "inAppPurchasePricePoint,territory"),
            ("limit", "200"),
        ];
        let mut path = format!(
            "/v1/inAppPurchasePriceSchedules/{}/manualPrices",
            schedule.id
        );
        let mut page_query: &[(&str, &str)] = &query;
        let mut prices = Vec::new();
        loop {
            let doc: PricesDocument = self
                .request(Method::GET, &path, page_query, None::<&()>)
                .await?;
            prices.extend(join(&doc));
            let Some(next) = doc
                .links
                .and_then(|l| l.next)
                .and_then(|n| path_and_query(&n))
            else {
                break;
            };
            path = next;
            page_query = &[];
        }
        Ok(prices)
    }
}

#[derive(Debug, Deserialize)]
struct IdOnly {
    id: String,
}

#[derive(Debug, Deserialize)]
struct IdDocument {
    data: IdOnly,
}

#[derive(Debug, Deserialize)]
struct OptionalIdDocument {
    data: Option<IdOnly>,
}

#[derive(Debug, Deserialize)]
struct PricesDocument {
    data: Vec<PriceRow>,
    #[serde(default)]
    included: Vec<serde_json::Value>,
    #[serde(default)]
    links: Option<Links>,
}

#[derive(Debug, Deserialize)]
struct PriceRow {
    #[serde(default)]
    attributes: PriceRowAttributes,
    #[serde(default)]
    relationships: serde_json::Value,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PriceRowAttributes {
    #[serde(default)]
    start_date: Option<String>,
    #[serde(default)]
    end_date: Option<String>,
}

fn related_id(relationships: &serde_json::Value, key: &str) -> Option<String> {
    relationships[key]["data"]["id"]
        .as_str()
        .map(str::to_string)
}

fn join(doc: &PricesDocument) -> Vec<ScheduledPrice> {
    let points: HashMap<&str, PricePointAttributes> = doc
        .included
        .iter()
        .filter(|r| r["type"] == PRICE_POINT_TYPE)
        .filter_map(|r| {
            let id = r["id"].as_str()?;
            let attrs = serde_json::from_value(r["attributes"].clone()).ok()?;
            Some((id, attrs))
        })
        .collect();
    doc.data
        .iter()
        .map(|row| {
            let point_id = related_id(&row.relationships, "inAppPurchasePricePoint");
            let point = point_id.as_deref().and_then(|id| points.get(id));
            ScheduledPrice {
                territory: related_id(&row.relationships, "territory"),
                customer_price: point.and_then(|p| p.customer_price.clone()),
                proceeds: point.and_then(|p| p.proceeds.clone()),
                start_date: row.attributes.start_date.clone(),
                end_date: row.attributes.end_date.clone(),
                price_point_id: point_id,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(id: &str, price: &str) -> PricePoint {
        Resource {
            id: id.into(),
            resource_type: PRICE_POINT_TYPE.into(),
            attributes: PricePointAttributes {
                customer_price: Some(price.into()),
                proceeds: None,
            },
            relationships: None,
        }
    }

    #[test]
    fn the_schedule_body_creates_its_price_inline() {
        let v = schedule_body("IAP1", "USA", "PP499");
        let rel = &v["data"]["relationships"];
        assert_eq!(v["data"]["type"], "inAppPurchasePriceSchedules");
        assert_eq!(rel["inAppPurchase"]["data"]["id"], "IAP1");
        assert_eq!(rel["baseTerritory"]["data"]["id"], "USA");
        assert_eq!(rel["manualPrices"]["data"][0]["id"], "${price1}");
        let price = &v["included"][0];
        assert_eq!(price["id"], "${price1}");
        assert_eq!(price["type"], "inAppPurchasePrices");
        assert!(price["attributes"]["startDate"].is_null());
        assert_eq!(
            price["relationships"]["inAppPurchaseV2"]["data"]["id"],
            "IAP1"
        );
        assert_eq!(
            price["relationships"]["inAppPurchasePricePoint"]["data"]["id"],
            "PP499"
        );
    }

    #[test]
    fn a_price_point_matches_the_exact_string() {
        let points = vec![point("A", "4.49"), point("B", "4.99"), point("C", "49.99")];
        assert_eq!(
            pick_price_point(&points, "4.99").map(|p| p.id.as_str()),
            Some("B")
        );
        assert!(pick_price_point(&points, "4.9").is_none());
        assert!(pick_price_point(&points, "5").is_none());
    }

    #[test]
    fn manual_prices_join_their_point() {
        let doc: PricesDocument = serde_json::from_value(serde_json::json!({
            "data": [{
                "type": "inAppPurchasePrices", "id": "P1",
                "attributes": { "startDate": null, "endDate": null },
                "relationships": {
                    "inAppPurchasePricePoint": { "data": { "type": "inAppPurchasePricePoints", "id": "PP499" } },
                    "territory": { "data": { "type": "territories", "id": "USA" } }
                }
            }],
            "included": [
                { "type": "inAppPurchasePricePoints", "id": "PP499",
                  "attributes": { "customerPrice": "4.99", "proceeds": "4.24" } },
                { "type": "territories", "id": "USA", "attributes": { "currency": "USD" } }
            ]
        }))
        .unwrap();
        let prices = join(&doc);
        assert_eq!(prices.len(), 1);
        assert_eq!(prices[0].territory.as_deref(), Some("USA"));
        assert_eq!(prices[0].customer_price.as_deref(), Some("4.99"));
        assert_eq!(prices[0].proceeds.as_deref(), Some("4.24"));
    }
}
