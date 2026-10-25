//! Microsoft Store listing operations shared by `asc msstore` and the MCP tools.
//!
//! The Store keeps at most one pending submission per app. The rule here is
//! to reuse it, never replace it: the pending submission may already hold the
//! uploaded package, so listing text is written into that one. Only when none
//! exists is a submission created (cloned from the last published one). A
//! submission that is being processed or certified is never touched, and
//! nothing here commits a submission to certification.

use std::path::Path;

use serde::Serialize;
use serde_json::Value;
use smbcloud_ascapi_msstore::{Credentials, ListingFields, MsStoreClient};

/// Submission states in which the Store is working on the submission and a
/// write would be refused or lost.
const LOCKED_STATUSES: [&str; 6] = [
    "CommitStarted",
    "PreProcessing",
    "Certification",
    "PendingPublish",
    "Publishing",
    "Release",
];

/// Resolve the Store client. `path` wins; then `MSSTORE_ENV_PATH`; then the
/// `MSSTORE_*` variables set directly. Per call, so an unconfigured MCP
/// server still lists its tools. The secret never appears in an error.
pub fn msstore_client(path: Option<&Path>) -> Result<MsStoreClient, String> {
    let credentials = if let Some(path) = path {
        Credentials::from_env_file(path)
    } else if let Ok(path) = std::env::var("MSSTORE_ENV_PATH") {
        Credentials::from_env_file(path)
    } else if std::env::var_os("MSSTORE_CLIENT_SECRET").is_some() {
        Credentials::from_process_env()
    } else {
        return Err(
            "no Microsoft Store credentials: pass --msstore-env, or set \
                    MSSTORE_ENV_PATH (a dotenv file) or the MSSTORE_* variables"
                .to_string(),
        );
    };
    credentials
        .map(MsStoreClient::new)
        .map_err(|e| e.to_string())
}

fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn product<'a>(client: &'a MsStoreClient, product_id: Option<&'a str>) -> &'a str {
    product_id.unwrap_or_else(|| client.default_product_id())
}

fn str_at<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str)
}

pub async fn read_app(client: &MsStoreClient, product_id: Option<&str>) -> Result<Value, String> {
    client
        .get_app(product(client, product_id))
        .await
        .map_err(err)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmissionStatus {
    pub product_id: String,
    pub pending_submission_id: Option<String>,
    /// The pending submission's status record; `None` when there is no draft.
    pub pending: Option<Value>,
    pub last_published_submission_id: Option<String>,
}

pub async fn submission_status(
    client: &MsStoreClient,
    product_id: Option<&str>,
) -> Result<SubmissionStatus, String> {
    let product_id = product(client, product_id);
    let app = client.get_app(product_id).await.map_err(err)?;
    let pending_id = str_at(&app, "/pendingApplicationSubmission/id").map(str::to_string);
    let pending = match &pending_id {
        Some(id) => Some(
            client
                .submission_status(product_id, id)
                .await
                .map_err(err)?,
        ),
        None => None,
    };
    Ok(SubmissionStatus {
        product_id: product_id.to_string(),
        pending_submission_id: pending_id,
        pending,
        last_published_submission_id: str_at(&app, "/lastPublishedApplicationSubmission/id")
            .map(str::to_string),
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmissionRead {
    pub submission_id: String,
    /// `pending` or `lastPublished`.
    pub source: &'static str,
    pub status: Option<String>,
    pub packages: Vec<Value>,
    pub listing_locales: Vec<String>,
}

/// The package keys worth showing. The raw package record can carry a signed
/// upload URL, so it is not passed through whole.
const PACKAGE_KEYS: [&str; 5] = ["fileName", "version", "fileStatus", "architecture", "id"];

/// The packages and listing locales of a submission: the given one, else the
/// pending one, else the last published one.
pub async fn read_submission(
    client: &MsStoreClient,
    product_id: Option<&str>,
    submission_id: Option<&str>,
) -> Result<SubmissionRead, String> {
    let product_id = product(client, product_id);
    let (id, source) = match submission_id {
        Some(id) => (id.to_string(), "requested"),
        None => {
            let app = client.get_app(product_id).await.map_err(err)?;
            match str_at(&app, "/pendingApplicationSubmission/id") {
                Some(id) => (id.to_string(), "pending"),
                None => match str_at(&app, "/lastPublishedApplicationSubmission/id") {
                    Some(id) => (id.to_string(), "lastPublished"),
                    None => return Err("the app has no submission".to_string()),
                },
            }
        }
    };
    let submission = client.get_submission(product_id, &id).await.map_err(err)?;
    let packages = submission
        .get("applicationPackages")
        .and_then(Value::as_array)
        .map(|packages| {
            packages
                .iter()
                .map(|package| {
                    let kept = PACKAGE_KEYS
                        .iter()
                        .filter_map(|key| package.get(*key).map(|v| (key.to_string(), v.clone())))
                        .collect();
                    Value::Object(kept)
                })
                .collect()
        })
        .unwrap_or_default();
    let listing_locales = submission
        .get("listings")
        .and_then(Value::as_object)
        .map(|listings| listings.keys().cloned().collect())
        .unwrap_or_default();
    Ok(SubmissionRead {
        submission_id: id,
        source,
        status: str_at(&submission, "/status").map(str::to_string),
        packages,
        listing_locales,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingRead {
    pub submission_id: String,
    /// `pending` or `lastPublished`.
    pub source: &'static str,
    pub status: Option<String>,
    pub locale: String,
    pub listing: ListingFields,
}

/// The listing of the pending submission, or of the last published one when
/// there is no draft.
pub async fn read_listing(
    client: &MsStoreClient,
    product_id: Option<&str>,
    locale: &str,
) -> Result<ListingRead, String> {
    let product_id = product(client, product_id);
    let app = client.get_app(product_id).await.map_err(err)?;
    let (id, source) = match str_at(&app, "/pendingApplicationSubmission/id") {
        Some(id) => (id, "pending"),
        None => match str_at(&app, "/lastPublishedApplicationSubmission/id") {
            Some(id) => (id, "lastPublished"),
            None => return Err("the app has no pending or published submission".to_string()),
        },
    };
    let submission = client.get_submission(product_id, id).await.map_err(err)?;
    Ok(ListingRead {
        submission_id: id.to_string(),
        source,
        status: str_at(&submission, "/status").map(str::to_string),
        locale: locale.to_string(),
        listing: ListingFields::read(&submission, locale),
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldChange {
    pub field: &'static str,
    pub before: Value,
    pub after: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingUpdate {
    pub dry_run: bool,
    pub submission_id: String,
    /// True when no draft existed and one was created for this update. On a
    /// dry run that submission is deleted again.
    pub created_submission: bool,
    pub warnings: Vec<String>,
    /// The fields that differ from what the submission holds now.
    pub changes: Vec<FieldChange>,
    /// The listing read back after the write. `None` on a dry run.
    pub after: Option<ListingFields>,
}

fn changes(sent: &ListingFields, before: &ListingFields) -> Vec<FieldChange> {
    let mut out = Vec::new();
    let mut push = |field, differs: bool, b: Value, a: Value| {
        if differs {
            out.push(FieldChange {
                field,
                before: b,
                after: a,
            });
        }
    };
    macro_rules! field {
        ($name:literal, $f:ident) => {
            push(
                $name,
                sent.$f.is_some() && sent.$f != before.$f,
                to_value(&before.$f),
                to_value(&sent.$f),
            );
        };
    }
    field!("title", title);
    field!("shortDescription", short_description);
    field!("description", description);
    field!("releaseNotes", release_notes);
    field!("features", features);
    field!("keywords", keywords);
    out
}

fn to_value(v: &impl Serialize) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// Write listing text into the pending submission (creating one only when
/// none exists), then read it back and compare. Never commits.
pub async fn update_listing(
    client: &MsStoreClient,
    product_id: Option<&str>,
    locale: &str,
    fields: ListingFields,
    dry_run: bool,
) -> Result<ListingUpdate, String> {
    let warnings = fields.validate()?;
    if fields.is_empty() {
        return Err("nothing to update: pass at least one listing field".to_string());
    }
    let product_id = product(client, product_id);
    let app = client.get_app(product_id).await.map_err(err)?;

    let (mut submission, created) = match str_at(&app, "/pendingApplicationSubmission/id") {
        Some(id) => {
            let submission = client.get_submission(product_id, id).await.map_err(err)?;
            if let Some(status) = str_at(&submission, "/status") {
                if LOCKED_STATUSES.contains(&status) {
                    return Err(format!(
                        "submission {id} is {status}; the Store is working on it, so its \
                         listing cannot be edited now"
                    ));
                }
            }
            (submission, false)
        }
        None => (
            client.create_submission(product_id).await.map_err(err)?,
            true,
        ),
    };
    let id = str_at(&submission, "/id")
        .ok_or("the submission has no id")?
        .to_string();

    let outcome = write_and_verify(
        client,
        product_id,
        &id,
        &mut submission,
        locale,
        &fields,
        dry_run,
    )
    .await;
    // A submission this call created is removed again unless it holds the
    // change: on a dry run, or when the write failed.
    if created && (dry_run || outcome.is_err()) {
        if let Err(e) = client.delete_submission(product_id, &id).await {
            return Err(format!(
                "{}submission {id} was created and could not be deleted: {e}",
                outcome.err().map(|o| format!("{o}; ")).unwrap_or_default()
            ));
        }
    }
    let (changes, after) = outcome?;
    Ok(ListingUpdate {
        dry_run,
        submission_id: id,
        created_submission: created,
        warnings,
        changes,
        after,
    })
}

type Outcome = (Vec<FieldChange>, Option<ListingFields>);

async fn write_and_verify(
    client: &MsStoreClient,
    product_id: &str,
    id: &str,
    submission: &mut Value,
    locale: &str,
    fields: &ListingFields,
    dry_run: bool,
) -> Result<Outcome, String> {
    let before = ListingFields::read(submission, locale);
    let changes = changes(fields, &before);
    if dry_run {
        return Ok((changes, None));
    }
    let packages = submission.get("applicationPackages").cloned();
    fields.apply(submission, locale).map_err(err)?;
    client
        .update_submission(product_id, id, submission)
        .await
        .map_err(err)?;
    let stored = client.get_submission(product_id, id).await.map_err(err)?;
    let after = ListingFields::read(&stored, locale);
    let wrong = fields.diff(&after);
    if !wrong.is_empty() {
        return Err(format!(
            "written, but the read-back differs in: {}",
            wrong.join(", ")
        ));
    }
    if stored.get("applicationPackages").cloned() != packages {
        return Err(
            "written, but applicationPackages changed; check the submission in \
                    Partner Center"
                .to_string(),
        );
    }
    Ok((changes, Some(after)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_statuses_cover_the_in_flight_states() {
        for s in ["CommitStarted", "PreProcessing", "Certification"] {
            assert!(LOCKED_STATUSES.contains(&s));
        }
        assert!(!LOCKED_STATUSES.contains(&"PendingCommit"));
    }

    #[test]
    fn changes_list_only_sent_fields_that_differ() {
        let before = ListingFields {
            title: Some("A".into()),
            description: Some("d".into()),
            ..Default::default()
        };
        let sent = ListingFields {
            title: Some("A".into()),
            description: Some("e".into()),
            keywords: Some(vec!["k".into()]),
            ..Default::default()
        };
        let names: Vec<_> = changes(&sent, &before).iter().map(|c| c.field).collect();
        assert_eq!(names, vec!["description", "keywords"]);
    }

    #[test]
    fn missing_credentials_name_the_options_without_a_secret() {
        if std::env::var_os("MSSTORE_ENV_PATH").is_none()
            && std::env::var_os("MSSTORE_CLIENT_SECRET").is_none()
        {
            let error = msstore_client(None).err().unwrap();
            assert!(error.contains("MSSTORE_ENV_PATH"));
        }
    }
}
