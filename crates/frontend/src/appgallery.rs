//! Huawei AppGallery listing operations shared by `asc appgallery` and the
//! MCP tools.

use std::path::Path;

use serde::Serialize;
use smbcloud_ascapi_appgallery::upload::sha256_hex;
use smbcloud_ascapi_appgallery::{
    AppGalleryClient, AppInfo, AppInfoFields, AppInfoResponse, FileType, ImageOrientation,
    LanguageFields, LanguageInfo, ServiceAccount,
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

/// Most screenshots AppGallery takes per language.
pub const MAX_SCREENSHOTS: usize = 10;

#[derive(Debug, Serialize)]
pub struct PlannedFile {
    pub path: String,
    pub bytes: usize,
    pub sha256: String,
    /// The server's object id; `None` on a dry run.
    pub object_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ImageUpload {
    pub dry_run: bool,
    pub app_id: String,
    pub kind: &'static str,
    pub lang: String,
    pub files: Vec<PlannedFile>,
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg"))
        .unwrap_or(false)
}

/// Upload an icon (exactly one file) or screenshots (in display order) and
/// bind them to one language. With `dry_run` nothing is sent: the plan lists
/// each file's size and hash. Uploads are sent with `chineseMainlandFlag=0`,
/// matching the "not available in the Chinese mainland" decision.
pub async fn upload_images(
    client: &AppGalleryClient,
    app: &str,
    lang: &str,
    kind: FileType,
    orientation: Option<ImageOrientation>,
    paths: &[std::path::PathBuf],
    dry_run: bool,
) -> Result<ImageUpload, String> {
    let kind_name = match kind {
        FileType::Icon => "icon",
        FileType::Screenshots => "screenshots",
        FileType::Package => return Err("use the package upload for APKs".to_string()),
    };
    if lang.is_empty() {
        return Err("lang must not be empty".to_string());
    }
    if paths.is_empty() {
        return Err("pass at least one image file".to_string());
    }
    if kind == FileType::Icon && paths.len() != 1 {
        return Err("an icon is exactly one file".to_string());
    }
    if kind == FileType::Screenshots {
        if paths.len() > MAX_SCREENSHOTS {
            return Err(format!(
                "at most {MAX_SCREENSHOTS} screenshots per language"
            ));
        }
        if orientation.is_none() {
            return Err("screenshots need --orientation portrait or landscape".to_string());
        }
    }
    let mut loaded = Vec::new();
    for path in paths {
        if !is_image(path) {
            return Err(format!("{} is not a PNG or JPEG file", path.display()));
        }
        let bytes = std::fs::read(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
        loaded.push((path, bytes));
    }
    let app_id = resolve_app_id(client, app).await?;
    let mut files: Vec<PlannedFile> = loaded
        .iter()
        .map(|(path, bytes)| PlannedFile {
            path: path.display().to_string(),
            bytes: bytes.len(),
            sha256: sha256_hex(bytes),
            object_id: None,
        })
        .collect();
    if dry_run {
        return Ok(ImageUpload {
            dry_run,
            app_id,
            kind: kind_name,
            lang: lang.to_string(),
            files,
        });
    }
    for ((path, bytes), planned) in loaded.into_iter().zip(files.iter_mut()) {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| format!("{} has no file name", path.display()))?;
        let target = client
            .upload_url(&app_id, name, &bytes, Some(false))
            .await
            .map_err(err)?;
        client.put_upload(&target, bytes).await.map_err(err)?;
        planned.object_id = Some(target.object_id);
    }
    let objects: Vec<(Option<&str>, &str)> = files
        .iter()
        .map(|f| (None, f.object_id.as_deref().unwrap_or_default()))
        .collect();
    client
        .bind_files(&app_id, kind, Some(lang), orientation, &objects)
        .await
        .map_err(err)?;
    Ok(ImageUpload {
        dry_run,
        app_id,
        kind: kind_name,
        lang: lang.to_string(),
        files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
