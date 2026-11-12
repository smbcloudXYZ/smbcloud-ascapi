//! Amazon Appstore operations shared by `asc amazon` and the MCP tools.
//!
//! Nothing here commits an edit: submitting for review stays a human action
//! in the Developer Console.

use std::path::Path;

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use smbcloud_ascapi_amazon::{AmazonClient, Credentials, Edit};

/// Resolve the client. `path` wins; then `AMAZON_APPSTORE_CREDENTIALS_JSON_PATH`;
/// then the contents inline in `AMAZON_APPSTORE_CREDENTIALS_JSON`. Per call, so
/// an unconfigured MCP server still lists its tools. The secret never appears
/// in an error.
pub fn amazon_client(path: Option<&Path>) -> Result<AmazonClient, String> {
    let credentials = if let Some(path) = path {
        Credentials::from_json_file(path)
    } else if let Ok(path) = std::env::var("AMAZON_APPSTORE_CREDENTIALS_JSON_PATH") {
        Credentials::from_json_file(path)
    } else if let Ok(json) = std::env::var("AMAZON_APPSTORE_CREDENTIALS_JSON") {
        Credentials::from_json(&json)
    } else {
        return Err(
            "no Amazon Appstore credentials: pass --credentials, or set \
                    AMAZON_APPSTORE_CREDENTIALS_JSON_PATH (or AMAZON_APPSTORE_CREDENTIALS_JSON)"
                .to_string(),
        );
    }
    .map_err(|e| e.to_string())?;
    Ok(AmazonClient::new(credentials))
}

fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// The open edit, or an error naming the remedy when there is none.
pub async fn open_edit(client: &AmazonClient, app_id: &str) -> Result<Edit, String> {
    client.get_edit(app_id).await.map_err(err)?.ok_or_else(|| {
        format!("app {app_id} has no open edit; run `asc amazon edits create {app_id}`")
    })
}

pub async fn edit_get(client: &AmazonClient, app_id: &str) -> Result<Option<Edit>, String> {
    client.get_edit(app_id).await.map_err(err)
}

pub async fn apk_list(client: &AmazonClient, app_id: &str) -> Result<Value, String> {
    let edit = open_edit(client, app_id).await?;
    client.list_apks(app_id, &edit.id).await.map_err(err)
}

#[derive(Debug, Serialize)]
pub struct UploadPlan {
    pub dry_run: bool,
    pub app_id: String,
    pub edit_id: String,
    pub file: String,
    pub bytes: u64,
    pub sha256: String,
    /// `upload` when the edit has no APK, `replace` when it has one.
    pub action: &'static str,
    pub existing_apk_id: Option<String>,
    /// The API reply; `None` on a dry run.
    pub result: Option<Value>,
}

fn sha256_file(path: &Path) -> Result<(u64, String), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let digest = Sha256::digest(&bytes);
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    Ok((bytes.len() as u64, hex))
}

fn first_apk_id(list: &Value) -> Option<String> {
    let items = list.as_array().or_else(|| list.get("apks")?.as_array())?;
    items.first()?.get("id")?.as_str().map(str::to_string)
}

/// Upload the APK to the open edit; when the edit already has one, replace it
/// (Amazon allows one APK per edit slot here, and replace keeps its targeting).
pub async fn apk_upload(
    client: &AmazonClient,
    app_id: &str,
    apk: &Path,
    dry_run: bool,
) -> Result<UploadPlan, String> {
    let (bytes, sha256) = sha256_file(apk)?;
    let edit = open_edit(client, app_id).await?;
    let existing = first_apk_id(&client.list_apks(app_id, &edit.id).await.map_err(err)?);
    let action = if existing.is_some() {
        "replace"
    } else {
        "upload"
    };
    let mut plan = UploadPlan {
        dry_run,
        app_id: app_id.to_string(),
        edit_id: edit.id.clone(),
        file: apk.display().to_string(),
        bytes,
        sha256,
        action,
        existing_apk_id: existing.clone(),
        result: None,
    };
    if dry_run {
        return Ok(plan);
    }
    plan.result = Some(match existing {
        Some(apk_id) => {
            let etag = client
                .apk_etag(app_id, &edit.id, &apk_id)
                .await
                .map_err(err)?;
            client
                .replace_apk(app_id, &edit.id, &apk_id, &etag, apk)
                .await
                .map_err(err)?
        }
        None => client
            .upload_apk(app_id, &edit.id, apk)
            .await
            .map_err(err)?,
    });
    Ok(plan)
}

pub async fn apk_delete(
    client: &AmazonClient,
    app_id: &str,
    apk_id: &str,
    dry_run: bool,
) -> Result<Value, String> {
    let edit = open_edit(client, app_id).await?;
    if !dry_run {
        client
            .delete_apk(app_id, &edit.id, apk_id)
            .await
            .map_err(err)?;
    }
    Ok(serde_json::json!({"dry_run": dry_run, "edit_id": edit.id, "deleted_apk_id": apk_id}))
}

pub async fn targeting_get(
    client: &AmazonClient,
    app_id: &str,
    apk_id: &str,
) -> Result<Value, String> {
    let edit = open_edit(client, app_id).await?;
    client
        .get_targeting(app_id, &edit.id, apk_id)
        .await
        .map_err(err)
}

/// Put a targeting document for an APK, then read it back.
pub async fn targeting_set(
    client: &AmazonClient,
    app_id: &str,
    apk_id: &str,
    targeting: Value,
    dry_run: bool,
) -> Result<Value, String> {
    let edit = open_edit(client, app_id).await?;
    let before = client
        .get_targeting(app_id, &edit.id, apk_id)
        .await
        .map_err(err)?;
    if dry_run {
        return Ok(serde_json::json!({"dry_run": true, "before": before, "sent": targeting}));
    }
    client
        .put_targeting(app_id, &edit.id, apk_id, &targeting)
        .await
        .map_err(err)?;
    let after = client
        .get_targeting(app_id, &edit.id, apk_id)
        .await
        .map_err(err)?;
    Ok(serde_json::json!({"dry_run": false, "before": before, "after": after}))
}

pub async fn edit_create(
    client: &AmazonClient,
    app_id: &str,
    dry_run: bool,
) -> Result<Value, String> {
    if let Some(edit) = client.get_edit(app_id).await.map_err(err)? {
        return Ok(serde_json::json!({"dry_run": dry_run, "created": false, "edit": edit}));
    }
    if dry_run {
        return Ok(serde_json::json!({"dry_run": true, "created": false, "would_create": true}));
    }
    let edit = client.create_edit(app_id).await.map_err(err)?;
    Ok(serde_json::json!({"dry_run": false, "created": true, "edit": edit}))
}

pub async fn edit_validate(client: &AmazonClient, app_id: &str) -> Result<Value, String> {
    let edit = open_edit(client, app_id).await?;
    let reply = client.validate_edit(app_id, &edit.id).await.map_err(err)?;
    Ok(serde_json::json!({"edit_id": edit.id, "validation": reply}))
}

pub async fn edit_delete(
    client: &AmazonClient,
    app_id: &str,
    dry_run: bool,
) -> Result<Value, String> {
    let edit = open_edit(client, app_id).await?;
    if !dry_run {
        client.delete_edit(app_id, &edit.id).await.map_err(err)?;
    }
    Ok(serde_json::json!({"dry_run": dry_run, "deleted_edit_id": edit.id}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_apk_id_reads_a_bare_array_or_an_apks_wrapper() {
        assert_eq!(
            first_apk_id(&serde_json::json!([{"id": "a1"}])).as_deref(),
            Some("a1")
        );
        assert_eq!(
            first_apk_id(&serde_json::json!({"apks": [{"id": "a2"}]})).as_deref(),
            Some("a2")
        );
        assert_eq!(first_apk_id(&serde_json::json!([])), None);
    }
}
