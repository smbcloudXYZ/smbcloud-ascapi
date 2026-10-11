//! `GET /v1/apps`, `GET`/`PATCH /v1/apps/{id}`. Note there is no
//! `POST /v1/apps` — App Store Connect has no "create an app" endpoint.
//! An app record comes into existence the first time you register its
//! bundle ID (see [`crate::bundle_id`]) and give it a first
//! [`crate::app_store_version`]; the App Store Connect UI (or, per
//! platform, a build upload) is what actually provisions the `App` row.
//! This module only reads and updates one that already exists.

use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use smbcloud_ascapi_core::jsonapi::{Document, Resource, UpdateBody, UpdateData};
use smbcloud_ascapi_core::Client;
use smbcloud_ascapi_core::Result;

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

/// Apps: the top-level record for a product on the store.
///
/// An extension trait rather than inherent methods, because `Client`
/// lives in the core crate and Rust only allows inherent impls in the
/// crate that defines the type. Import it, or the crate's `prelude`,
/// to call these on a `Client`.
#[async_trait]
pub trait AppsApi {
    /// `GET /v1/apps`, optionally narrowed to the app with exactly this
    /// bundle identifier — the usual way to resolve an app's ASC id from
    /// the bundle identifier already baked into an Xcode project.
    ///
    /// App Store Connect's `filter[bundleId]` matches substrings, so
    /// `ai.splitfire.KaroKowe` also returns `ai.splitfire.KaroKoweJam`, and
    /// listed first. The filter still goes to the server to keep the
    /// response small, and the rows are then cut to exact matches here.
    async fn list_apps(&self, filter_bundle_id: Option<&str>) -> Result<Vec<App>>;

    async fn get_app(&self, app_id: &str) -> Result<App>;

    async fn update_app(&self, app_id: &str, attributes: AppUpdateAttributes) -> Result<App>;
}

#[async_trait]
impl AppsApi for Client {
    async fn list_apps(&self, filter_bundle_id: Option<&str>) -> Result<Vec<App>> {
        let mut query = Vec::new();
        if let Some(bundle_id) = filter_bundle_id {
            query.push(("filter[bundleId]", bundle_id));
        }
        let apps = self.list_all::<AppAttributes>("/v1/apps", &query).await?;
        Ok(match filter_bundle_id {
            Some(bundle_id) => exact_bundle_id(apps, bundle_id),
            None => apps,
        })
    }

    async fn get_app(&self, app_id: &str) -> Result<App> {
        let path = format!("/v1/apps/{app_id}");
        let doc: Document<AppAttributes> =
            self.request(Method::GET, &path, &[], None::<&()>).await?;
        Ok(doc.data)
    }

    async fn update_app(&self, app_id: &str, attributes: AppUpdateAttributes) -> Result<App> {
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

/// Keep the apps whose bundle identifier is exactly `bundle_id`.
fn exact_bundle_id(apps: Vec<App>, bundle_id: &str) -> Vec<App> {
    apps.into_iter()
        .filter(|app| app.attributes.bundle_id.as_deref() == Some(bundle_id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use smbcloud_ascapi_core::jsonapi::ListDocument;

    /// The shape App Store Connect returned for `filter[bundleId]=
    /// ai.splitfire.KaroKowe`: the longer bundle ID first.
    #[test]
    fn a_bundle_id_filter_keeps_only_the_exact_match() {
        let doc: ListDocument<AppAttributes> = serde_json::from_value(serde_json::json!({
            "data": [
                { "id": "6787445875", "type": "apps",
                  "attributes": { "bundleId": "ai.splitfire.KaroKoweJam", "name": "KaroKowe Jam" } },
                { "id": "6756801861", "type": "apps",
                  "attributes": { "bundleId": "ai.splitfire.KaroKowe", "name": "Karaoke KaroKowe" } }
            ]
        }))
        .unwrap();
        let apps = exact_bundle_id(doc.data, "ai.splitfire.KaroKowe");
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].id, "6756801861");
    }
}
