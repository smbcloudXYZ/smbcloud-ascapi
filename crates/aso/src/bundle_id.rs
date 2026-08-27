use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use smbcloud_ascapi_core::jsonapi::{CreateBody, CreateData, Document, ListDocument, Resource};
use smbcloud_ascapi_core::Client;
use smbcloud_ascapi_core::Result;

pub const RESOURCE_TYPE: &str = "bundleIds";

/// Note this is a *different* enum from
/// [`crate::app_store_version::Platform`] — App Store Connect uses a
/// coarser set of platforms when registering a bundle ID than when
/// declaring an App Store Version's platform. A visionOS app typically
/// registers its bundle ID as `Ios` (visionOS shares the iOS identifier
/// namespace) rather than a dedicated visionOS value, because there isn't
/// one here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BundleIdPlatform {
    #[serde(rename = "IOS")]
    Ios,
    #[serde(rename = "MAC_OS")]
    MacOs,
    #[serde(rename = "UNIVERSAL")]
    Universal,
    /// Not an app platform: the identifier kind Apple assigns to Services
    /// IDs (Sign in with Apple, push-only identifiers, and similar).
    ///
    /// Present because `GET /v1/bundleIds` returns every identifier the
    /// team owns, mixed together. Without this variant, one Services ID
    /// anywhere in the account fails the deserialization of the whole
    /// list, so listing breaks for reasons that have nothing to do with
    /// the app being looked up.
    #[serde(rename = "SERVICES")]
    Services,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleIdAttributes {
    pub name: Option<String>,
    pub identifier: Option<String>,
    pub platform: Option<BundleIdPlatform>,
    pub seed_id: Option<String>,
}

pub type BundleId = Resource<BundleIdAttributes>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleIdCreateAttributes {
    pub identifier: String,
    pub name: String,
    pub platform: BundleIdPlatform,
}

/// Registered reverse-DNS identifiers.
///
/// An extension trait rather than inherent methods, because `Client`
/// lives in the core crate and Rust only allows inherent impls in the
/// crate that defines the type. Import it, or the crate's `prelude`,
/// to call these on a `Client`.
#[async_trait]
pub trait BundleIdsApi {
    /// `GET /v1/bundleIds`, optionally filtered to an exact identifier —
    /// check whether a bundle ID is already registered before trying to
    /// create it, or before creating an App Store Version under an app
    /// that uses it.
    async fn list_bundle_ids(&self, filter_identifier: Option<&str>) -> Result<Vec<BundleId>>;

    /// `POST /v1/bundleIds` — registers a new bundle ID with the developer
    /// account (Certificates, Identifiers & Profiles).
    async fn create_bundle_id(&self, attributes: BundleIdCreateAttributes) -> Result<BundleId>;
}

#[async_trait]
impl BundleIdsApi for Client {
    async fn list_bundle_ids(&self, filter_identifier: Option<&str>) -> Result<Vec<BundleId>> {
        let mut query = Vec::new();
        if let Some(identifier) = filter_identifier {
            query.push(("filter[identifier]", identifier));
        }
        // Apple's default page size is 20. Without this an unfiltered list
        // silently stops at the first 20 identifiers, which reads as "that
        // bundle ID is not registered" for anything further down.
        query.push(("limit", "200"));

        let doc: ListDocument<BundleIdAttributes> = self
            .request(Method::GET, "/v1/bundleIds", &query, None::<&()>)
            .await?;
        Ok(doc.data)
    }

    async fn create_bundle_id(&self, attributes: BundleIdCreateAttributes) -> Result<BundleId> {
        let body = CreateBody {
            data: CreateData {
                resource_type: RESOURCE_TYPE,
                attributes,
                relationships: None::<()>,
            },
        };
        let doc: Document<BundleIdAttributes> = self
            .request(Method::POST, "/v1/bundleIds", &[], Some(&body))
            .await?;
        Ok(doc.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_services_identifier_does_not_break_the_list() {
        // Regression: `GET /v1/bundleIds` returns every identifier a team
        // owns, and a single Services ID used to fail the whole response's
        // deserialization — so `bundle-ids list` reported nothing at all
        // for accounts that have one.
        let body = serde_json::json!({
            "data": [
                {
                    "id": "AAA",
                    "type": "bundleIds",
                    "attributes": {
                        "name": "An app",
                        "identifier": "xyz.example.app",
                        "platform": "UNIVERSAL",
                        "seedId": "TEAMID"
                    }
                },
                {
                    "id": "BBB",
                    "type": "bundleIds",
                    "attributes": {
                        "name": "Sign in with Apple",
                        "identifier": "xyz.example.service",
                        "platform": "SERVICES",
                        "seedId": "TEAMID"
                    }
                }
            ]
        });

        let doc: ListDocument<BundleIdAttributes> =
            serde_json::from_value(body).expect("a Services identifier must not fail the list");
        assert_eq!(doc.data.len(), 2);
        assert_eq!(
            doc.data[1].attributes.platform,
            Some(BundleIdPlatform::Services)
        );
    }
}
