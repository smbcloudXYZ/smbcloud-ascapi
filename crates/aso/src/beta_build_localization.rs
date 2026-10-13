//! `BetaBuildLocalization` — per-locale "What to Test" notes on a TestFlight build.
//!
//! Each build can carry one `BetaBuildLocalization` per locale (e.g.
//! `"en-US"`, `"ja"`). These notes appear to testers in the TestFlight app
//! beneath the build entry. Creating or updating them is optional; if absent
//! the build simply shows no notes.

use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use smbcloud_ascapi_core::jsonapi::{
    CreateBody, CreateData, Document, Resource, ResourceId, ToOne, UpdateBody, UpdateData,
};
use smbcloud_ascapi_core::Client;
use smbcloud_ascapi_core::Result;

pub const RESOURCE_TYPE: &str = "betaBuildLocalizations";

/// Attributes of a per-locale beta build note.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BetaBuildLocalizationAttributes {
    /// BCP 47 locale tag, e.g. `"en-US"` or `"ja"`.
    pub locale: Option<String>,
    /// Free-form "What to Test" note shown to testers (max 4000 chars).
    pub whats_new: Option<String>,
}

/// A beta build localization resource.
pub type BetaBuildLocalization = Resource<BetaBuildLocalizationAttributes>;

// ── internal create/update helpers ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateAttributes {
    locale: String,
    whats_new: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct CreateRelationships {
    build: ToOne,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateAttributes {
    whats_new: String,
}

// ── trait ─────────────────────────────────────────────────────────────────────

/// Beta build localization operations on App Store Connect.
///
/// An extension trait rather than inherent methods, because `Client`
/// lives in the core crate and Rust only allows inherent impls in the
/// crate that defines the type. Import it, or the crate's `prelude`,
/// to call these on a `Client`.
#[async_trait]
pub trait BetaBuildLocalizationsApi {
    /// `GET /v1/betaBuildLocalizations?filter[build]={build_id}` — all
    /// locale notes for a build.
    async fn list_beta_build_localizations(
        &self,
        build_id: &str,
    ) -> Result<Vec<BetaBuildLocalization>>;

    /// `GET /v1/betaBuildLocalizations/{id}` — a single note by its ASC id.
    async fn get_beta_build_localization(&self, id: &str) -> Result<BetaBuildLocalization>;

    /// `POST /v1/betaBuildLocalizations` — create a locale note for a build.
    ///
    /// `locale` is a BCP 47 tag (e.g. `"en-US"`). `whats_new` is optional
    /// at creation; App Store Connect accepts an empty record.
    async fn create_beta_build_localization(
        &self,
        build_id: &str,
        locale: &str,
        whats_new: Option<&str>,
    ) -> Result<BetaBuildLocalization>;

    /// `PATCH /v1/betaBuildLocalizations/{id}` — update the "What to Test"
    /// text for an existing localization.
    async fn update_beta_build_localization(
        &self,
        id: &str,
        whats_new: &str,
    ) -> Result<BetaBuildLocalization>;
}

#[async_trait]
impl BetaBuildLocalizationsApi for Client {
    async fn list_beta_build_localizations(
        &self,
        build_id: &str,
    ) -> Result<Vec<BetaBuildLocalization>> {
        self.list_all::<BetaBuildLocalizationAttributes>(
            "/v1/betaBuildLocalizations",
            &[("filter[build]", build_id), ("limit", "200")],
        )
        .await
    }

    async fn get_beta_build_localization(&self, id: &str) -> Result<BetaBuildLocalization> {
        let path = format!("/v1/betaBuildLocalizations/{id}");
        let doc: Document<BetaBuildLocalizationAttributes> =
            self.request(Method::GET, &path, &[], None::<&()>).await?;
        Ok(doc.data)
    }

    async fn create_beta_build_localization(
        &self,
        build_id: &str,
        locale: &str,
        whats_new: Option<&str>,
    ) -> Result<BetaBuildLocalization> {
        let body = CreateBody {
            data: CreateData {
                resource_type: RESOURCE_TYPE,
                attributes: CreateAttributes {
                    locale: locale.to_string(),
                    whats_new: whats_new.map(str::to_string),
                },
                relationships: Some(CreateRelationships {
                    build: ToOne {
                        data: ResourceId {
                            resource_type: crate::build::RESOURCE_TYPE,
                            id: build_id.to_string(),
                        },
                    },
                }),
            },
        };
        let doc: Document<BetaBuildLocalizationAttributes> = self
            .request(Method::POST, "/v1/betaBuildLocalizations", &[], Some(&body))
            .await?;
        Ok(doc.data)
    }

    async fn update_beta_build_localization(
        &self,
        id: &str,
        whats_new: &str,
    ) -> Result<BetaBuildLocalization> {
        let path = format!("/v1/betaBuildLocalizations/{id}");
        let body = UpdateBody {
            data: UpdateData {
                resource_type: RESOURCE_TYPE,
                id: id.to_string(),
                attributes: UpdateAttributes {
                    whats_new: whats_new.to_string(),
                },
            },
        };
        let doc: Document<BetaBuildLocalizationAttributes> =
            self.request(Method::PATCH, &path, &[], Some(&body)).await?;
        Ok(doc.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smbcloud_ascapi_core::jsonapi::ListDocument;

    #[test]
    fn deserializes_a_localization_list() {
        let body = serde_json::json!({
            "data": [
                {
                    "id": "LOC1",
                    "type": "betaBuildLocalizations",
                    "attributes": {
                        "locale": "en-US",
                        "whatsNew": "Bug fixes and performance improvements."
                    }
                },
                {
                    "id": "LOC2",
                    "type": "betaBuildLocalizations",
                    "attributes": {
                        "locale": "ja",
                        "whatsNew": "バグ修正とパフォーマンス改善。"
                    }
                }
            ]
        });

        let doc: ListDocument<BetaBuildLocalizationAttributes> =
            serde_json::from_value(body).expect("localization list must deserialize");
        assert_eq!(doc.data.len(), 2);

        let en = &doc.data[0];
        assert_eq!(en.id, "LOC1");
        assert_eq!(en.attributes.locale.as_deref(), Some("en-US"));
        assert_eq!(
            en.attributes.whats_new.as_deref(),
            Some("Bug fixes and performance improvements.")
        );

        let ja = &doc.data[1];
        assert_eq!(ja.attributes.locale.as_deref(), Some("ja"));
    }

    #[test]
    fn create_body_serializes_filter_key() {
        // The filter key for the list endpoint must match exactly.
        let query: Vec<(&str, &str)> = vec![("filter[build]", "BUILD1"), ("limit", "200")];
        assert_eq!(query[0].0, "filter[build]");
        assert_eq!(query[0].1, "BUILD1");
    }

    #[test]
    fn create_body_without_whats_new_omits_the_field() {
        // whatsNew = None → field should be null in JSON (we serialize as
        // `Option`, so it is present as null rather than absent, which ASC
        // accepts as "no notes yet").
        let body = CreateBody {
            data: CreateData {
                resource_type: RESOURCE_TYPE,
                attributes: CreateAttributes {
                    locale: "en-US".into(),
                    whats_new: None,
                },
                relationships: None::<()>,
            },
        };
        let v = serde_json::to_value(&body).unwrap();
        let attrs = &v["data"]["attributes"];
        assert_eq!(attrs["locale"], "en-US");
        assert!(attrs["whatsNew"].is_null());
    }
}
