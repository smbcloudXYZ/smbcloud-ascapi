//! AdMob operations shared by `asc admob` and the MCP tools.
//!
//! Sign-in is a one-time browser flow (`asc admob login`); every other call
//! uses the cached refresh token. The MCP tools are read-only and never start
//! a browser: with no cached token they fail with a message that says to run
//! `asc admob login`.

use std::path::{Path, PathBuf};

use serde::Serialize;
use smbcloud_ascapi_admob::{
    auth, Account, AdFormat, AdUnit, AdmobClient, App, CreateAdUnit, OAuthClient, StoredToken,
};

fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn oauth_client(path: Option<&Path>) -> Result<OAuthClient, String> {
    let path = match path {
        Some(path) => path.to_path_buf(),
        None => std::env::var("ADMOB_OAUTH_CLIENT_JSON_PATH")
            .map(PathBuf::from)
            .map_err(|_| {
                "no AdMob OAuth client: pass --admob-oauth-client, or set \
                 ADMOB_OAUTH_CLIENT_JSON_PATH to a Desktop-app client JSON (see docs/admob.md)"
                    .to_string()
            })?,
    };
    OAuthClient::from_json_file(path).map_err(err)
}

/// Resolve the AdMob client. `path` wins; then `ADMOB_OAUTH_CLIENT_JSON_PATH`.
/// Per call, so an unconfigured MCP server still lists its tools.
pub fn admob_client(path: Option<&Path>) -> Result<AdmobClient, String> {
    AdmobClient::new(oauth_client(path)?, None).map_err(err)
}

/// Run the browser sign-in and cache the refresh token.
pub async fn login(path: Option<&Path>) -> Result<String, String> {
    let client = oauth_client(path)?;
    let token_path = StoredToken::default_path().map_err(err)?;
    auth::login(&client, &token_path).await.map_err(err)?;
    Ok(format!(
        "signed in; token cached at {}",
        token_path.display()
    ))
}

pub async fn list_accounts(client: &AdmobClient) -> Result<Vec<Account>, String> {
    client.list_accounts().await.map_err(err)
}

/// The account to use: `account` when given, else the only one the user has.
pub async fn resolve_account(
    client: &AdmobClient,
    account: Option<&str>,
) -> Result<String, String> {
    if let Some(account) = account {
        return Ok(account.to_string());
    }
    let accounts = list_accounts(client).await?;
    match accounts.as_slice() {
        [only] => Ok(only.name.clone()),
        [] => Err("this Google user has no AdMob account".to_string()),
        many => Err(format!(
            "several AdMob accounts ({}); specify the account",
            many.iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

pub async fn list_apps(client: &AdmobClient, account: Option<&str>) -> Result<Vec<App>, String> {
    let account = resolve_account(client, account).await?;
    client.list_apps(&account).await.map_err(err)
}

/// Ad units, optionally limited to one app id (`ca-app-pub-…~…`).
pub async fn list_ad_units(
    client: &AdmobClient,
    account: Option<&str>,
    app: Option<&str>,
) -> Result<Vec<AdUnit>, String> {
    let account = resolve_account(client, account).await?;
    let units = client.list_ad_units(&account).await.map_err(err)?;
    Ok(match app {
        Some(app) => units
            .into_iter()
            .filter(|u| u.app_id.as_deref() == Some(app))
            .collect(),
        None => units,
    })
}

/// The request body for a rewarded unit. Pure, so `--dry-run` needs no
/// credentials.
pub fn plan_create(
    app: &str,
    name: &str,
    format: &str,
    reward_amount: u64,
    reward_item: &str,
) -> Result<CreateAdUnit, String> {
    match AdFormat::parse(format)? {
        AdFormat::Rewarded => Ok(CreateAdUnit::rewarded(
            app,
            name,
            reward_amount,
            reward_item,
        )),
    }
}

#[derive(Debug, Serialize)]
pub struct CreatedAdUnit {
    pub created: AdUnit,
    /// Whether the unit showed up when the list was read back.
    pub confirmed_in_list: bool,
    /// Set when the read-back failed or did not show the unit. The create
    /// itself succeeded, so this is never an error: a caller that retried on
    /// failure would create a duplicate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// Create the unit, then read the list back to confirm it is there. Only a
/// failed create is an error; a failed or inconclusive read-back is reported
/// in the result.
pub async fn create_ad_unit(
    client: &AdmobClient,
    account: Option<&str>,
    body: &CreateAdUnit,
) -> Result<CreatedAdUnit, String> {
    let account = resolve_account(client, account).await?;
    let created = client.create_ad_unit(&account, body).await.map_err(err)?;
    let (confirmed_in_list, warning) = match client.list_ad_units(&account).await {
        Ok(listed) if listed.iter().any(|u| u.name == created.name) => (true, None),
        Ok(_) => (
            false,
            Some(format!(
                "created {}, but it is not in the list yet; run `asc admob adunits list` before retrying",
                created.name
            )),
        ),
        Err(e) => (
            false,
            Some(format!(
                "created {}, but reading the list back failed ({e}); run `asc admob adunits list` before retrying",
                created.name
            )),
        ),
    };
    Ok(CreatedAdUnit {
        created,
        confirmed_in_list,
        warning,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_builds_the_rewarded_body() {
        let body = plan_create(
            "ca-app-pub-1~2",
            "Karaoke rewarded",
            "rewarded",
            50,
            "Semitones",
        )
        .unwrap();
        assert_eq!(body.ad_format, "REWARDED");
        assert_eq!(body.reward_settings.unit_amount, "50");
    }

    #[test]
    fn plan_rejects_other_formats() {
        assert!(plan_create("a", "n", "banner", 1, "x").is_err());
    }
}
