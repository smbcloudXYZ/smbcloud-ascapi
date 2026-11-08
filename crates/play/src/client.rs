use reqwest::Method;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::auth::{ServiceAccount, TokenCache};
use crate::error::{Error, Result};
use crate::image::{Image, ImageType};
use crate::listing::{Listing, ListingFields};
use crate::track::Track;

const API_ROOT: &str = "https://androidpublisher.googleapis.com";

/// An open edit. Nothing in it is visible to users until it is committed.
#[derive(Debug, Clone, Deserialize)]
pub struct Edit {
    pub id: String,
    #[serde(default, rename = "expiryTimeSeconds")]
    pub expiry_time_seconds: Option<String>,
}

/// Build `/androidpublisher/v3/applications/{package}/edits…`. `suffix` is
/// appended verbatim, so it carries its own leading `/` or `:`.
pub(crate) fn edits_path(package: &str, suffix: &str) -> String {
    format!("/androidpublisher/v3/applications/{package}/edits{suffix}")
}

/// Build `/androidpublisher/v3/applications/{package}/oneTimeProducts…`.
pub(crate) fn products_path(package: &str, suffix: &str) -> String {
    format!("/androidpublisher/v3/applications/{package}/oneTimeProducts{suffix}")
}

/// `onetimeproducts.patch` is the one call in this resource whose path is
/// all lowercase (`onetimeproducts`); get, list and the state changes use
/// `oneTimeProducts`. Google's discovery document says so, and a wrong case is
/// a 404 page from the front end.
pub(crate) fn product_patch_path(package: &str, product_id: &str) -> String {
    format!("/androidpublisher/v3/applications/{package}/onetimeproducts/{product_id}")
}

pub(crate) fn upload_path(package: &str, edit_id: &str, locale: &str, image: ImageType) -> String {
    format!(
        "/upload/androidpublisher/v3/applications/{package}/edits/{edit_id}/listings/{locale}/{}?uploadType=media",
        image.api_name()
    )
}

/// `POST` target for `edits.bundles.upload`. `ack_install_warning` asks Play
/// to accept a bundle it would otherwise refuse for an install-time warning.
pub(crate) fn bundle_upload_path(
    package: &str,
    edit_id: &str,
    ack_install_warning: bool,
) -> String {
    let ack = if ack_install_warning {
        "&ackBundleInstallationWarning=true"
    } else {
        ""
    };
    format!(
        "/upload/androidpublisher/v3/applications/{package}/edits/{edit_id}/bundles?uploadType=media{ack}"
    )
}

/// `POST` target for `edits.deobfuscationfiles.upload` (a ProGuard/R8
/// mapping file for one uploaded version code).
pub(crate) fn deobfuscation_path(package: &str, edit_id: &str, version_code: u64) -> String {
    format!(
        "/upload/androidpublisher/v3/applications/{package}/edits/{edit_id}/apks/{version_code}/deobfuscationFiles/proguard?uploadType=media"
    )
}

/// What Play reports for an uploaded bundle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UploadedBundle {
    pub version_code: u64,
    /// Lowercase hex SHA-256 of the bundle as Play received it.
    #[serde(default)]
    pub sha256: String,
}

/// Path and query for a patch that must not create: no `allowMissing`.
pub(crate) fn product_update_path(
    package: &str,
    product_id: &str,
    update_mask: &str,
    regions_version: Option<&str>,
) -> String {
    let mut path = format!(
        "{}?updateMask={update_mask}",
        product_patch_path(package, product_id)
    );
    if let Some(version) = regions_version {
        path.push_str("&regionsVersion.version=");
        path.push_str(&version.replace('/', "%2F"));
    }
    path
}

pub struct PlayClient {
    http: reqwest::Client,
    account: ServiceAccount,
    tokens: TokenCache,
    root: String,
}

impl PlayClient {
    pub fn new(account: ServiceAccount) -> Self {
        Self {
            http: reqwest::Client::new(),
            account,
            tokens: TokenCache::new(),
            root: API_ROOT.to_string(),
        }
    }

    /// The service account's email, for diagnostics.
    pub fn account_email(&self) -> &str {
        &self.account.client_email
    }

    async fn send(
        &self,
        method: Method,
        path_and_query: &str,
        body: Option<(&str, Vec<u8>)>,
    ) -> Result<reqwest::Response> {
        let token = self.tokens.get(&self.http, &self.account).await?;
        let mut request = self
            .http
            .request(method, format!("{}{path_and_query}", self.root))
            .bearer_auth(token);
        if let Some((content_type, bytes)) = body {
            request = request.header("Content-Type", content_type).body(bytes);
        }
        let response = request.send().await?;
        let status = response.status();
        if status.is_success() {
            Ok(response)
        } else {
            Err(Error::Api {
                status: status.as_u16(),
                detail: response.text().await.unwrap_or_default(),
            })
        }
    }

    async fn json<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&impl Serialize>,
    ) -> Result<T> {
        let body = match body {
            Some(value) => Some(("application/json", serde_json::to_vec(value)?)),
            None => (method == Method::POST).then_some(("application/json", b"{}".to_vec())),
        };
        let response = self.send(method, path, body).await?;
        Ok(response.json().await?)
    }

    async fn no_body(&self, method: Method, path: &str) -> Result<()> {
        self.send(method, path, None).await?;
        Ok(())
    }

    pub async fn insert_edit(&self, package: &str) -> Result<Edit> {
        self.json(Method::POST, &edits_path(package, ""), None::<&()>)
            .await
    }

    pub async fn get_edit(&self, package: &str, edit_id: &str) -> Result<Edit> {
        self.json(
            Method::GET,
            &edits_path(package, &format!("/{edit_id}")),
            None::<&()>,
        )
        .await
    }

    pub async fn validate_edit(&self, package: &str, edit_id: &str) -> Result<()> {
        let path = edits_path(package, &format!("/{edit_id}:validate"));
        self.send(
            Method::POST,
            &path,
            Some(("application/json", b"{}".to_vec())),
        )
        .await?;
        Ok(())
    }

    /// Commit the edit. `changes_not_sent_for_review` is for apps with
    /// rejected changes pending: Play then publishes without sending them
    /// for review again.
    pub async fn commit_edit(
        &self,
        package: &str,
        edit_id: &str,
        changes_not_sent_for_review: bool,
    ) -> Result<()> {
        let query = if changes_not_sent_for_review {
            "?changesNotSentForReview=true"
        } else {
            ""
        };
        let path = edits_path(package, &format!("/{edit_id}:commit{query}"));
        self.send(
            Method::POST,
            &path,
            Some(("application/json", b"{}".to_vec())),
        )
        .await?;
        Ok(())
    }

    pub async fn delete_edit(&self, package: &str, edit_id: &str) -> Result<()> {
        self.no_body(Method::DELETE, &edits_path(package, &format!("/{edit_id}")))
            .await
    }

    pub async fn list_listings(&self, package: &str, edit_id: &str) -> Result<Vec<Listing>> {
        #[derive(Deserialize)]
        struct Page {
            #[serde(default)]
            listings: Vec<Listing>,
        }
        let page: Page = self
            .json(
                Method::GET,
                &edits_path(package, &format!("/{edit_id}/listings")),
                None::<&()>,
            )
            .await?;
        Ok(page.listings)
    }

    pub async fn get_listing(&self, package: &str, edit_id: &str, locale: &str) -> Result<Listing> {
        self.json(
            Method::GET,
            &edits_path(package, &format!("/{edit_id}/listings/{locale}")),
            None::<&()>,
        )
        .await
    }

    /// `listings.patch`; a locale the app does not have yet is created with
    /// `listings.update` (PUT), which Play treats as an upsert.
    pub async fn patch_listing(
        &self,
        package: &str,
        edit_id: &str,
        locale: &str,
        fields: &ListingFields,
    ) -> Result<Listing> {
        let path = edits_path(package, &format!("/{edit_id}/listings/{locale}"));
        match self.json(Method::PATCH, &path, Some(fields)).await {
            Err(Error::Api { status: 404, .. }) => {
                self.json(Method::PUT, &path, Some(fields)).await
            }
            other => other,
        }
    }

    pub async fn list_images(
        &self,
        package: &str,
        edit_id: &str,
        locale: &str,
        image_type: ImageType,
    ) -> Result<Vec<Image>> {
        #[derive(Deserialize)]
        struct Page {
            #[serde(default)]
            images: Vec<Image>,
        }
        let path = edits_path(
            package,
            &format!("/{edit_id}/listings/{locale}/{}", image_type.api_name()),
        );
        let page: Page = self.json(Method::GET, &path, None::<&()>).await?;
        Ok(page.images)
    }

    pub async fn upload_image(
        &self,
        package: &str,
        edit_id: &str,
        locale: &str,
        image_type: ImageType,
        content_type: &str,
        bytes: Vec<u8>,
    ) -> Result<Image> {
        #[derive(Deserialize)]
        struct Uploaded {
            image: Image,
        }
        let path = upload_path(package, edit_id, locale, image_type);
        let response = self
            .send(Method::POST, &path, Some((content_type, bytes)))
            .await?;
        let uploaded: Uploaded = response.json().await?;
        Ok(uploaded.image)
    }

    /// Upload an `.aab` into the edit. The version code is assigned by Play
    /// from the bundle and consumed for good, even if the edit is later
    /// discarded.
    pub async fn upload_bundle(
        &self,
        package: &str,
        edit_id: &str,
        bytes: Vec<u8>,
        ack_install_warning: bool,
    ) -> Result<UploadedBundle> {
        let path = bundle_upload_path(package, edit_id, ack_install_warning);
        let response = self
            .send(
                Method::POST,
                &path,
                Some(("application/octet-stream", bytes)),
            )
            .await?;
        Ok(response.json().await?)
    }

    /// Attach a ProGuard/R8 `mapping.txt` to an uploaded version code.
    pub async fn upload_deobfuscation(
        &self,
        package: &str,
        edit_id: &str,
        version_code: u64,
        bytes: Vec<u8>,
    ) -> Result<()> {
        let path = deobfuscation_path(package, edit_id, version_code);
        self.send(
            Method::POST,
            &path,
            Some(("application/octet-stream", bytes)),
        )
        .await?;
        Ok(())
    }

    /// Remove every image in a bucket; returns how many Play reports deleted.
    pub async fn delete_all_images(
        &self,
        package: &str,
        edit_id: &str,
        locale: &str,
        image_type: ImageType,
    ) -> Result<usize> {
        #[derive(Deserialize)]
        struct Deleted {
            #[serde(default)]
            deleted: Vec<Image>,
        }
        let path = edits_path(
            package,
            &format!("/{edit_id}/listings/{locale}/{}", image_type.api_name()),
        );
        let deleted: Deleted = self.json(Method::DELETE, &path, None::<&()>).await?;
        Ok(deleted.deleted.len())
    }

    /// All one-time products of the app, as Play returns them, following
    /// `nextPageToken` so an app with more than one page is not truncated.
    pub async fn list_products(&self, package: &str) -> Result<Vec<serde_json::Value>> {
        let mut products = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let suffix = match &page_token {
                Some(token) => format!("?pageToken={token}"),
                None => String::new(),
            };
            // An app with no products can answer with an empty body, not `{}`.
            let text = self
                .send(Method::GET, &products_path(package, &suffix), None)
                .await?
                .text()
                .await?;
            if text.trim().is_empty() {
                break;
            }
            let page: serde_json::Value = serde_json::from_str(&text)?;
            if let Some(items) = page.get("oneTimeProducts").and_then(|v| v.as_array()) {
                products.extend(items.iter().cloned());
            }
            match page.get("nextPageToken").and_then(|v| v.as_str()) {
                Some(token) if !token.is_empty() => page_token = Some(token.to_string()),
                _ => break,
            }
        }
        Ok(products)
    }

    pub async fn get_product(&self, package: &str, product_id: &str) -> Result<serde_json::Value> {
        self.json(
            Method::GET,
            &products_path(package, &format!("/{product_id}")),
            None::<&()>,
        )
        .await
    }

    /// `pricing:convertRegionPrices`: one price in, a price for every region out.
    pub async fn convert_region_prices(
        &self,
        package: &str,
        price: &crate::product::Money,
    ) -> Result<serde_json::Value> {
        let path =
            format!("/androidpublisher/v3/applications/{package}/pricing:convertRegionPrices");
        self.json(
            Method::POST,
            &path,
            Some(&serde_json::json!({ "price": price.to_json() })),
        )
        .await
    }

    /// `onetimeproducts.patch` with `allowMissing`, so it creates the product
    /// when it does not exist. `regions_version` is the one the prices were
    /// converted against.
    pub async fn upsert_product(
        &self,
        package: &str,
        product_id: &str,
        body: &serde_json::Value,
        regions_version: &str,
    ) -> Result<serde_json::Value> {
        let path = format!(
            "{}?allowMissing=true&updateMask=listings,purchaseOptions&regionsVersion.version={}",
            product_patch_path(package, product_id),
            regions_version.replace('/', "%2F"),
        );
        self.json(Method::PATCH, &path, Some(body)).await
    }

    /// Activate one purchase option, which is what makes the product buyable.
    pub async fn activate_purchase_option(
        &self,
        package: &str,
        product_id: &str,
        purchase_option_id: &str,
    ) -> Result<serde_json::Value> {
        self.set_purchase_option_state(package, product_id, purchase_option_id, true)
            .await
    }

    /// Deactivate one purchase option, which stops the product being sold.
    pub async fn deactivate_purchase_option(
        &self,
        package: &str,
        product_id: &str,
        purchase_option_id: &str,
    ) -> Result<serde_json::Value> {
        self.set_purchase_option_state(package, product_id, purchase_option_id, false)
            .await
    }

    async fn set_purchase_option_state(
        &self,
        package: &str,
        product_id: &str,
        purchase_option_id: &str,
        activate: bool,
    ) -> Result<serde_json::Value> {
        let path = format!(
            "{}:batchUpdateStates",
            products_path(package, &format!("/{product_id}/purchaseOptions"))
        );
        let body = crate::product::purchase_option_state_body(
            package,
            product_id,
            purchase_option_id,
            activate,
        );
        self.json(Method::POST, &path, Some(&body)).await
    }

    /// `onetimeproducts.patch` without `allowMissing`: it changes the masked
    /// parts of a product that exists, and a mistyped id is a 404 instead of
    /// a new product. `regions_version` is sent when prices are involved.
    pub async fn update_product(
        &self,
        package: &str,
        product_id: &str,
        body: &serde_json::Value,
        update_mask: &str,
        regions_version: Option<&str>,
    ) -> Result<serde_json::Value> {
        let path = product_update_path(package, product_id, update_mask, regions_version);
        self.json(Method::PATCH, &path, Some(body)).await
    }

    pub async fn get_track(&self, package: &str, edit_id: &str, track: &str) -> Result<Track> {
        self.json(
            Method::GET,
            &edits_path(package, &format!("/{edit_id}/tracks/{track}")),
            None::<&()>,
        )
        .await
    }

    /// Every track the app has, with its releases.
    pub async fn list_tracks(&self, package: &str, edit_id: &str) -> Result<Vec<Track>> {
        #[derive(serde::Deserialize)]
        struct Tracks {
            #[serde(default)]
            tracks: Vec<Track>,
        }
        let list: Tracks = self
            .json(
                Method::GET,
                &edits_path(package, &format!("/{edit_id}/tracks")),
                None::<&()>,
            )
            .await?;
        Ok(list.tracks)
    }

    pub async fn update_track(&self, package: &str, edit_id: &str, track: &Track) -> Result<Track> {
        self.json(
            Method::PUT,
            &edits_path(package, &format!("/{edit_id}/tracks/{}", track.track)),
            Some(track),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_urls_follow_the_v3_layout() {
        assert_eq!(
            edits_path("xyz.smbcloud.SiGit", ""),
            "/androidpublisher/v3/applications/xyz.smbcloud.SiGit/edits"
        );
        assert_eq!(
            edits_path("p", "/E1:commit"),
            "/androidpublisher/v3/applications/p/edits/E1:commit"
        );
        assert_eq!(
            edits_path("p", "/E1/listings/en-US"),
            "/androidpublisher/v3/applications/p/edits/E1/listings/en-US"
        );
    }

    #[test]
    fn product_urls_use_the_one_time_products_resource() {
        assert_eq!(
            products_path("p", "/app.unlock"),
            "/androidpublisher/v3/applications/p/oneTimeProducts/app.unlock"
        );
    }

    #[test]
    fn the_product_patch_path_is_lowercase() {
        assert_eq!(
            product_patch_path("p", "app.unlock"),
            "/androidpublisher/v3/applications/p/onetimeproducts/app.unlock"
        );
    }

    #[test]
    fn uploads_use_the_upload_host_path_with_media_type() {
        assert_eq!(
            upload_path("p", "E1", "en-US", ImageType::PhoneScreenshots),
            "/upload/androidpublisher/v3/applications/p/edits/E1/listings/en-US/phoneScreenshots?uploadType=media"
        );
    }

    #[test]
    fn bundle_upload_urls() {
        assert_eq!(
            bundle_upload_path("p", "E1", false),
            "/upload/androidpublisher/v3/applications/p/edits/E1/bundles?uploadType=media"
        );
        assert_eq!(
            bundle_upload_path("p", "E1", true),
            "/upload/androidpublisher/v3/applications/p/edits/E1/bundles?uploadType=media&ackBundleInstallationWarning=true"
        );
        assert_eq!(
            deobfuscation_path("p", "E1", 42),
            "/upload/androidpublisher/v3/applications/p/edits/E1/apks/42/deobfuscationFiles/proguard?uploadType=media"
        );
    }

    #[test]
    fn uploaded_bundle_parses_play_response() {
        let b: UploadedBundle =
            serde_json::from_str(r#"{"versionCode":7,"sha256":"ab12"}"#).unwrap();
        assert_eq!(b.version_code, 7);
        assert_eq!(b.sha256, "ab12");
    }

    #[test]
    fn a_product_update_never_allows_missing() {
        let path = product_update_path(
            "p",
            "app.unlock",
            "listings,purchaseOptions",
            Some("2022/02"),
        );
        assert_eq!(
            path,
            "/androidpublisher/v3/applications/p/onetimeproducts/app.unlock?updateMask=listings,purchaseOptions&regionsVersion.version=2022%2F02"
        );
        assert!(!path.contains("allowMissing"));
        assert!(!product_update_path("p", "x", "listings", None).contains("regionsVersion"));
    }
}
