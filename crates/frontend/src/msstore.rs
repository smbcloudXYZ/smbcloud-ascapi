//! Microsoft Store listing operations shared by `asc msstore` and the MCP tools.
//!
//! The Store keeps at most one pending submission per app. The rule here is
//! to reuse it, never replace it: the pending submission may already hold the
//! uploaded package, so listing text is written into that one. Only when none
//! exists is a submission created (cloned from the last published one). A
//! submission that is being processed or certified is never touched. Only
//! [`commit_pending`] sends a submission to certification.

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

/// Statuses at or past the commit: the Store already has the submission, so a
/// second commit would be refused.
const PAST_COMMIT: [&str; 4] = ["CommitStarted", "PreProcessing", "Certification", "Release"];

/// True when the submission has been committed, so committing again is a
/// no-op.
pub fn is_past_commit(status: &str) -> bool {
    PAST_COMMIT.contains(&status)
}

/// Versions of the packages that will ship: every entry of
/// `applicationPackages[]` whose `fileStatus` is not `PendingDelete`.
pub fn package_versions(submission: &Value) -> Vec<String> {
    submission
        .get("applicationPackages")
        .and_then(Value::as_array)
        .map(|packages| {
            packages
                .iter()
                .filter(|p| str_at(p, "/fileStatus") != Some("PendingDelete"))
                .filter_map(|p| str_at(p, "/version").map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// File names of the packages that will ship but have no `version` yet. A
/// package in `PendingUpload` gets its version only when the commit ingests
/// it, so before the commit the file name is all there is to go on.
pub fn pending_file_names(submission: &Value) -> Vec<String> {
    submission
        .get("applicationPackages")
        .and_then(Value::as_array)
        .map(|packages| {
            packages
                .iter()
                .filter(|p| str_at(p, "/fileStatus") != Some("PendingDelete"))
                .filter(|p| str_at(p, "/version").is_none())
                .filter_map(|p| str_at(p, "/fileName").map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Every dotted number in a file name, whole: `siGit-1.3.2.0.msix` gives
/// `1.3.2.0`, and `x64_11.3.2.0.msixupload` gives `11.3.2.0`, never `1.3.2.0`.
pub fn versions_in_file_name(name: &str) -> Vec<&str> {
    name.split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .map(|run| run.trim_matches('.'))
        .filter(|run| run.contains('.'))
        .collect()
}

/// The guard behind `--expect-package-version`, before the commit. An
/// ingested package is matched by its `version`; a package still pending
/// upload has none, so it is matched by the version in its file name.
pub fn check_expected_version(submission: &Value, expected: &str) -> Result<(), String> {
    let versions = package_versions(submission);
    if versions.iter().any(|v| v == expected) {
        return Ok(());
    }
    let pending = pending_file_names(submission);
    if pending
        .iter()
        .any(|name| versions_in_file_name(name).contains(&expected))
    {
        return Ok(());
    }
    if !pending.is_empty()
        && versions.is_empty()
        && pending.iter().all(|n| versions_in_file_name(n).is_empty())
    {
        return Err(format!(
            "the pending package [{}] has no version yet and its file name does not carry one, so {expected} cannot be checked; nothing was committed",
            pending.join(", ")
        ));
    }
    Err(format!(
        "submission carries package versions [{}] and pending packages [{}], not {expected}; nothing was committed",
        versions.join(", "),
        pending.join(", ")
    ))
}

/// The error messages of a status record, for a `...Failed` status.
fn status_errors(status: &Value) -> Vec<String> {
    status
        .pointer("/statusDetails/errors")
        .and_then(Value::as_array)
        .map(|errors| {
            errors
                .iter()
                .map(|e| {
                    let code = str_at(e, "/code").unwrap_or("");
                    let details = str_at(e, "/details").unwrap_or("");
                    format!("{code}: {details}")
                })
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Committed {
    pub dry_run: bool,
    /// The submission was already past the commit, so nothing was sent.
    pub noop: bool,
    pub submission_id: String,
    pub package_versions: Vec<String>,
    /// Files still pending upload before the commit, matched by name.
    pub pending_uploads: Vec<String>,
    /// Versions the Store reports after the commit, once it has ingested the
    /// packages. `None` on a dry run or no-op, or while it still reports none.
    pub package_versions_after: Option<Vec<String>>,
    /// The status before this call.
    pub status_before: String,
    /// The status after the commit and the wait. `None` on a dry run or no-op.
    pub status_after: Option<String>,
}

/// The post-commit outcome, given the status after the wait and the versions
/// the Store reports (empty when they could not be read). Returns the versions
/// worth reporting, or `None` when the result is not known yet, or an error on
/// a genuine version mismatch after the commit.
///
/// While the status is still `CommitStarted` the new package is not ingested,
/// so any versions the Store reports are the pre-commit ones. The outcome is
/// unknown then, never a mismatch.
fn post_commit_check(
    id: &str,
    current: &str,
    after_versions: Vec<String>,
    expected: Option<&str>,
) -> Result<Option<Vec<String>>, String> {
    if current == "CommitStarted" {
        return Ok(None);
    }
    if let Some(expected) = expected {
        if !after_versions.is_empty() && !after_versions.iter().any(|v| v == expected) {
            return Err(format!(
                "submission {id} was committed (status {current}) but carries package versions [{}], not {expected}",
                after_versions.join(", ")
            ));
        }
    }
    Ok((!after_versions.is_empty()).then_some(after_versions))
}

/// Commit the pending submission to certification.
///
/// Fails closed before writing when there is no pending submission or, with
/// `expect_package_version`, when no shipping package has that version (see
/// [`check_expected_version`]; a package still pending upload is matched by
/// its file name). After the commit the Store's own `version` is checked
/// strictly. A submission already committed is a no-op. After the commit the status is
/// polled until it leaves `CommitStarted` or `wait_seconds` runs out; a status
/// ending in `Failed` is an error carrying the status details.
pub async fn commit_pending(
    client: &MsStoreClient,
    product_id: Option<&str>,
    expect_package_version: Option<&str>,
    wait_seconds: u64,
    dry_run: bool,
) -> Result<Committed, String> {
    let product_id = product(client, product_id);
    let app = client.get_app(product_id).await.map_err(err)?;
    let id = str_at(&app, "/pendingApplicationSubmission/id")
        .ok_or("the app has no pending submission to commit")?
        .to_string();
    let submission = client.get_submission(product_id, &id).await.map_err(err)?;
    let versions = package_versions(&submission);
    let pending_uploads = pending_file_names(&submission);
    if let Some(expected) = expect_package_version {
        check_expected_version(&submission, expected)?;
    }
    let status = client
        .submission_status(product_id, &id)
        .await
        .map_err(err)?;
    let before = str_at(&status, "/status").unwrap_or("").to_string();
    let result = |noop, after, versions_after| Committed {
        dry_run,
        noop,
        submission_id: id.clone(),
        package_versions: versions.clone(),
        pending_uploads: pending_uploads.clone(),
        package_versions_after: versions_after,
        status_before: before.clone(),
        status_after: after,
    };
    if is_past_commit(&before) {
        return Ok(result(true, None, None));
    }
    if dry_run {
        return Ok(result(false, None, None));
    }
    client
        .commit_submission(product_id, &id)
        .await
        .map_err(err)?;

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(wait_seconds);
    loop {
        let status = client
            .submission_status(product_id, &id)
            .await
            .map_err(err)?;
        let current = str_at(&status, "/status").unwrap_or("").to_string();
        if current.ends_with("Failed") {
            return Err(format!(
                "submission {id} is {current}: {}",
                status_errors(&status).join("; ")
            ));
        }
        if current != "CommitStarted" || std::time::Instant::now() >= deadline {
            // Still CommitStarted means the wait ran out before ingestion, so
            // the Store has no post-commit versions yet and there is nothing
            // to read or check. Once it has left CommitStarted the packages
            // are ingested and `version` is authoritative; the commit already
            // succeeded, so a failed read here is reported as unknown, not as
            // a commit failure.
            let after_versions = if current == "CommitStarted" {
                Vec::new()
            } else {
                match client.get_submission(product_id, &id).await {
                    Ok(after) => package_versions(&after),
                    Err(_) => return Ok(result(false, Some(current), None)),
                }
            };
            let reported =
                post_commit_check(&id, &current, after_versions, expect_package_version)?;
            return Ok(result(false, Some(current), reported));
        }
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
}

/// The size Partner Center asks for an add-on's store logo.
pub const ADDON_ICON_SIZE: u32 = 300;

/// One add-on row for `addons list`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddonSummary {
    /// The Store id (e.g. 9NBLGGH4TNMP), which the API addresses.
    pub id: String,
    /// The product id the app buys with (e.g. personal_pro_monthly).
    pub product_id: Option<String>,
    pub product_type: Option<String>,
    pub pending_submission_id: Option<String>,
}

fn addon_summary(addon: &Value) -> AddonSummary {
    AddonSummary {
        id: str_at(addon, "/id").unwrap_or_default().to_string(),
        product_id: str_at(addon, "/productId").map(str::to_string),
        product_type: str_at(addon, "/productType").map(str::to_string),
        pending_submission_id: str_at(addon, "/pendingInAppProductSubmission/id")
            .map(str::to_string),
    }
}

/// The submission API (v1.0) refuses subscription add-ons outright, even for
/// a read: `400 NotAllowed "API does not support in-app-product type:
/// Subscription"`. Seen 2026-11-20 on smbCloud MailX. Their listing, logo
/// included, can only be edited in Partner Center.
const SUBSCRIPTION_REFUSAL: &str = "does not support in-app-product type: Subscription";

pub const SUBSCRIPTION_ADDON_HINT: &str = "Partner Center's submission API does not support \
subscription add-ons; set this one's logo in Partner Center (Add-ons > the add-on > Store listing).";

fn is_subscription_refusal(error: &impl std::fmt::Display) -> bool {
    error.to_string().contains(SUBSCRIPTION_REFUSAL)
}

/// List an app's add-on ids. When the app has a subscription add-on, the
/// list call itself is refused, so explain that instead of passing the raw
/// 400 through.
async fn list_addons_or_explain(client: &MsStoreClient, app: &str) -> Result<Vec<String>, String> {
    client.list_addon_ids(app).await.map_err(|e| {
        if is_subscription_refusal(&e) {
            format!(
                "{app} has subscription add-ons, and Partner Center's submission API refuses to \
                 list or edit an app's add-ons when it does ({SUBSCRIPTION_REFUSAL:?}). Set their \
                 listings and logos in Partner Center (Add-ons > the add-on > Store listing)."
            )
        } else {
            err(e)
        }
    })
}

/// Every add-on of the app, with the product id each one is bought by.
/// Subscription add-ons, which the API will not read, are listed by Store id
/// with `productType: "Subscription"` and no other detail.
pub async fn read_addons(
    client: &MsStoreClient,
    product_id: Option<&str>,
) -> Result<Vec<AddonSummary>, String> {
    let app = product(client, product_id);
    let mut rows = Vec::new();
    for id in list_addons_or_explain(client, app).await? {
        match client.get_addon(&id).await {
            Ok(addon) => rows.push(addon_summary(&addon)),
            Err(e) if is_subscription_refusal(&e) => rows.push(AddonSummary {
                id,
                product_id: None,
                product_type: Some("Subscription".to_string()),
                pending_submission_id: None,
            }),
            Err(e) => return Err(err(e)),
        }
    }
    Ok(rows)
}

/// Find an add-on by its Store id or by the product id the app buys with.
async fn find_addon(client: &MsStoreClient, app: &str, wanted: &str) -> Result<Value, String> {
    let mut known = Vec::new();
    let mut subscriptions = 0;
    for id in list_addons_or_explain(client, app).await? {
        let addon = match client.get_addon(&id).await {
            Ok(addon) => addon,
            Err(e) if is_subscription_refusal(&e) => {
                if id == wanted {
                    return Err(format!(
                        "{id} is a subscription add-on. {SUBSCRIPTION_ADDON_HINT}"
                    ));
                }
                subscriptions += 1;
                continue;
            }
            Err(e) => return Err(err(e)),
        };
        let product_id = str_at(&addon, "/productId").unwrap_or_default().to_string();
        if id == wanted || product_id == wanted {
            return Ok(addon);
        }
        known.push(product_id);
    }
    let mut message = format!(
        "{app} has no readable add-on {wanted:?} (it has: {})",
        if known.is_empty() {
            "none".to_string()
        } else {
            known.join(", ")
        }
    );
    if subscriptions > 0 {
        message.push_str(&format!(
            ". {subscriptions} subscription add-on(s) could not be read; if {wanted:?} is one of \
             them: {SUBSCRIPTION_ADDON_HINT}"
        ));
    }
    Err(message)
}

/// Point every listing (or one locale's) at `file_name` as its icon,
/// marked for upload. Returns the locales changed.
pub fn set_listing_icons(
    submission: &mut Value,
    file_name: &str,
    locale: Option<&str>,
) -> Result<Vec<String>, String> {
    let listings = submission
        .get_mut("listings")
        .and_then(Value::as_object_mut)
        .ok_or("the add-on submission has no listings to put an icon on")?;
    let locales: Vec<String> = match locale {
        Some(l) => {
            if !listings.contains_key(l) {
                return Err(format!(
                    "the add-on has no {l} listing (it has: {})",
                    listings.keys().cloned().collect::<Vec<_>>().join(", ")
                ));
            }
            vec![l.to_string()]
        }
        None => listings.keys().cloned().collect(),
    };
    if locales.is_empty() {
        return Err("the add-on submission has no listings to put an icon on".to_string());
    }
    for code in &locales {
        listings[code]["icon"] = serde_json::json!({
            "fileName": file_name,
            "fileStatus": "PendingUpload",
        });
    }
    Ok(locales)
}

/// A zip holding the one icon file at its root, as `fileUploadUrl` expects.
pub fn icon_zip(file_name: &str, png: &[u8]) -> Result<Vec<u8>, String> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        zip.start_file(file_name, zip::write::SimpleFileOptions::default())
            .map_err(err)?;
        std::io::Write::write_all(&mut zip, png).map_err(err)?;
        zip.finish().map_err(err)?;
    }
    Ok(cursor.into_inner())
}

/// The result of setting an add-on's icon.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddonIconSet {
    pub dry_run: bool,
    pub addon_id: String,
    pub product_id: Option<String>,
    /// The pending submission used, or `None` on a dry run that would create one.
    pub submission_id: Option<String>,
    /// It would create (dry run) or created a submission for this.
    pub created_submission: bool,
    pub locales: Vec<String>,
    pub file_name: String,
}

/// Put `png` on an add-on's store listing(s) as its logo. Writes into the
/// add-on's pending submission, creating one from the last published one
/// when there is none, then uploads the zipped file. It never commits: the
/// submission stays a draft for Partner Center. A dry run reads only.
pub async fn set_addon_icon(
    client: &MsStoreClient,
    product_id: Option<&str>,
    addon: &str,
    path: &Path,
    locale: Option<&str>,
    dry_run: bool,
) -> Result<AddonIconSet, String> {
    let png = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    match smbcloud_ascapi_play::png_dimensions(&png) {
        Some((ADDON_ICON_SIZE, ADDON_ICON_SIZE)) => {}
        Some((w, h)) => {
            return Err(format!(
                "{} is {w}x{h}; an add-on logo must be {ADDON_ICON_SIZE}x{ADDON_ICON_SIZE}",
                path.display()
            ))
        }
        None => return Err(format!("{} is not a PNG", path.display())),
    }
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("the icon path has no file name")?
        .to_string();

    let app = product(client, product_id);
    let found = find_addon(client, app, addon).await?;
    let addon_id = str_at(&found, "/id").unwrap_or(addon).to_string();
    let addon_product_id = str_at(&found, "/productId").map(str::to_string);
    let pending = str_at(&found, "/pendingInAppProductSubmission/id").map(str::to_string);

    let (mut submission, created) = match &pending {
        Some(id) => (
            client
                .get_addon_submission(&addon_id, id)
                .await
                .map_err(err)?,
            false,
        ),
        None if dry_run => {
            // Read the published submission to show which locales would change.
            let published = str_at(&found, "/lastPublishedInAppProductSubmission/id")
                .ok_or("the add-on has no published submission to start from")?;
            (
                client
                    .get_addon_submission(&addon_id, published)
                    .await
                    .map_err(err)?,
                true,
            )
        }
        None => (
            client
                .create_addon_submission(&addon_id)
                .await
                .map_err(err)?,
            true,
        ),
    };
    if let Some(status) = str_at(&submission, "/status") {
        if pending.is_some() && LOCKED_STATUSES.contains(&status) {
            return Err(format!(
                "the pending add-on submission is {status}; wait for it to finish before changing it"
            ));
        }
    }
    let locales = set_listing_icons(&mut submission, &file_name, locale)?;
    let submission_id = str_at(&submission, "/id").map(str::to_string);
    let result = |submission_id| AddonIconSet {
        dry_run,
        addon_id: addon_id.clone(),
        product_id: addon_product_id.clone(),
        submission_id,
        created_submission: created,
        locales: locales.clone(),
        file_name: file_name.clone(),
    };
    if dry_run {
        return Ok(result(if created { None } else { submission_id }));
    }

    let submission_id = submission_id.ok_or("the add-on submission has no id")?;
    let upload_url = str_at(&submission, "/fileUploadUrl")
        .ok_or("the add-on submission has no fileUploadUrl")?
        .to_string();
    client
        .update_addon_submission(&addon_id, &submission_id, &submission)
        .await
        .map_err(err)?;
    client
        .upload_submission_files(&upload_url, icon_zip(&file_name, &png)?)
        .await
        .map_err(|e| format!("updated the submission, but uploading the icon failed: {e}"))?;

    let back = client
        .get_addon_submission(&addon_id, &submission_id)
        .await
        .map_err(|e| format!("uploaded, but reading the submission back failed: {e}"))?;
    for code in &locales {
        let stored = back.pointer(&format!("/listings/{code}/icon/fileName"));
        if stored.and_then(Value::as_str) != Some(file_name.as_str()) {
            return Err(format!(
                "uploaded, but the {code} listing reads back without {file_name} as its icon"
            ));
        }
    }
    Ok(result(Some(submission_id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_icons_go_on_every_locale_or_the_one_asked_for() {
        let mut sub = serde_json::json!({ "listings": {
            "en-us": { "title": "Personal Pro" }, "sv-se": { "title": "Personal Pro" } } });
        let mut all = set_listing_icons(&mut sub, "pro.png", None).unwrap();
        all.sort();
        assert_eq!(all, vec!["en-us", "sv-se"]);
        assert_eq!(
            sub["listings"]["sv-se"]["icon"]["fileStatus"],
            "PendingUpload"
        );
        assert_eq!(sub["listings"]["en-us"]["title"], "Personal Pro");

        let mut one = serde_json::json!({ "listings": { "en-us": {} } });
        assert!(set_listing_icons(&mut one, "pro.png", Some("de-de")).is_err());
        assert_eq!(
            set_listing_icons(&mut one, "pro.png", Some("en-us")).unwrap(),
            vec!["en-us"]
        );
    }

    #[test]
    fn the_icon_zip_holds_the_file_at_its_root() {
        let zip = icon_zip("pro.png", b"png-bytes").unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).unwrap();
        assert_eq!(archive.len(), 1);
        let mut entry = archive.by_name("pro.png").unwrap();
        let mut body = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut body).unwrap();
        assert_eq!(body, b"png-bytes");
    }

    #[test]
    fn an_addon_summary_reads_the_pending_submission() {
        let row = addon_summary(&serde_json::json!({
            "id": "9NBL", "productId": "personal_pro_monthly", "productType": "Subscription",
            "pendingInAppProductSubmission": { "id": "1152921" } }));
        assert_eq!(row.product_id.as_deref(), Some("personal_pro_monthly"));
        assert_eq!(row.pending_submission_id.as_deref(), Some("1152921"));
    }

    fn submission() -> Value {
        serde_json::json!({"applicationPackages": [
            {"fileName": "a.msix", "version": "1.3.1.0", "fileStatus": "PendingDelete"},
            {"fileName": "b.msix", "version": "1.3.2.0", "fileStatus": "Uploaded"},
            {"fileName": "c.msix", "version": "1.3.2.0"}
        ]})
    }

    #[test]
    fn package_versions_skip_packages_pending_delete() {
        assert_eq!(package_versions(&submission()), vec!["1.3.2.0", "1.3.2.0"]);
        assert!(package_versions(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn pending_upload_is_matched_by_file_name() {
        let draft = serde_json::json!({"applicationPackages": [
            {"fileName": "siGit-1.3.2.0.msix", "fileStatus": "PendingUpload"}]});
        assert!(package_versions(&draft).is_empty());
        assert_eq!(pending_file_names(&draft), vec!["siGit-1.3.2.0.msix"]);
        assert!(check_expected_version(&draft, "1.3.2.0").is_ok());
    }

    #[test]
    fn pending_upload_with_another_version_is_a_mismatch() {
        let draft = serde_json::json!({"applicationPackages": [
            {"fileName": "siGit-1.3.1.0.msix", "fileStatus": "PendingUpload"}]});
        let error = check_expected_version(&draft, "1.3.2.0").unwrap_err();
        assert!(error.contains("siGit-1.3.1.0.msix") && error.contains("not 1.3.2.0"));
    }

    #[test]
    fn pending_upload_without_a_version_in_its_name_cannot_be_checked() {
        let draft = serde_json::json!({"applicationPackages": [
            {"fileName": "release.msix", "fileStatus": "PendingUpload"}]});
        let error = check_expected_version(&draft, "1.3.2.0").unwrap_err();
        assert!(error.contains("has no version yet"));
    }

    #[test]
    fn file_name_versions_are_matched_whole() {
        assert_eq!(versions_in_file_name("siGit-1.3.2.0.msix"), vec!["1.3.2.0"]);
        assert_eq!(
            versions_in_file_name("x64_1.3.2.0.msixupload"),
            vec!["1.3.2.0"]
        );
        assert_eq!(versions_in_file_name("x64_11.3.2.0.msix"), vec!["11.3.2.0"]);
        assert_eq!(versions_in_file_name("a-1.3.2.01.msix"), vec!["1.3.2.01"]);
        assert!(versions_in_file_name("release.msix").is_empty());
        for name in ["x64_11.3.2.0.msix", "a-1.3.2.01.msix"] {
            let draft = serde_json::json!({"applicationPackages": [
                {"fileName": name, "fileStatus": "PendingUpload"}]});
            assert!(check_expected_version(&draft, "1.3.2.0").is_err());
        }
    }

    #[test]
    fn ingested_package_is_still_checked_by_version() {
        assert!(check_expected_version(&submission(), "1.3.2.0").is_ok());
        assert!(check_expected_version(&submission(), "1.3.1.0").is_err());
    }

    #[test]
    fn still_committing_reports_unknown_not_a_mismatch() {
        // The deadline ran out while an older package version is carried over
        // and the new one is not ingested yet: unknown, never a false
        // mismatch, and nothing stale reported as the post-commit result.
        let r = post_commit_check(
            "s1",
            "CommitStarted",
            vec!["1.3.1.0".into()],
            Some("1.3.2.0"),
        );
        assert_eq!(r, Ok(None));
    }

    #[test]
    fn ingested_versions_are_checked_strictly_after_the_commit() {
        assert_eq!(
            post_commit_check(
                "s1",
                "PreProcessing",
                vec!["1.3.2.0".into()],
                Some("1.3.2.0")
            ),
            Ok(Some(vec!["1.3.2.0".into()]))
        );
        let wrong = post_commit_check(
            "s1",
            "PreProcessing",
            vec!["1.3.1.0".into()],
            Some("1.3.2.0"),
        )
        .unwrap_err();
        assert!(wrong.contains("was committed") && wrong.contains("not 1.3.2.0"));
    }

    #[test]
    fn an_unreadable_post_commit_version_list_is_unknown() {
        // Empty means the extra read failed or returned nothing; don't call
        // that a mismatch.
        assert_eq!(
            post_commit_check("s1", "PreProcessing", vec![], Some("1.3.2.0")),
            Ok(None)
        );
        assert_eq!(
            post_commit_check("s1", "PreProcessing", vec!["1.3.2.0".into()], None),
            Ok(Some(vec!["1.3.2.0".into()]))
        );
    }

    #[test]
    fn only_committed_statuses_are_past_commit() {
        for s in ["CommitStarted", "PreProcessing", "Certification", "Release"] {
            assert!(is_past_commit(s));
        }
        for s in ["PendingCommit", "CommitFailed", ""] {
            assert!(!is_past_commit(s));
        }
    }

    #[test]
    fn failed_statuses_list_their_errors() {
        let status = serde_json::json!({"status": "CommitFailed", "statusDetails": {"errors": [
            {"code": "InvalidState", "details": "bad package"}]}});
        assert_eq!(status_errors(&status), vec!["InvalidState: bad package"]);
    }

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
