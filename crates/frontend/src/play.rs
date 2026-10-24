//! Google Play listing operations shared by `asc play` and the MCP tools.
//!
//! Play does everything through edits: open one, change things, `validate`,
//! then `commit`. Every function here opens exactly one edit and closes it
//! again, so a failure or a dry run leaves nothing behind. Reads open an edit
//! too (the API has no edit-less read for listings) and delete it after.

use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};
use smbcloud_ascapi_play::{
    png_dimensions, Image, ImageType, Listing, ListingFields, PlayClient, ServiceAccount, Track,
};

/// Resolve the Play client. `path` wins; then `PLAY_SERVICE_ACCOUNT_JSON_PATH`;
/// then the key contents inline in `PLAY_SERVICE_ACCOUNT_JSON`. Per call, like
/// the App Store Connect credentials, so an unconfigured MCP server still
/// lists its tools. The key itself never appears in an error.
pub fn play_client(path: Option<&Path>) -> Result<PlayClient, String> {
    let account = if let Some(path) = path {
        ServiceAccount::from_json_file(path)
    } else if let Ok(path) = std::env::var("PLAY_SERVICE_ACCOUNT_JSON_PATH") {
        ServiceAccount::from_json_file(path)
    } else if let Ok(json) = std::env::var("PLAY_SERVICE_ACCOUNT_JSON") {
        ServiceAccount::from_json(&json)
    } else {
        return Err(
            "no Google Play credentials: pass --play-service-account, or set \
                    PLAY_SERVICE_ACCOUNT_JSON_PATH (or PLAY_SERVICE_ACCOUNT_JSON)"
                .to_string(),
        );
    };
    account.map(PlayClient::new).map_err(|e| e.to_string())
}

/// Parse an image bucket name given as `phone-screenshots` or
/// `phoneScreenshots`.
pub fn image_type_from_str(value: &str) -> Result<ImageType, String> {
    match value.replace('-', "").to_ascii_lowercase().as_str() {
        "phonescreenshots" => Ok(ImageType::PhoneScreenshots),
        "seveninchscreenshots" => Ok(ImageType::SevenInchScreenshots),
        "teninchscreenshots" => Ok(ImageType::TenInchScreenshots),
        "featuregraphic" => Ok(ImageType::FeatureGraphic),
        "icon" => Ok(ImageType::Icon),
        other => Err(format!(
            "unknown image type {other:?}; use phone-screenshots, seven-inch-screenshots, \
             ten-inch-screenshots, feature-graphic or icon"
        )),
    }
}

fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// Open an edit, run the body inside it, then commit (after `validate`) when
/// `commit` is set, or delete the edit otherwise. A failure anywhere deletes
/// the edit and returns the first error. A macro rather than a function so the
/// body can borrow from the caller's scope.
macro_rules! in_edit {
    ($client:expr, $package:expr, $commit:expr, |$c:ident, $id:ident| $body:expr) => {{
        let $c: &PlayClient = $client;
        let edit = $c.insert_edit($package).await.map_err(err)?;
        let $id: &str = edit.id.as_str();
        let outcome: Result<_, String> = $body.await;
        finish($c, $package, $id, $commit, outcome).await
    }};
}

async fn finish<T>(
    client: &PlayClient,
    package: &str,
    edit_id: &str,
    commit: bool,
    outcome: Result<T, String>,
) -> Result<T, String> {
    let outcome = match outcome {
        Ok(value) if commit => {
            async {
                client.validate_edit(package, edit_id).await.map_err(err)?;
                client.commit_edit(package, edit_id).await.map_err(err)?;
                Ok(value)
            }
            .await
        }
        other => other,
    };
    if !(commit && outcome.is_ok()) {
        // Best effort: an edit that fails to delete simply expires.
        let _ = client.delete_edit(package, edit_id).await;
    }
    outcome
}

pub async fn read_listings(client: &PlayClient, package: &str) -> Result<Vec<Listing>, String> {
    in_edit!(client, package, false, |c, id| {
        Box::pin(async move { c.list_listings(package, id).await.map_err(err) })
    })
}

pub async fn read_images(
    client: &PlayClient,
    package: &str,
    locale: &str,
    image_type: ImageType,
) -> Result<Vec<Image>, String> {
    in_edit!(client, package, false, |c, id| {
        Box::pin(async move {
            c.list_images(package, id, locale, image_type)
                .await
                .map_err(err)
        })
    })
}

pub async fn read_track(client: &PlayClient, package: &str, track: &str) -> Result<Track, String> {
    in_edit!(client, package, false, |c, id| {
        Box::pin(async move { c.get_track(package, id, track).await.map_err(err) })
    })
}

/// The result of a listing update.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingUpdate {
    pub dry_run: bool,
    /// What was (or, on a dry run, would be) sent.
    pub sent: ListingFields,
    /// The listing before the change, if the locale existed.
    pub before: Option<Listing>,
    /// The listing read back after commit. `None` on a dry run.
    pub after: Option<Listing>,
}

pub async fn update_listing(
    client: &PlayClient,
    package: &str,
    locale: &str,
    fields: ListingFields,
    dry_run: bool,
) -> Result<ListingUpdate, String> {
    fields.validate()?;
    if fields.is_empty() {
        return Err("nothing to update: pass at least one listing field".to_string());
    }
    let sent = fields.clone();
    let before = in_edit!(client, package, !dry_run, |c, id| {
        Box::pin(async move {
            let before = c.get_listing(package, id, locale).await.ok();
            if !dry_run {
                c.patch_listing(package, id, locale, &fields)
                    .await
                    .map_err(err)?;
            }
            Ok(before)
        })
    })?;
    let after = if dry_run {
        None
    } else {
        let stored = in_edit!(client, package, false, |c, id| {
            Box::pin(async move { c.get_listing(package, id, locale).await.map_err(err) })
        })?;
        let wrong = sent.diff(&stored);
        if !wrong.is_empty() {
            return Err(format!(
                "committed, but the read-back differs in: {}",
                wrong.join(", ")
            ));
        }
        Some(stored)
    };
    Ok(ListingUpdate {
        dry_run,
        sent,
        before,
        after,
    })
}

/// One local file picked for upload.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageFile {
    pub path: PathBuf,
    pub bytes: usize,
    pub sha256: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    #[serde(skip)]
    content_type: &'static str,
    #[serde(skip)]
    data: Vec<u8>,
}

/// Read and lint a local image: PNG or JPEG, and for PNGs the pixel size
/// against the bucket's rules.
pub fn load_image(path: &Path, image_type: ImageType) -> Result<ImageFile, String> {
    let data = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    let content_type = match extension.as_deref() {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        _ => return Err(format!("{}: Play accepts only PNG or JPEG", path.display())),
    };
    let dims = png_dimensions(&data);
    if let Some((w, h)) = dims {
        image_type
            .check_dimensions(w, h)
            .map_err(|m| format!("{}: {m}", path.display()))?;
    } else if content_type == "image/png" {
        return Err(format!("{}: not a valid PNG", path.display()));
    }
    Ok(ImageFile {
        path: path.to_path_buf(),
        bytes: data.len(),
        sha256: hex(&Sha256::digest(&data)),
        width: dims.map(|d| d.0),
        height: dims.map(|d| d.1),
        content_type,
        data,
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImagesReplaced {
    pub dry_run: bool,
    pub image_type: String,
    pub locale: String,
    pub files: Vec<ImageFile>,
    /// How many images the bucket held before the replace.
    pub previously: usize,
    /// The bucket read back after commit. `None` on a dry run.
    pub after: Option<Vec<Image>>,
}

/// Delete the bucket's images and upload `paths` in order, inside one edit,
/// so a failed upload leaves the old set in place.
pub async fn replace_images(
    client: &PlayClient,
    package: &str,
    locale: &str,
    image_type: ImageType,
    paths: &[PathBuf],
    dry_run: bool,
) -> Result<ImagesReplaced, String> {
    if paths.is_empty() {
        return Err("no image files given".to_string());
    }
    let files = paths
        .iter()
        .map(|p| load_image(p, image_type))
        .collect::<Result<Vec<_>, _>>()?;
    let previously = in_edit!(client, package, !dry_run, |c, id| {
        let files = &files;
        Box::pin(async move {
            let previously = c
                .list_images(package, id, locale, image_type)
                .await
                .map_err(err)?
                .len();
            if !dry_run {
                c.delete_all_images(package, id, locale, image_type)
                    .await
                    .map_err(err)?;
                for file in files {
                    c.upload_image(
                        package,
                        id,
                        locale,
                        image_type,
                        file.content_type,
                        file.data.clone(),
                    )
                    .await
                    .map_err(|e| format!("{}: {e}", file.path.display()))?;
                }
            }
            Ok(previously)
        })
    })?;
    let after = if dry_run {
        None
    } else {
        let images = read_images(client, package, locale, image_type).await?;
        if images.len() != files.len() {
            return Err(format!(
                "committed, but the bucket holds {} images; expected {}",
                images.len(),
                files.len()
            ));
        }
        for (image, file) in images.iter().zip(&files) {
            if image.sha256.as_deref().is_some_and(|s| s != file.sha256) {
                return Err(format!(
                    "committed, but the stored copy of {} has a different sha256",
                    file.path.display()
                ));
            }
        }
        Some(images)
    };
    Ok(ImagesReplaced {
        dry_run,
        image_type: image_type.api_name().to_string(),
        locale: locale.to_string(),
        files,
        previously,
        after,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesSet {
    pub dry_run: bool,
    /// The track as it will be (dry run) or is (read back after commit).
    pub track: Track,
}

pub async fn set_track_notes(
    client: &PlayClient,
    package: &str,
    track_name: &str,
    version_code: &str,
    locale: &str,
    notes: &str,
    dry_run: bool,
) -> Result<NotesSet, String> {
    let planned = in_edit!(client, package, !dry_run, |c, id| {
        Box::pin(async move {
            let mut track = c.get_track(package, id, track_name).await.map_err(err)?;
            track.set_release_notes(version_code, locale, notes)?;
            if !dry_run {
                c.update_track(package, id, &track).await.map_err(err)?;
            }
            Ok(track)
        })
    })?;
    if dry_run {
        return Ok(NotesSet {
            dry_run,
            track: planned,
        });
    }
    let track = read_track(client, package, track_name).await?;
    let stored = track
        .releases
        .iter()
        .filter(|r| r.version_codes.iter().any(|c| c == version_code))
        .flat_map(|r| &r.release_notes)
        .any(|n| n.language == locale && n.text == notes);
    if !stored {
        return Err("committed, but the read-back release notes differ".to_string());
    }
    Ok(NotesSet { dry_run, track })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_types_parse_in_both_spellings() {
        assert_eq!(
            image_type_from_str("phone-screenshots"),
            Ok(ImageType::PhoneScreenshots)
        );
        assert_eq!(
            image_type_from_str("featureGraphic"),
            Ok(ImageType::FeatureGraphic)
        );
        assert!(image_type_from_str("tv").is_err());
    }

    #[test]
    fn load_image_rejects_other_formats() {
        let dir = std::env::temp_dir();
        let path = dir.join("ascapi-play-test.gif");
        std::fs::write(&path, b"GIF89a").unwrap();
        let error = load_image(&path, ImageType::PhoneScreenshots).unwrap_err();
        assert!(error.contains("PNG or JPEG"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn load_image_lints_png_size_for_the_bucket() {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&1000u32.to_be_bytes());
        png.extend_from_slice(&500u32.to_be_bytes());
        let path = std::env::temp_dir().join("ascapi-play-test.png");
        std::fs::write(&path, &png).unwrap();
        assert!(load_image(&path, ImageType::FeatureGraphic).is_err());
        assert!(load_image(&path, ImageType::PhoneScreenshots).is_ok());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn missing_credentials_name_the_options_without_a_key() {
        // Only meaningful when the variables are unset, as in CI.
        if std::env::var_os("PLAY_SERVICE_ACCOUNT_JSON_PATH").is_none()
            && std::env::var_os("PLAY_SERVICE_ACCOUNT_JSON").is_none()
        {
            let error = play_client(None).err().unwrap();
            assert!(error.contains("PLAY_SERVICE_ACCOUNT_JSON_PATH"));
        }
    }
}
