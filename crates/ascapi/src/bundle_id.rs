use crate::client::Client;
use crate::error::Result;
use crate::jsonapi::{CreateBody, CreateData, Document, ListDocument, Resource};
use reqwest::Method;
use serde::{Deserialize, Serialize};

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

impl Client {
    /// `GET /v1/bundleIds`, optionally filtered to an exact identifier —
    /// check whether a bundle ID is already registered before trying to
    /// create it, or before creating an App Store Version under an app
    /// that uses it.
    pub async fn list_bundle_ids(&self, filter_identifier: Option<&str>) -> Result<Vec<BundleId>> {
        let mut query = Vec::new();
        if let Some(identifier) = filter_identifier {
            query.push(("filter[identifier]", identifier));
        }
        let doc: ListDocument<BundleIdAttributes> = self
            .request(Method::GET, "/v1/bundleIds", &query, None::<&()>)
            .await?;
        Ok(doc.data)
    }

    /// `POST /v1/bundleIds` — registers a new bundle ID with the developer
    /// account (Certificates, Identifiers & Profiles).
    pub async fn create_bundle_id(&self, attributes: BundleIdCreateAttributes) -> Result<BundleId> {
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
