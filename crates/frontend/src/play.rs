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
    png_dimensions,
    product::{
        check_applied, existing_price, is_billing_refusal, pick_purchase_option, product_body,
        purchase_option_state, regions_version, BILLING_HINT,
    },
    Details, Image, ImageType, Listing, ListingFields, Money, NewProduct, PlayClient,
    ProductChanges, RolloutAction, ServiceAccount, Track,
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
    ($client:expr, $package:expr, $commit:expr, |$c:ident, $id:ident| $body:expr) => {
        in_edit!($client, $package, $commit, false, |$c, $id| $body)
    };
    ($client:expr, $package:expr, $commit:expr, $unsent:expr, |$c:ident, $id:ident| $body:expr) => {{
        let $c: &PlayClient = $client;
        let edit = $c.insert_edit($package).await.map_err(err)?;
        let $id: &str = edit.id.as_str();
        let outcome: Result<_, String> = $body.await;
        finish($c, $package, $id, $commit, $unsent, outcome).await
    }};
}

async fn finish<T>(
    client: &PlayClient,
    package: &str,
    edit_id: &str,
    commit: bool,
    changes_not_sent_for_review: bool,
    outcome: Result<T, String>,
) -> Result<T, String> {
    let outcome = match outcome {
        Ok(value) if commit => {
            async {
                client.validate_edit(package, edit_id).await.map_err(err)?;
                client
                    .commit_edit(package, edit_id, changes_not_sent_for_review)
                    .await
                    .map_err(err)?;
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

pub async fn read_details(client: &PlayClient, package: &str) -> Result<Details, String> {
    in_edit!(client, package, false, |c, id| {
        Box::pin(async move { c.get_details(package, id).await.map_err(err) })
    })
}

/// The result of a details update.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailsUpdate {
    pub dry_run: bool,
    pub sent: Details,
    pub before: Details,
    /// Read back from a fresh edit after commit. `None` on a dry run.
    pub after: Option<Details>,
}

pub async fn update_details(
    client: &PlayClient,
    package: &str,
    details: Details,
    dry_run: bool,
) -> Result<DetailsUpdate, String> {
    if details.is_empty() {
        return Err("nothing to update: pass at least one details field".to_string());
    }
    let sent = details.clone();
    let before = in_edit!(client, package, !dry_run, |c, id| {
        Box::pin(async move {
            let before = c.get_details(package, id).await.map_err(err)?;
            if !dry_run {
                c.patch_details(package, id, &details).await.map_err(err)?;
            }
            Ok(before)
        })
    })?;
    let after = if dry_run {
        None
    } else {
        let stored = read_details(client, package).await?;
        let wrong = sent.diff(&stored);
        if !wrong.is_empty() {
            return Err(format!(
                "committed, but the read-back differs in: {}",
                wrong.join(", ")
            ));
        }
        Some(stored)
    };
    Ok(DetailsUpdate {
        dry_run,
        sent,
        before,
        after,
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

#[allow(clippy::too_many_arguments)]
pub async fn set_track_notes(
    client: &PlayClient,
    package: &str,
    track_name: &str,
    version_code: &str,
    locale: &str,
    notes: &str,
    changes_not_sent_for_review: bool,
    dry_run: bool,
) -> Result<NotesSet, String> {
    let planned = in_edit!(
        client,
        package,
        !dry_run,
        changes_not_sent_for_review,
        |c, id| {
            Box::pin(async move {
                let mut track = c.get_track(package, id, track_name).await.map_err(err)?;
                track.set_release_notes(version_code, locale, notes)?;
                if !dry_run {
                    c.update_track(package, id, &track).await.map_err(err)?;
                }
                Ok(track)
            })
        }
    )?;
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

/// The manifest every Android App Bundle carries.
const BUNDLE_MANIFEST: &str = "base/manifest/AndroidManifest.xml";

/// A bundle checked on disk, ready to upload.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleFile {
    pub path: PathBuf,
    pub bytes: u64,
    /// Lowercase hex SHA-256 of the file, to compare with Play's.
    pub sha256: String,
}

/// Check a file is an `.aab` (a zip with `base/manifest/AndroidManifest.xml`)
/// and hash it. Nothing is sent.
pub fn load_bundle(path: &Path) -> Result<(BundleFile, Vec<u8>), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes.as_slice()))
        .map_err(|_| format!("{} is not a zip archive, so not an .aab", path.display()))?;
    if archive.by_name(BUNDLE_MANIFEST).is_err() {
        return Err(format!(
            "{} has no {BUNDLE_MANIFEST}; it is a zip but not an Android App Bundle",
            path.display()
        ));
    }
    let file = BundleFile {
        path: path.to_path_buf(),
        bytes: bytes.len() as u64,
        sha256: hex(&Sha256::digest(&bytes)),
    };
    Ok((file, bytes))
}

fn check_upload_status(status: &str) -> Result<(), String> {
    match status {
        "draft" | "completed" => Ok(()),
        other => Err(format!(
            "status {other:?} is not supported for an upload; use draft or completed"
        )),
    }
}

/// What `bundles upload --dry-run` reports. A dry run cannot know the version
/// code: Play assigns it on upload.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundlePlan {
    pub dry_run: bool,
    pub package: String,
    pub track: String,
    pub status: String,
    pub bundle: BundleFile,
    pub mapping_file: Option<PathBuf>,
    pub ack_install_warning: bool,
}

/// The local half of `bundles upload`: everything that can be checked without
/// sending anything.
pub fn plan_bundle_upload(
    package: &str,
    track: &str,
    file: &Path,
    status: &str,
    mapping_file: Option<&Path>,
    ack_install_warning: bool,
) -> Result<BundlePlan, String> {
    check_upload_status(status)?;
    let (bundle, _) = load_bundle(file)?;
    if let Some(mapping) = mapping_file {
        if !mapping.is_file() {
            return Err(format!("mapping file {} does not exist", mapping.display()));
        }
    }
    Ok(BundlePlan {
        dry_run: true,
        package: package.to_string(),
        track: track.to_string(),
        status: status.to_string(),
        bundle,
        mapping_file: mapping_file.map(Path::to_path_buf),
        ack_install_warning,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleUploaded {
    /// Assigned by Play; use it with `tracks set-notes` or `tracks promote`.
    pub version_code: u64,
    pub sha256: String,
    pub status: String,
    pub mapping_uploaded: bool,
    /// The track as read back after the commit.
    pub track: Track,
}

/// Whether the read-back track shows `code` on a release at `status`.
fn bundle_landed(track: &Track, code: &str, status: &str) -> bool {
    track
        .releases
        .iter()
        .any(|r| r.version_codes.iter().any(|c| c == code) && r.status.as_deref() == Some(status))
}

/// Upload an `.aab`, optionally its mapping file, and put it on `track_name`
/// as a new release, in one edit. Play's SHA-256 must match the local one, and
/// the track is read back after the commit. The version code is consumed on
/// upload, so a failure after that point still uses one up.
pub async fn upload_bundle(
    client: &PlayClient,
    package: &str,
    track_name: &str,
    file: &Path,
    status: &str,
    mapping_file: Option<&Path>,
    ack_install_warning: bool,
) -> Result<BundleUploaded, String> {
    check_upload_status(status)?;
    let (bundle, bytes) = load_bundle(file)?;
    let mapping = match mapping_file {
        Some(p) => Some(std::fs::read(p).map_err(|e| format!("cannot read {}: {e}", p.display()))?),
        None => None,
    };
    let mapping_uploaded = mapping.is_some();
    let local_sha = bundle.sha256;
    let expected = local_sha.as_str();
    let version_code = in_edit!(client, package, true, |c, id| {
        Box::pin(async move {
            let uploaded = c
                .upload_bundle(package, id, bytes, ack_install_warning)
                .await
                .map_err(err)?;
            if !uploaded.sha256.eq_ignore_ascii_case(expected) {
                return Err(format!(
                    "Play stored version code {} with SHA-256 {}, but the local file is {expected}; \
                     nothing was committed",
                    uploaded.version_code, uploaded.sha256
                ));
            }
            if let Some(mapping) = mapping {
                c.upload_deobfuscation(package, id, uploaded.version_code, mapping)
                    .await
                    .map_err(err)?;
            }
            let code = uploaded.version_code.to_string();
            let track = c.get_track(package, id, track_name).await.map_err(err)?;
            let planned = track.place_uploaded(&code, status)?;
            c.update_track(package, id, &planned).await.map_err(err)?;
            Ok(uploaded.version_code)
        })
    })?;
    let track = read_track(client, package, track_name).await?;
    let code = version_code.to_string();
    let landed = bundle_landed(&track, &code, status);
    if !landed {
        return Err(format!(
            "committed, but {track_name} does not show version code {version_code} as {status}"
        ));
    }
    Ok(BundleUploaded {
        version_code,
        sha256: local_sha,
        status: status.to_string(),
        mapping_uploaded,
        track,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Promoted {
    pub dry_run: bool,
    /// The target already held the code at this status and fraction.
    pub noop: bool,
    /// The source track as read.
    pub from: Track,
    /// The target track as it will be (dry run or no-op) or is (read back
    /// after commit).
    pub to: Track,
}

/// Promote `version_code` from one track to another inside one edit: get both
/// tracks, plan, update the target, validate and commit. A dry run deletes the
/// edit. After the commit the target is read back and compared.
#[allow(clippy::too_many_arguments)]
pub async fn promote_track(
    client: &PlayClient,
    package: &str,
    from_track: &str,
    to_track: &str,
    version_code: &str,
    user_fraction: f64,
    changes_not_sent_for_review: bool,
    dry_run: bool,
) -> Result<Promoted, String> {
    if from_track == to_track {
        return Err("--from and --to are the same track".to_string());
    }
    let (from, plan) = in_edit!(
        client,
        package,
        !dry_run,
        changes_not_sent_for_review,
        |c, id| {
            Box::pin(async move {
                let source = c.get_track(package, id, from_track).await.map_err(err)?;
                let target = c.get_track(package, id, to_track).await.map_err(err)?;
                let plan = target.promote_from(&source, version_code, user_fraction)?;
                if !dry_run && !plan.noop {
                    c.update_track(package, id, &plan.target)
                        .await
                        .map_err(err)?;
                }
                Ok((source, plan))
            })
        }
    )?;
    if dry_run || plan.noop {
        return Ok(Promoted {
            dry_run,
            noop: plan.noop,
            from,
            to: plan.target,
        });
    }
    let stored = read_track(client, package, to_track).await?;
    let full = user_fraction >= 1.0;
    let landed = stored
        .releases
        .iter()
        .filter(|r| r.version_codes.iter().any(|c| c == version_code))
        .any(|r| {
            if full {
                r.status.as_deref() == Some("completed")
            } else {
                r.status.as_deref() == Some("inProgress")
                    && r.user_fraction
                        .is_some_and(|f| (f - user_fraction).abs() < 1e-9)
            }
        });
    if !landed {
        return Err(format!(
            "committed, but {to_track} does not show version code {version_code} at the \
             requested status and fraction"
        ));
    }
    Ok(Promoted {
        dry_run,
        noop: false,
        from,
        to: stored,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RolledOut {
    pub dry_run: bool,
    /// The release already had the requested status and fraction.
    pub noop: bool,
    /// The track as it will be (dry run or no-op) or is (read back after
    /// commit).
    pub track: Track,
}

/// Change the rollout of `version_code` on `track_name` inside one edit: get
/// the track, plan, update it, validate and commit. A fraction never goes
/// down. A dry run deletes the edit. After the commit the track is read back
/// and compared.
pub async fn rollout_track(
    client: &PlayClient,
    package: &str,
    track_name: &str,
    version_code: &str,
    action: RolloutAction,
    changes_not_sent_for_review: bool,
    dry_run: bool,
) -> Result<RolledOut, String> {
    let plan = in_edit!(
        client,
        package,
        !dry_run,
        changes_not_sent_for_review,
        |c, id| {
            Box::pin(async move {
                let track = c.get_track(package, id, track_name).await.map_err(err)?;
                let plan = track.rollout(version_code, action)?;
                if !dry_run && !plan.noop {
                    c.update_track(package, id, &plan.target)
                        .await
                        .map_err(err)?;
                }
                Ok(plan)
            })
        }
    )?;
    if dry_run || plan.noop {
        return Ok(RolledOut {
            dry_run,
            noop: plan.noop,
            track: plan.target,
        });
    }
    let stored = read_track(client, package, track_name).await?;
    let landed = stored
        .releases
        .iter()
        .find(|r| r.version_codes.iter().any(|c| c == version_code))
        .is_some_and(|r| match action {
            // A halt keeps the fraction the rollout reached, and a later resume
            // is checked against it, so a halt that dropped it did not land.
            RolloutAction::Halt => {
                let planned = plan
                    .target
                    .releases
                    .iter()
                    .find(|p| p.version_codes.iter().any(|c| c == version_code))
                    .and_then(|p| p.user_fraction);
                r.status.as_deref() == Some("halted")
                    && match (planned, r.user_fraction) {
                        (Some(want), Some(got)) => (want - got).abs() < 1e-9,
                        (want, got) => want.is_none() && got.is_none(),
                    }
            }
            RolloutAction::Complete => r.status.as_deref() == Some("completed"),
            RolloutAction::Fraction(f) if f >= 1.0 => r.status.as_deref() == Some("completed"),
            RolloutAction::Fraction(f) => {
                r.status.as_deref() == Some("inProgress")
                    && r.user_fraction.is_some_and(|got| (got - f).abs() < 1e-9)
            }
        });
    if !landed {
        return Err(format!(
            "committed, but {track_name} does not show version code {version_code} at the \
             requested status and fraction"
        ));
    }
    Ok(RolledOut {
        dry_run,
        noop: false,
        track: stored,
    })
}

/// The highest version code on any track of `package`, read in a throwaway
/// edit. Play refuses an upload whose code is not above it, so this is the
/// number to stay above.
pub async fn max_version_code(client: &PlayClient, package: &str) -> Result<u64, String> {
    let tracks = in_edit!(client, package, false, |c, id| {
        Box::pin(async move { c.list_tracks(package, id).await.map_err(err) })
    })?;
    tracks
        .iter()
        .filter_map(Track::max_version_code)
        .max()
        .ok_or_else(|| format!("no track of {package} carries a version code"))
}

/// All one-time products of the app.
pub async fn read_products(
    client: &PlayClient,
    package: &str,
) -> Result<Vec<serde_json::Value>, String> {
    client.list_products(package).await.map_err(err)
}

/// One one-time product by id.
pub async fn read_product(
    client: &PlayClient,
    package: &str,
    product_id: &str,
) -> Result<serde_json::Value, String> {
    client.get_product(package, product_id).await.map_err(err)
}

/// An error message, with a line on uploading a BILLING build when Play's
/// refusal looks like it is about billing.
fn with_billing_hint(error: smbcloud_ascapi_play::Error) -> String {
    if is_billing_refusal(&error) {
        format!("{error}\n{BILLING_HINT}")
    } else {
        err(error)
    }
}

/// The result of creating a one-time product.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductCreated {
    pub dry_run: bool,
    /// The product body that is sent (dry run) or was sent.
    pub sent: serde_json::Value,
    /// The product as Play stores it, read back after activation. `None` on a
    /// dry run.
    pub stored: Option<serde_json::Value>,
}

/// Create a one-time product priced in one currency and converted to every
/// region, then activate its purchase option. Refuses a product id that
/// already exists, so a rerun never overwrites a price. A dry run converts
/// prices (a read) and sends nothing.
pub async fn create_product(
    client: &PlayClient,
    product: &NewProduct,
    price: &Money,
    dry_run: bool,
) -> Result<ProductCreated, String> {
    product.check().map_err(err)?;
    let package = product.package.as_str();
    let id = product.product_id.as_str();
    match client.get_product(package, id).await {
        Ok(_) => {
            return Err(format!(
                "{id} already exists in {package}; it was left alone"
            ))
        }
        Err(smbcloud_ascapi_play::Error::Api { status: 404, .. }) => {}
        Err(e) => return Err(err(e)),
    }
    let converted = client
        .convert_region_prices(package, price)
        .await
        .map_err(err)?;
    let body = product_body(product, &converted).map_err(err)?;
    if dry_run {
        return Ok(ProductCreated {
            dry_run,
            sent: body,
            stored: None,
        });
    }
    let version = regions_version(&converted).map_err(err)?;
    client
        .upsert_product(package, id, &body, &version)
        .await
        .map_err(with_billing_hint)?;
    client
        .activate_purchase_option(package, id, &product.purchase_option_id)
        .await
        .map_err(|e| {
            format!(
                "created {id}, but activating its purchase option failed: {}",
                with_billing_hint(e)
            )
        })?;
    let stored = client
        .get_product(package, id)
        .await
        .map_err(|e| format!("created {id}, but reading it back failed: {}", err(e)))?;
    let state = stored
        .pointer("/purchaseOptions/0/state")
        .and_then(|v| v.as_str());
    if state != Some("ACTIVE") {
        return Err(format!(
            "created {id}, but its purchase option is {} and not ACTIVE",
            state.unwrap_or("missing")
        ));
    }
    Ok(ProductCreated {
        dry_run,
        sent: body,
        stored: Some(stored),
    })
}

/// The result of activating or deactivating a purchase option.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseOptionStateChanged {
    pub dry_run: bool,
    pub purchase_option_id: String,
    /// The state before.
    pub from: Option<String>,
    /// The state after (read back), or the one that would be set on a dry run.
    pub to: String,
    /// It already was in the requested state, so nothing was sent.
    pub noop: bool,
}

/// Activate or deactivate one purchase option, then read the state back. With
/// no `purchase_option_id`, the product's only option is used; several options
/// make that an error.
pub async fn set_purchase_option_active(
    client: &PlayClient,
    package: &str,
    product_id: &str,
    purchase_option_id: Option<&str>,
    activate: bool,
    dry_run: bool,
) -> Result<PurchaseOptionStateChanged, String> {
    let product = client.get_product(package, product_id).await.map_err(err)?;
    let option = pick_purchase_option(&product, purchase_option_id).map_err(err)?;
    let from = purchase_option_state(&product, &option).map(str::to_string);
    let wanted = |state: &str| {
        if activate {
            state == "ACTIVE"
        } else {
            state.starts_with("INACTIVE")
        }
    };
    let target = if activate { "ACTIVE" } else { "INACTIVE" }.to_string();
    if from.as_deref().is_some_and(wanted) {
        return Ok(PurchaseOptionStateChanged {
            dry_run,
            purchase_option_id: option,
            to: from.clone().unwrap_or(target),
            from,
            noop: true,
        });
    }
    if dry_run {
        return Ok(PurchaseOptionStateChanged {
            dry_run,
            purchase_option_id: option,
            from,
            to: target,
            noop: false,
        });
    }
    if activate {
        client
            .activate_purchase_option(package, product_id, &option)
            .await
    } else {
        client
            .deactivate_purchase_option(package, product_id, &option)
            .await
    }
    .map_err(with_billing_hint)?;
    let stored = client.get_product(package, product_id).await.map_err(|e| {
        format!(
            "changed the state of {product_id}, but reading it back failed: {}",
            err(e)
        )
    })?;
    let state = purchase_option_state(&stored, &option).unwrap_or("missing");
    if !wanted(state) {
        return Err(format!(
            "asked for {target} on {product_id}/{option}, but it reads back as {state}"
        ));
    }
    Ok(PurchaseOptionStateChanged {
        dry_run,
        purchase_option_id: option,
        from,
        to: state.to_string(),
        noop: false,
    })
}

/// The result of updating a one-time product.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductUpdated {
    pub dry_run: bool,
    /// The `updateMask` sent.
    pub update_mask: String,
    /// The patch body that is sent (dry run) or was sent.
    pub sent: serde_json::Value,
    /// The product as read back after the update. `None` on a dry run.
    pub stored: Option<serde_json::Value>,
}

/// Change a product's listing text and/or price. Reads the product, changes
/// only what was given and patches with an `updateMask` and without
/// `allowMissing`, so a mistyped id fails instead of creating a product. A
/// dry run prints the body and sends nothing (the price conversion, if any,
/// is a read).
pub async fn update_product(
    client: &PlayClient,
    package: &str,
    product_id: &str,
    changes: &ProductChanges,
    dry_run: bool,
) -> Result<ProductUpdated, String> {
    let product = client.get_product(package, product_id).await.map_err(err)?;
    // Play wants the regions version quoted back on any write. Prices give it
    // through the conversion; for a text-only change, convert a price the
    // product already has, which sends nothing.
    let probe = match (&changes.price, changes.touches_listings()) {
        (Some(price), _) => Some(price.clone()),
        (None, true) => Some(existing_price(&product).ok_or_else(|| {
            "the product has no price to read a regions version from; \
             pass --price to set one"
                .to_string()
        })?),
        (None, false) => None,
    };
    let converted = match &probe {
        Some(price) => Some(
            client
                .convert_region_prices(package, price)
                .await
                .map_err(err)?,
        ),
        None => None,
    };
    let body = smbcloud_ascapi_play::product::update_body(
        &product,
        changes,
        changes.price.as_ref().and(converted.as_ref()),
    )
    .map_err(err)?;
    let mask = changes.update_mask().to_string();
    if dry_run {
        return Ok(ProductUpdated {
            dry_run,
            update_mask: mask,
            sent: body,
            stored: None,
        });
    }
    let version = match converted.as_ref() {
        Some(c) => Some(regions_version(c).map_err(err)?),
        None => None,
    };
    client
        .update_product(package, product_id, &body, &mask, version.as_deref())
        .await
        .map_err(err)?;
    let stored = client.get_product(package, product_id).await.map_err(|e| {
        format!(
            "updated {product_id}, but reading it back failed: {}",
            err(e)
        )
    })?;
    check_applied(&stored, &body)?;
    Ok(ProductUpdated {
        dry_run,
        update_mask: mask,
        sent: body,
        stored: Some(stored),
    })
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

    fn zip_with(dir: &Path, name: &str, entries: &[&str]) -> PathBuf {
        let path = dir.join(name);
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for entry in entries {
            zip.start_file(*entry, zip::write::SimpleFileOptions::default())
                .unwrap();
            std::io::Write::write_all(&mut zip, b"x").unwrap();
        }
        zip.finish().unwrap();
        path
    }

    #[test]
    fn load_bundle_accepts_a_bundle_and_rejects_the_rest() {
        let dir = std::env::temp_dir().join(format!("asc-aab-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = zip_with(&dir, "ok.aab", &[BUNDLE_MANIFEST]);
        let (file, bytes) = load_bundle(&good).unwrap();
        assert_eq!(file.bytes as usize, bytes.len());
        assert_eq!(file.sha256.len(), 64);

        let bare = zip_with(&dir, "bare.aab", &["other.txt"]);
        assert!(load_bundle(&bare)
            .unwrap_err()
            .contains("not an Android App Bundle"));

        let text = dir.join("a.aab");
        std::fs::write(&text, b"not a zip").unwrap();
        assert!(load_bundle(&text).unwrap_err().contains("not a zip"));
        assert!(load_bundle(&dir.join("missing.aab")).is_err());

        let missing_mapping = dir.join("nope.txt");
        assert!(plan_bundle_upload(
            "p",
            "internal",
            &good,
            "draft",
            Some(&missing_mapping),
            false
        )
        .unwrap_err()
        .contains("mapping file"));
        assert!(plan_bundle_upload("p", "internal", &good, "inProgress", None, false).is_err());
        let plan = plan_bundle_upload("p", "internal", &good, "draft", None, false).unwrap();
        assert!(plan.dry_run);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn read_back_must_show_the_code_at_the_status() {
        let track: Track = serde_json::from_str(
            r#"{"track":"internal","releases":[{"versionCodes":["8"],"status":"draft"}]}"#,
        )
        .unwrap();
        assert!(bundle_landed(&track, "8", "draft"));
        assert!(!bundle_landed(&track, "8", "completed"));
        assert!(!bundle_landed(&track, "9", "draft"));
    }

    #[test]
    fn billing_refusals_get_the_upload_hint() {
        let refusal = smbcloud_ascapi_play::Error::Api {
            status: 400,
            detail: "no Billing permission".into(),
        };
        let message = with_billing_hint(refusal);
        assert!(message.contains("asc play bundles upload"));
        let other = smbcloud_ascapi_play::Error::Api {
            status: 400,
            detail: "bad price".into(),
        };
        assert!(!with_billing_hint(other).contains("bundles upload"));
    }
}
