//! `GET /v1/apps`, `GET`/`PATCH /v1/apps/{id}`. Note there is no
//! `POST /v1/apps` — App Store Connect has no "create an app" endpoint.
//! An app record comes into existence the first time you register its
//! bundle ID (see [`crate::bundle_id`]) and give it a first
//! [`crate::app_store_version`]; the App Store Connect UI (or, per
//! platform, a build upload) is what actually provisions the `App` row.
//! This module only reads and updates one that already exists.

use crate::client::Client;
use crate::error::Result;
use crate::jsonapi::{Document, ListDocument, Resource, UpdateBody, UpdateData};
use reqwest::Method;
use serde::{Deserialize, Serialize};

pub const RESOURCE_TYPE: &str = "apps";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppAttributes {
    pub name: Option<String>,
    pub bundle_id: Option<String>,
    pub sku: Option<String>,
    pub primary_locale: Option<String>,
    pub is_or_ever_was_made_for_kids: Option<bool>,
    pub content_rights_declaration: Option<String>,
}

pub type App = Resource<AppAttributes>;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateAttributes {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_locale: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_rights_declaration: Option<String>,
}

impl Client {
    /// `GET /v1/apps`, optionally narrowed with `filter[bundleId]` — the
    /// usual way to resolve an app's ASC id from the bundle identifier
    /// already baked into an Xcode project.
    pub async fn list_apps(&self, filter_bundle_id: Option<&str>) -> Result<Vec<App>> {
        let mut query = Vec::new();
        if let Some(bundle_id) = filter_bundle_id {
            query.push(("filter[bundleId]", bundle_id));
        }
        let doc: ListDocument<AppAttributes> = self
            .request(Method::GET, "/v1/apps", &query, None::<&()>)
            .await?;
        Ok(doc.data)
    }

    pub async fn get_app(&self, app_id: &str) -> Result<App> {
        let path = format!("/v1/apps/{app_id}");
        let doc: Document<AppAttributes> =
            self.request(Method::GET, &path, &[], None::<&()>).await?;
        Ok(doc.data)
    }

    pub async fn update_app(&self, app_id: &str, attributes: AppUpdateAttributes) -> Result<App> {
        let path = format!("/v1/apps/{app_id}");
        let body = UpdateBody {
            data: UpdateData {
                resource_type: RESOURCE_TYPE,
                id: app_id.to_string(),
                attributes,
            },
        };
        let doc: Document<AppAttributes> =
            self.request(Method::PATCH, &path, &[], Some(&body)).await?;
        Ok(doc.data)
    }
}
