//! `GET /v1/appPriceSchedules/{id}/manualPrices` and `.../automaticPrices`.
//!
//! The two collections answer different questions and are easy to confuse:
//!
//! - **Manual** prices are what a human set. Usually one row, in the
//!   schedule's base territory.
//! - **Automatic** prices are what Apple converted that into for every
//!   other storefront — currently ~178 rows. This is where you look to
//!   find out what an app actually costs in Indonesia, and whether a
//!   currency move has a conversion scheduled (`endDate` in the near
//!   future means the price changes then).
//!
//! Both return `appPrices`, which hold no money. The amounts live on the
//! related `appPricePoint`, the currency on the related `territory`, so
//! every request here sends `include=appPricePoint,territory` and joins
//! the result into [`TerritoryPrice`] before returning it.

use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use smbcloud_ascapi_core::Client;
use smbcloud_ascapi_core::Result;
use std::collections::HashMap;

pub const RESOURCE_TYPE: &str = "appPrices";

/// Apple caps `limit` at 200 on these collections. Territories number
/// ~178, so one page covers a whole app today; [`AppPricesApi`] still
/// follows `links.next` rather than trusting that to hold.
const PAGE_LIMIT: &str = "200";

/// Which of a schedule's two price collections to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriceKind {
    /// Prices a human set, in the base territory.
    Manual,
    /// Prices Apple derived for the other storefronts.
    Automatic,
}

impl PriceKind {
    fn path_segment(self) -> &'static str {
        match self {
            PriceKind::Manual => "manualPrices",
            PriceKind::Automatic => "automaticPrices",
        }
    }
}

/// One territory's price, with the price point and territory already
/// joined in — the shape callers actually want, rather than the three
/// resources App Store Connect splits it across.
///
/// `customer_price` and `proceeds` are strings because Apple sends them
/// as strings ("7.99"). They are left that way rather than parsed into a
/// float, since money in a display path should not go through binary
/// floating point, and the caller knows better than this crate whether it
/// wants a decimal type or the literal text.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerritoryPrice {
    /// Territory code, e.g. `USA`, `IDN`.
    pub territory: String,
    /// ISO currency of `customer_price`, e.g. `USD`.
    pub currency: Option<String>,
    /// What the customer pays, as Apple formats it.
    pub customer_price: Option<String>,
    /// What the developer receives, net of Apple's commission *and* of any
    /// tax Apple collects and remits in that territory.
    ///
    /// Only in territories where Apple withholds nothing does the ratio to
    /// `customer_price` equal the commission rate — USD 6.79 on USD 7.99
    /// is the Small Business Program's 85%. Indonesia's IDR 98,784 on IDR
    /// 129,000 is ~77% for the same account, because local VAT comes out
    /// first. Do not infer a commission tier from a single territory.
    pub proceeds: Option<String>,
    /// True when a human set this price, false when Apple converted it.
    pub manual: bool,
    /// When this price takes effect. `None` means "already in effect".
    pub start_date: Option<String>,
    /// When this price stops applying. A near-future date means a price
    /// change is already scheduled for that territory.
    pub end_date: Option<String>,
    /// The related `appPricePoint` id, for callers that want to look up
    /// equalizations. Opaque — see the crate docs.
    pub price_point_id: Option<String>,
    /// The `appPrices` row's own id. Opaque.
    pub app_price_id: String,
}

/// App prices: what an app costs, per territory.
///
/// An extension trait rather than inherent methods, because `Client`
/// lives in the core crate and Rust only allows inherent impls in the
/// crate that defines the type. Import it, or the crate's `prelude`,
/// to call these on a `Client`.
#[async_trait]
pub trait AppPricesApi {
    /// `GET /v1/appPriceSchedules/{app_id}/{manual,automatic}Prices`,
    /// joined against the included price points and territories.
    ///
    /// Takes the **app** id; a schedule shares its app's id. Pass
    /// `territory` to narrow to one storefront (`filter[territory]`),
    /// which is much cheaper than fetching all ~178 and filtering
    /// locally. Rows come back sorted by territory so two runs diff
    /// cleanly.
    async fn list_app_prices(
        &self,
        app_id: &str,
        kind: PriceKind,
        territory: Option<&str>,
    ) -> Result<Vec<TerritoryPrice>>;
}

#[async_trait]
impl AppPricesApi for Client {
    async fn list_app_prices(
        &self,
        app_id: &str,
        kind: PriceKind,
        territory: Option<&str>,
    ) -> Result<Vec<TerritoryPrice>> {
        let mut query = vec![
            ("include", "appPricePoint,territory"),
            ("limit", PAGE_LIMIT),
        ];
        if let Some(code) = territory {
            query.push(("filter[territory]", code));
        }

        let mut path = format!("/v1/appPriceSchedules/{app_id}/{}", kind.path_segment());
        let mut query_for_this_page: &[(&str, &str)] = &query;
        let mut prices = Vec::new();

        loop {
            let doc: PricesDocument = self
                .request(Method::GET, &path, query_for_this_page, None::<&()>)
                .await?;
            prices.extend(join(&doc));

            // `links.next` already carries include/limit/filter and a
            // cursor, so following it must not re-append our own query.
            let Some(next) = doc.links.and_then(|links| links.next) else {
                break;
            };
            let Some(next_path) = path_and_query(&next) else {
                break;
            };
            path = next_path;
            query_for_this_page = &[];
        }

        prices.sort_by(|a, b| a.territory.cmp(&b.territory));
        Ok(prices)
    }
}

/// Flatten one page: index the `included` array by id, then attach each
/// price's point and territory.
fn join(doc: &PricesDocument) -> Vec<TerritoryPrice> {
    let mut points: HashMap<&str, &PricePointAttributes> = HashMap::new();
    let mut currencies: HashMap<&str, Option<&str>> = HashMap::new();
    for resource in &doc.included {
        match resource {
            IncludedResource::AppPricePoint { id, attributes } => {
                points.insert(id.as_str(), attributes);
            }
            IncludedResource::Territory { id, attributes } => {
                currencies.insert(id.as_str(), attributes.currency.as_deref());
            }
            IncludedResource::Unknown => {}
        }
    }

    doc.data
        .iter()
        .map(|price| {
            let point_id = price
                .relationships
                .app_price_point
                .as_ref()
                .and_then(|rel| rel.data.as_ref())
                .map(|reference| reference.id.clone());
            let territory = price
                .relationships
                .territory
                .as_ref()
                .and_then(|rel| rel.data.as_ref())
                .map(|reference| reference.id.clone())
                .unwrap_or_default();
            let point = point_id.as_deref().and_then(|id| points.get(id)).copied();

            TerritoryPrice {
                currency: currencies
                    .get(territory.as_str())
                    .copied()
                    .flatten()
                    .map(str::to_string),
                customer_price: point.and_then(|p| p.customer_price.clone()),
                proceeds: point.and_then(|p| p.proceeds.clone()),
                manual: price.attributes.manual.unwrap_or(false),
                start_date: price.attributes.start_date.clone(),
                end_date: price.attributes.end_date.clone(),
                price_point_id: point_id,
                app_price_id: price.id.clone(),
                territory,
            }
        })
        .collect()
}

/// Turn an absolute `links.next` URL into the path+query
/// [`Client::request`] wants, since that method prefixes its own base
/// URL. Returns `None` for anything unparseable, which the caller treats
/// as "no more pages" rather than an error — a missing last page is
/// better than a panic on a link Apple changed the shape of.
fn path_and_query(url: &str) -> Option<String> {
    let after_scheme = url.split_once("://")?.1;
    let (_host, rest) = after_scheme.split_once('/')?;
    Some(format!("/{rest}"))
}

#[derive(Debug, Clone, Deserialize)]
struct PricesDocument {
    data: Vec<PriceResource>,
    #[serde(default)]
    included: Vec<IncludedResource>,
    #[serde(default)]
    links: Option<Links>,
}

#[derive(Debug, Clone, Deserialize)]
struct Links {
    #[serde(default)]
    next: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct PriceResource {
    id: String,
    #[serde(default)]
    attributes: PriceAttributes,
    #[serde(default)]
    relationships: PriceRelationships,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PriceAttributes {
    #[serde(default)]
    manual: Option<bool>,
    #[serde(default)]
    start_date: Option<String>,
    #[serde(default)]
    end_date: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PriceRelationships {
    #[serde(default)]
    app_price_point: Option<ToOne>,
    #[serde(default)]
    territory: Option<ToOne>,
}

/// A to-one relationship as it comes *back* from Apple. Distinct from
/// [`smbcloud_ascapi_core::jsonapi::ToOne`], which is serialize-only and
/// requires `data` — a read can legitimately carry a relationship whose
/// `data` is absent (`"territory": {}` on an included price point).
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ToOne {
    #[serde(default)]
    pub(crate) data: Option<Reference>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Reference {
    pub(crate) id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
enum IncludedResource {
    #[serde(rename = "appPricePoints")]
    AppPricePoint {
        id: String,
        attributes: PricePointAttributes,
    },
    #[serde(rename = "territories")]
    Territory {
        id: String,
        attributes: TerritoryAttributes,
    },
    /// Anything Apple adds to `included` later. Ignored rather than
    /// fatal, so a new sideloaded type does not break price reading.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PricePointAttributes {
    #[serde(default)]
    customer_price: Option<String>,
    #[serde(default)]
    proceeds: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct TerritoryAttributes {
    #[serde(default)]
    currency: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // The exact body App Store Connect returned for
    // /v1/appPriceSchedules/6751143237/manualPrices, trimmed to the fields
    // this module reads. The join is the whole point of the crate, so it
    // is pinned against a real response rather than a hand-made one.
    const MANUAL_PAGE: &str = r#"{
      "data": [{
        "type": "appPrices",
        "id": "eyJzIjoiNjc1MTE0MzIzNyJ9",
        "attributes": { "manual": true, "startDate": null, "endDate": null },
        "relationships": {
          "appPricePoint": { "data": { "type": "appPricePoints", "id": "point-usa" } },
          "territory": { "data": { "type": "territories", "id": "USA" } }
        }
      }],
      "included": [
        { "type": "appPricePoints", "id": "point-usa",
          "attributes": { "customerPrice": "7.99", "proceeds": "6.79" },
          "relationships": { "territory": {} } },
        { "type": "territories", "id": "USA", "attributes": { "currency": "USD" } }
      ],
      "links": { "self": "https://api.appstoreconnect.apple.com/v1/x" }
    }"#;

    #[test]
    fn joins_price_point_and_territory_onto_the_price() {
        let doc: PricesDocument = serde_json::from_str(MANUAL_PAGE).unwrap();
        let prices = join(&doc);

        assert_eq!(prices.len(), 1);
        let price = &prices[0];
        assert_eq!(price.territory, "USA");
        assert_eq!(price.currency.as_deref(), Some("USD"));
        assert_eq!(price.customer_price.as_deref(), Some("7.99"));
        assert_eq!(price.proceeds.as_deref(), Some("6.79"));
        assert!(price.manual);
        assert_eq!(price.end_date, None);
    }

    #[test]
    fn tolerates_an_included_type_it_does_not_know() {
        let body = MANUAL_PAGE.replace(
            r#"{ "type": "territories", "id": "USA", "attributes": { "currency": "USD" } }"#,
            r#"{ "type": "territories", "id": "USA", "attributes": { "currency": "USD" } },
               { "type": "somethingNew", "id": "z", "attributes": { "whatever": 1 } }"#,
        );
        let doc: PricesDocument = serde_json::from_str(&body).unwrap();
        assert_eq!(join(&doc)[0].customer_price.as_deref(), Some("7.99"));
    }

    // Apple schedules a conversion by giving the current row an endDate;
    // surfacing that is how a caller sees a price change coming.
    #[test]
    fn keeps_scheduled_end_dates_on_automatic_prices() {
        let body = MANUAL_PAGE
            .replace(r#""manual": true"#, r#""manual": false"#)
            .replace(r#""endDate": null"#, r#""endDate": "2026-09-14""#);
        let doc: PricesDocument = serde_json::from_str(&body).unwrap();
        let price = &join(&doc)[0];

        assert!(!price.manual);
        assert_eq!(price.end_date.as_deref(), Some("2026-09-14"));
    }

    #[test]
    fn strips_the_host_off_a_next_link() {
        assert_eq!(
            path_and_query("https://api.appstoreconnect.apple.com/v1/appPriceSchedules/1/automaticPrices?cursor=AQ").as_deref(),
            Some("/v1/appPriceSchedules/1/automaticPrices?cursor=AQ")
        );
        assert_eq!(path_and_query("not-a-url"), None);
    }
}
