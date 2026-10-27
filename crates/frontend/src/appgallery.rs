//! Huawei AppGallery listing operations shared by `asc appgallery` and the
//! MCP tools.

use std::path::Path;

use serde::Serialize;
use smbcloud_ascapi_appgallery::upload::sha256_hex;
use smbcloud_ascapi_appgallery::{
    AppGalleryClient, AppInfo, AppInfoFields, AppInfoResponse, FileBind, FileType,
    ImageOrientation, LanguageFields, LanguageInfo, PackageList, PackageState, PhasedRelease,
    ReleaseTimeChange, ServiceAccount, Submit,
};

/// Resolve the AppGallery client. `path` wins; then
/// `APPGALLERY_SERVICE_ACCOUNT_JSON_PATH`; then the credential contents inline
/// in `APPGALLERY_SERVICE_ACCOUNT_JSON`. `APPGALLERY_CLIENT_ID` (or
/// `client_id`) adds the `client_id` header. Per call, so an unconfigured MCP
/// server still lists its tools. The private key never appears in an error.
pub fn appgallery_client(
    path: Option<&Path>,
    client_id: Option<&str>,
) -> Result<AppGalleryClient, String> {
    let account = if let Some(path) = path {
        ServiceAccount::from_json_file(path)
    } else if let Ok(path) = std::env::var("APPGALLERY_SERVICE_ACCOUNT_JSON_PATH") {
        ServiceAccount::from_json_file(path)
    } else if let Ok(json) = std::env::var("APPGALLERY_SERVICE_ACCOUNT_JSON") {
        ServiceAccount::from_json(&json)
    } else {
        return Err("no AppGallery credentials: pass --service-account, or set \
                    APPGALLERY_SERVICE_ACCOUNT_JSON_PATH (or APPGALLERY_SERVICE_ACCOUNT_JSON)"
            .to_string());
    }
    .map_err(|e| e.to_string())?;
    let client = AppGalleryClient::new(account);
    let client_id = client_id
        .map(str::to_string)
        .or_else(|| std::env::var("APPGALLERY_CLIENT_ID").ok());
    Ok(match client_id {
        Some(id) if !id.is_empty() => client.with_client_id(id),
        _ => client,
    })
}

fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// An app id is all digits; anything else is taken as a package name and
/// looked up.
pub async fn resolve_app_id(client: &AppGalleryClient, app: &str) -> Result<String, String> {
    if !app.is_empty() && app.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(app.to_string());
    }
    let ids = client.app_ids_for_packages(&[app]).await.map_err(err)?;
    match ids.as_slice() {
        [one] => Ok(one.value.clone()),
        [] => Err(format!("no AppGallery app found for package {app:?}")),
        _ => Err(format!("more than one app matches package {app:?}")),
    }
}

/// App info with every language's text, or one language's.
pub async fn read_listings(
    client: &AppGalleryClient,
    app: &str,
    lang: Option<&str>,
) -> Result<AppInfoResponse, String> {
    let app_id = resolve_app_id(client, app).await?;
    client.app_info(&app_id, lang).await.map_err(err)
}

#[derive(Debug, Serialize)]
pub struct LanguageUpdate {
    pub dry_run: bool,
    pub app_id: String,
    pub sent: LanguageFields,
    /// The language as it was before the change; `None` when it is new.
    pub before: Option<LanguageInfo>,
    /// Read back after the write; `None` on a dry run.
    pub after: Option<LanguageInfo>,
}

fn differs(sent: &LanguageFields, stored: &LanguageInfo) -> Vec<&'static str> {
    let mut wrong = Vec::new();
    let mut check = |name: &'static str, want: &Option<String>, got: &Option<String>| {
        if want.is_some() && want != got {
            wrong.push(name);
        }
    };
    check("appName", &sent.app_name, &stored.app_name);
    check("appDesc", &sent.app_desc, &stored.app_desc);
    check("briefInfo", &sent.brief_info, &stored.brief_info);
    check("newFeatures", &sent.new_features, &stored.new_features);
    wrong
}

/// Update (or add) one language's text, then read it back. With `dry_run`,
/// nothing is sent: the result shows the body and the current text.
pub async fn update_language(
    client: &AppGalleryClient,
    app: &str,
    fields: LanguageFields,
    release_type: Option<u8>,
    dry_run: bool,
) -> Result<LanguageUpdate, String> {
    fields.validate().map_err(err)?;
    if fields.app_name.is_none()
        && fields.app_desc.is_none()
        && fields.brief_info.is_none()
        && fields.new_features.is_none()
    {
        return Err("nothing to update: pass at least one text field".to_string());
    }
    let app_id = resolve_app_id(client, app).await?;
    let language_of = |info: AppInfoResponse| info.languages.into_iter().next();
    let before = client
        .app_info(&app_id, Some(&fields.lang))
        .await
        .ok()
        .and_then(language_of);
    if dry_run {
        return Ok(LanguageUpdate {
            dry_run,
            app_id,
            sent: fields,
            before,
            after: None,
        });
    }
    client
        .update_language_info(&app_id, &fields, release_type)
        .await
        .map_err(err)?;
    let after = client
        .app_info(&app_id, Some(&fields.lang))
        .await
        .map_err(err)?;
    let stored = language_of(after)
        .ok_or_else(|| "updated, but the language is missing on read-back".to_string())?;
    let wrong = differs(&fields, &stored);
    if !wrong.is_empty() {
        return Err(format!(
            "updated, but the read-back differs in: {}",
            wrong.join(", ")
        ));
    }
    Ok(LanguageUpdate {
        dry_run,
        app_id,
        sent: fields,
        before,
        after: Some(stored),
    })
}

#[derive(Debug, Serialize)]
pub struct BasicInfoUpdate {
    pub dry_run: bool,
    pub app_id: String,
    pub sent: AppInfoFields,
    /// Read back after the write; `None` on a dry run.
    pub after: Option<AppInfo>,
}

/// Update basic information (privacy policy link and similar), then read the
/// privacy link back.
pub async fn update_basic_info(
    client: &AppGalleryClient,
    app: &str,
    fields: AppInfoFields,
    release_type: Option<u8>,
    dry_run: bool,
) -> Result<BasicInfoUpdate, String> {
    fields.validate().map_err(err)?;
    let app_id = resolve_app_id(client, app).await?;
    if dry_run {
        return Ok(BasicInfoUpdate {
            dry_run,
            app_id,
            sent: fields,
            after: None,
        });
    }
    client
        .update_app_info(&app_id, &fields, release_type)
        .await
        .map_err(err)?;
    let after = client
        .app_info(&app_id, None)
        .await
        .map_err(err)?
        .app_info
        .ok_or_else(|| "updated, but app info is missing on read-back".to_string())?;
    if after.privacy_policy.as_deref() != Some(fields.privacy_policy.as_str()) {
        return Err("updated, but the privacy policy differs on read-back".to_string());
    }
    Ok(BasicInfoUpdate {
        dry_run,
        app_id,
        sent: fields,
        after: Some(after),
    })
}

/// Screenshots per language: the AGC console asks for 3 to 8 (the API
/// reference allows up to 10 addresses, but the console rejects the listing).
pub const MIN_SCREENSHOTS: usize = 3;
pub const MAX_SCREENSHOTS: usize = 8;

#[derive(Debug, Serialize)]
pub struct PlannedFile {
    pub path: String,
    pub bytes: usize,
    pub sha256: String,
    /// The server's object id; `None` on a dry run.
    pub object_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct FileUpload {
    pub dry_run: bool,
    pub app_id: String,
    pub file_type: &'static str,
    pub lang: Option<String>,
    pub files: Vec<PlannedFile>,
    /// Package ids, when a package was added.
    pub pkg_version: Vec<String>,
    /// True when binding cleared the language's text and it was written back.
    pub restored_text: bool,
}

/// Options for [`upload_files`] beyond the files themselves.
#[derive(Debug, Clone, Default)]
pub struct UploadOptions {
    pub lang: Option<String>,
    pub image_orientation: Option<ImageOrientation>,
    pub video_orientation: Option<ImageOrientation>,
    pub sensitive_permission_desc: Option<String>,
    pub device_type: Option<u8>,
    /// Sent as `chineseMainlandFlag`; false matches siGit's "not in the
    /// Chinese mainland" choice. Mandatory for developers registered
    /// outside the mainland.
    pub chinese_mainland: bool,
}

/// The CLI name of a file type.
pub fn file_type_name(kind: FileType) -> &'static str {
    match kind {
        FileType::Icon => "icon",
        FileType::IntroVideo => "intro-video",
        FileType::Screenshots => "screenshots",
        FileType::PromoVideo => "promo-video",
        FileType::PromoImage => "promo-image",
        FileType::Package => "package",
        FileType::AgentCertificate => "agent-certificate",
        FileType::ElectronicCertificate => "electronic-certificate",
        FileType::CultureLicenseScreenshot => "culture-license-screenshot",
        FileType::CultureLicense => "culture-license",
        FileType::VrCover => "vr-cover",
        FileType::VrScreenshots => "vr-screenshots",
        FileType::VrPromoImage => "vr-promo-image",
        FileType::VrCoverLayered => "vr-cover-layered",
        FileType::VrPromo4x3 => "vr-promo-4x3",
        FileType::VrPromo1x1 => "vr-promo-1x1",
        FileType::VrPanorama => "vr-panorama",
    }
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default()
}

fn is_image(path: &Path) -> bool {
    matches!(extension(path).as_str(), "png" | "jpg" | "jpeg")
}

/// Check the files suit the type before anything is read or sent.
fn check_files(kind: FileType, paths: &[std::path::PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Err("pass at least one file".to_string());
    }
    let one = |what: &str| {
        if paths.len() == 1 {
            Ok(())
        } else {
            Err(format!("{what} is exactly one file"))
        }
    };
    match kind {
        FileType::Icon => one("an icon")?,
        FileType::Package => one("a package")?,
        FileType::Screenshots | FileType::VrScreenshots
            if !(MIN_SCREENSHOTS..=MAX_SCREENSHOTS).contains(&paths.len()) =>
        {
            return Err(format!(
                "{MIN_SCREENSHOTS} to {MAX_SCREENSHOTS} screenshots per language; binding replaces the whole set"
            ))
        }
        FileType::IntroVideo if !paths.len().is_multiple_of(2) || paths.len() > 6 => {
            return Err("intro videos go as video, poster pairs, up to three pairs".to_string())
        }
        FileType::PromoVideo if paths.len() != 2 => {
            return Err("a promo video is one video then its poster".to_string())
        }
        _ => {}
    }
    for path in paths {
        let ext = extension(path);
        let ok = match kind {
            FileType::Package => matches!(ext.as_str(), "apk" | "aab" | "rpk"),
            FileType::ElectronicCertificate => ext == "pdf",
            FileType::CultureLicense => is_image(path) || ext == "pdf",
            // Videos alternate with poster images.
            FileType::IntroVideo | FileType::PromoVideo => {
                is_image(path) || matches!(ext.as_str(), "mp4" | "mov")
            }
            _ => is_image(path),
        };
        if !ok {
            return Err(format!(
                "{} does not fit {}",
                path.display(),
                file_type_name(kind)
            ));
        }
    }
    Ok(())
}

fn text_of(info: &LanguageInfo) -> LanguageFields {
    LanguageFields {
        lang: info.lang.clone().unwrap_or_default(),
        app_name: info.app_name.clone().filter(|v| !v.is_empty()),
        app_desc: info.app_desc.clone().filter(|v| !v.is_empty()),
        brief_info: info.brief_info.clone().filter(|v| !v.is_empty()),
        new_features: info.new_features.clone().filter(|v| !v.is_empty()),
    }
}

/// Text that was set before and is empty now.
fn lost_text(before: &LanguageFields, after: &LanguageInfo) -> bool {
    let gone = |was: &Option<String>, now: &Option<String>| {
        was.is_some() && now.as_deref().unwrap_or_default().is_empty()
    };
    gone(&before.app_desc, &after.app_desc)
        || gone(&before.brief_info, &after.brief_info)
        || gone(&before.new_features, &after.new_features)
        || gone(&before.app_name, &after.app_name)
}

/// Upload files of one type and bind them to the app, in order. With
/// `dry_run` nothing is sent: the plan lists each file's size and hash.
///
/// Binding a file for a language clears that language's text, so the text is
/// read first and written back if it went missing.
pub async fn upload_files(
    client: &AppGalleryClient,
    app: &str,
    kind: FileType,
    paths: &[std::path::PathBuf],
    options: &UploadOptions,
    dry_run: bool,
) -> Result<FileUpload, String> {
    check_files(kind, paths)?;
    if kind.needs_lang() && options.lang.as_deref().unwrap_or_default().is_empty() {
        return Err(format!("{} needs --lang", file_type_name(kind)));
    }
    if matches!(kind, FileType::Screenshots | FileType::VrScreenshots)
        && options.image_orientation.is_none()
    {
        return Err("screenshots need --orientation portrait or landscape".to_string());
    }
    if matches!(kind, FileType::IntroVideo | FileType::PromoVideo)
        && options.video_orientation.is_none()
    {
        return Err("videos need --video-orientation portrait or landscape".to_string());
    }
    let mut loaded = Vec::new();
    for path in paths {
        let bytes = std::fs::read(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
        loaded.push((path, bytes));
    }
    let app_id = resolve_app_id(client, app).await?;
    let lang = options.lang.clone().filter(|_| kind.needs_lang());
    let mut files: Vec<PlannedFile> = loaded
        .iter()
        .map(|(path, bytes)| PlannedFile {
            path: path.display().to_string(),
            bytes: bytes.len(),
            sha256: sha256_hex(bytes),
            object_id: None,
        })
        .collect();
    let mut result = FileUpload {
        dry_run,
        app_id: app_id.clone(),
        file_type: file_type_name(kind),
        lang: lang.clone(),
        files: Vec::new(),
        pkg_version: Vec::new(),
        restored_text: false,
    };
    if dry_run {
        result.files = files;
        return Ok(result);
    }

    let text_before = match &lang {
        Some(lang) => client
            .app_info(&app_id, Some(lang))
            .await
            .map_err(err)?
            .languages
            .into_iter()
            .next()
            .map(|info| text_of(&info)),
        None => None,
    };

    let mut names = Vec::new();
    for ((path, bytes), planned) in loaded.into_iter().zip(files.iter_mut()) {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| format!("{} has no file name", path.display()))?
            .to_string();
        let target = client
            .upload_url(&app_id, &name, &bytes, Some(options.chinese_mainland))
            .await
            .map_err(err)?;
        client.put_upload(&target, bytes).await.map_err(err)?;
        planned.object_id = Some(target.object_id);
        names.push(name);
    }
    let bind = FileBind {
        file_type: kind,
        lang: lang.as_deref(),
        image_orientation: options.image_orientation,
        video_orientation: options.video_orientation,
        sensitive_permission_desc: options.sensitive_permission_desc.as_deref(),
        device_type: options.device_type,
        files: files
            .iter()
            .zip(&names)
            .map(|(f, name)| {
                (
                    (kind == FileType::Package).then_some(name.as_str()),
                    f.object_id.as_deref().unwrap_or_default(),
                )
            })
            .collect(),
    };
    result.pkg_version = client.bind_files(&app_id, &bind).await.map_err(err)?;

    if let (Some(lang), Some(before)) = (&lang, text_before) {
        let after = client
            .app_info(&app_id, Some(lang))
            .await
            .map_err(err)?
            .languages
            .into_iter()
            .next()
            .unwrap_or_default();
        if lost_text(&before, &after) {
            client
                .update_language_info(&app_id, &before, None)
                .await
                .map_err(|e| format!("files bound, but restoring the {lang} text failed: {e}"))?;
            result.restored_text = true;
        }
    }
    result.files = files;
    Ok(result)
}

/// Delete one language's listing (not the default language).
pub async fn delete_language(
    client: &AppGalleryClient,
    app: &str,
    lang: &str,
    dry_run: bool,
) -> Result<serde_json::Value, String> {
    let app_id = resolve_app_id(client, app).await?;
    if !dry_run {
        client
            .delete_language_info(&app_id, lang, None)
            .await
            .map_err(err)?;
    }
    Ok(serde_json::json!({ "dry_run": dry_run, "app_id": app_id, "deleted_lang": lang }))
}

#[derive(Debug, Serialize)]
pub struct ReleaseAction {
    pub dry_run: bool,
    pub app_id: String,
    pub action: &'static str,
    /// What was (or would be) sent.
    pub detail: serde_json::Value,
}

/// Submit for review. Approved apps go live (at `release_time` if set).
pub async fn submit(
    client: &AppGalleryClient,
    app: &str,
    options: Submit,
    dry_run: bool,
) -> Result<ReleaseAction, String> {
    options.validate().map_err(err)?;
    let app_id = resolve_app_id(client, app).await?;
    let detail = serde_json::json!({
        "releaseTime": options.release_time,
        "remark": options.remark,
        "phased": options.phased,
    });
    if !dry_run {
        client.submit(&app_id, &options).await.map_err(err)?;
    }
    Ok(ReleaseAction {
        dry_run,
        app_id,
        action: "submit",
        detail,
    })
}

/// Update a phased release, or with `to_full` turn it into a full release.
pub async fn update_phased_release(
    client: &AppGalleryClient,
    app: &str,
    phased: PhasedRelease,
    to_full: bool,
    dry_run: bool,
) -> Result<ReleaseAction, String> {
    if !to_full {
        phased.validate().map_err(err)?;
    }
    let app_id = resolve_app_id(client, app).await?;
    if !dry_run {
        client
            .update_phased_release(&app_id, &phased, to_full)
            .await
            .map_err(err)?;
    }
    Ok(ReleaseAction {
        dry_run,
        app_id,
        action: if to_full {
            "phased-release-to-full"
        } else {
            "phased-release-update"
        },
        detail: if to_full {
            serde_json::json!({})
        } else {
            serde_json::to_value(&phased).map_err(err)?
        },
    })
}

/// Report whether the app depends on Google Mobile Services.
pub async fn set_gms_dependency(
    client: &AppGalleryClient,
    app: &str,
    needs_gms: bool,
    dry_run: bool,
) -> Result<ReleaseAction, String> {
    let app_id = resolve_app_id(client, app).await?;
    if !dry_run {
        client
            .set_gms_dependency(&app_id, needs_gms)
            .await
            .map_err(err)?;
    }
    Ok(ReleaseAction {
        dry_run,
        app_id,
        action: "gms",
        detail: serde_json::json!({ "needGms": u8::from(needs_gms) }),
    })
}

/// Change when a version in Releasing state goes live.
pub async fn update_release_time(
    client: &AppGalleryClient,
    app: &str,
    change: ReleaseTimeChange,
    dry_run: bool,
) -> Result<ReleaseAction, String> {
    let app_id = resolve_app_id(client, app).await?;
    let detail = match &change {
        ReleaseTimeChange::Immediately => serde_json::json!({ "changeType": 2 }),
        ReleaseTimeChange::At(t) => serde_json::json!({ "changeType": 3, "releaseTime": t }),
    };
    if !dry_run {
        client
            .update_release_time(&app_id, &change)
            .await
            .map_err(err)?;
    }
    Ok(ReleaseAction {
        dry_run,
        app_id,
        action: "release-time",
        detail,
    })
}

/// Packages uploaded for the app.
pub async fn package_list(
    client: &AppGalleryClient,
    app: &str,
    from: Option<u32>,
    max: Option<u32>,
) -> Result<PackageList, String> {
    let app_id = resolve_app_id(client, app).await?;
    client.package_list(&app_id, from, max).await.map_err(err)
}

/// Parse status of uploaded packages (0 normal, 1 parsing, 2 failed).
pub async fn package_status(
    client: &AppGalleryClient,
    app: &str,
    pkg_ids: &[String],
) -> Result<Vec<PackageState>, String> {
    let app_id = resolve_app_id(client, app).await?;
    let ids: Vec<&str> = pkg_ids.iter().map(String::as_str).collect();
    client.package_status(&app_id, &ids).await.map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_checks_follow_the_type() {
        use std::path::PathBuf;
        let p = |names: &[&str]| names.iter().map(PathBuf::from).collect::<Vec<_>>();
        assert!(check_files(FileType::Icon, &p(&["a.png"])).is_ok());
        assert!(check_files(FileType::Icon, &p(&["a.png", "b.png"])).is_err());
        assert!(check_files(FileType::Package, &p(&["siGit.apk"])).is_ok());
        assert!(check_files(FileType::Package, &p(&["siGit.png"])).is_err());
        assert!(check_files(FileType::PromoVideo, &p(&["v.mp4", "poster.png"])).is_ok());
        assert!(check_files(FileType::IntroVideo, &p(&["v.mp4"])).is_err());
        assert!(check_files(FileType::ElectronicCertificate, &p(&["c.pdf"])).is_ok());
        let nine: Vec<String> = (0..9).map(|i| format!("{i}.png")).collect();
        let nine: Vec<&str> = nine.iter().map(String::as_str).collect();
        assert!(check_files(FileType::Screenshots, &p(&nine)).is_err());
        assert!(check_files(FileType::Screenshots, &p(&["1.png", "2.png"])).is_err());
        assert!(check_files(FileType::Screenshots, &p(&["1.png", "2.png", "3.png"])).is_ok());
    }

    #[test]
    fn lost_text_spots_fields_that_were_cleared() {
        let before = LanguageFields {
            lang: "en-US".into(),
            app_desc: Some("d".into()),
            ..Default::default()
        };
        let cleared = LanguageInfo {
            app_desc: Some(String::new()),
            ..Default::default()
        };
        let kept = LanguageInfo {
            app_desc: Some("d".into()),
            ..Default::default()
        };
        assert!(lost_text(&before, &cleared));
        assert!(!lost_text(&before, &kept));
    }

    #[test]
    fn every_file_type_has_a_distinct_name() {
        let names: std::collections::HashSet<_> =
            FileType::ALL.iter().map(|t| file_type_name(*t)).collect();
        assert_eq!(names.len(), FileType::ALL.len());
    }

    #[test]
    fn only_png_and_jpeg_count_as_images() {
        assert!(is_image(Path::new("a/icon.PNG")));
        assert!(is_image(Path::new("shot.jpeg")));
        assert!(!is_image(Path::new("notes.txt")));
        assert!(!is_image(Path::new("noext")));
    }

    #[test]
    fn differs_names_only_the_fields_that_were_sent_and_changed() {
        let sent = LanguageFields {
            lang: "en-US".into(),
            app_name: Some("siGit".into()),
            brief_info: Some("a".into()),
            ..Default::default()
        };
        let stored = LanguageInfo {
            app_name: Some("siGit".into()),
            brief_info: Some("b".into()),
            app_desc: Some("untouched".into()),
            ..Default::default()
        };
        assert_eq!(differs(&sent, &stored), vec!["briefInfo"]);
    }
}
