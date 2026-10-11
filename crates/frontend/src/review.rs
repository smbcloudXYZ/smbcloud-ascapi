//! Submitting an App Store Version for App Review.
//!
//! Split into a read-only [`plan_submission`] and a [`submit_for_review`]
//! that carries the plan out, so `--dry-run` can show exactly what would
//! happen (reuse an open submission or create one, add the version or not)
//! without writing anything.

use serde::Serialize;
use smbcloud_ascapi_aso::app_store_version::Platform;
use smbcloud_ascapi_aso::prelude::*;
use smbcloud_ascapi_aso::review_submission::OPEN_STATES;
use smbcloud_ascapi_core::Client;

/// What [`submit_for_review`] will do, worked out from reads only.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct SubmissionPlan {
    pub app_id: String,
    pub app_store_version_id: String,
    pub version_string: Option<String>,
    pub platform: String,
    pub app_version_state: Option<String>,
    /// An open submission for this app and platform to reuse: a draft
    /// (`READY_FOR_REVIEW`), or one App Review sent back
    /// (`UNRESOLVED_ISSUES`), which is how a rejected version is
    /// resubmitted. `None` means a new one gets created.
    pub existing_submission_id: Option<String>,
    /// That submission's state, when there is one.
    pub existing_submission_state: Option<String>,
    /// Whether the open submission already carries an item. App Store
    /// Connect allows one item per version, so when this is true the item
    /// step is skipped.
    pub existing_submission_has_items: bool,
    /// Why this version can't be submitted, when its state says it already
    /// was (or is live). [`submit_for_review`] refuses a plan that has one.
    pub blocked: Option<String>,
}

/// The result of a submission, as reported to callers.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct SubmittedReview {
    pub submission_id: String,
    pub state: Option<String>,
    pub submitted_date: Option<String>,
    pub plan: SubmissionPlan,
}

pub async fn plan_submission(
    client: &Client,
    app_id: &str,
    app_store_version_id: &str,
) -> Result<SubmissionPlan, String> {
    let version = client
        .get_app_store_version(app_store_version_id)
        .await
        .map_err(|error| error.to_string())?;
    let platform = version.attributes.platform;

    let open = client
        .list_review_submissions(app_id, Some(platform), &OPEN_STATES)
        .await
        .map_err(|error| error.to_string())?;
    let existing = open.into_iter().next();
    let existing_submission_state = existing
        .as_ref()
        .and_then(|submission| submission.attributes.state.clone());

    let existing_submission_has_items = match &existing {
        Some(submission) => !client
            .list_review_submission_items(&submission.id)
            .await
            .map_err(|error| error.to_string())?
            .is_empty(),
        None => false,
    };

    let blocked = version
        .attributes
        .app_version_state
        .as_deref()
        .and_then(blocked_reason);

    Ok(SubmissionPlan {
        blocked,
        app_id: app_id.to_string(),
        app_store_version_id: app_store_version_id.to_string(),
        version_string: version.attributes.version_string,
        platform: platform_name(platform).to_string(),
        app_version_state: version.attributes.app_version_state,
        existing_submission_id: existing.map(|submission| submission.id),
        existing_submission_state,
        existing_submission_has_items,
    })
}

/// Carry out a plan from [`plan_submission`]: reuse or create the
/// submission, add the version unless it's already on it, then submit.
///
/// An open submission that already has an item is assumed to hold this
/// version, because App Store Connect keeps one open submission per app
/// and platform and this is the platform's only version in an editable
/// state. If it holds something else, App Store Connect rejects the
/// submit and the error says so.
pub async fn submit_for_review(
    client: &Client,
    plan: SubmissionPlan,
) -> Result<SubmittedReview, String> {
    if let Some(reason) = &plan.blocked {
        return Err(reason.clone());
    }
    let platform = platform_from_name(&plan.platform)?;
    let submission_id = match &plan.existing_submission_id {
        Some(id) => id.clone(),
        None => {
            client
                .create_review_submission(&plan.app_id, platform)
                .await
                .map_err(|error| error.to_string())?
                .id
        }
    };

    if !plan.existing_submission_has_items {
        client
            .add_app_store_version_to_review_submission(&submission_id, &plan.app_store_version_id)
            .await
            .map_err(|error| error.to_string())?;
    }

    let submitted = client
        .submit_review_submission(&submission_id)
        .await
        .map_err(|error| error.to_string())?;

    Ok(SubmittedReview {
        submission_id: submitted.id,
        state: submitted.attributes.state,
        submitted_date: submitted.attributes.submitted_date,
        plan,
    })
}

/// States in which a version has already gone to App Review or is past
/// it. Submitting again would only fail at the API, after the submission
/// and item had been created, so stop before any write.
fn blocked_reason(state: &str) -> Option<String> {
    const SUBMITTED_OR_LIVE: [&str; 9] = [
        "WAITING_FOR_REVIEW",
        "IN_REVIEW",
        "PENDING_APPLE_RELEASE",
        "PENDING_DEVELOPER_RELEASE",
        "PROCESSING_FOR_DISTRIBUTION",
        "READY_FOR_DISTRIBUTION",
        "READY_FOR_SALE",
        "ACCEPTED",
        "REPLACED_WITH_NEW_VERSION",
    ];
    SUBMITTED_OR_LIVE.contains(&state).then(|| {
        format!(
            "this version is {state}, so it has already been submitted; \
             `review-submissions list` shows its submission, and \
             `review-submissions cancel` withdraws one still waiting for review"
        )
    })
}

fn platform_name(platform: Platform) -> &'static str {
    match platform {
        Platform::Ios => "IOS",
        Platform::MacOs => "MAC_OS",
        Platform::TvOs => "TV_OS",
        Platform::VisionOs => "VISION_OS",
    }
}

fn platform_from_name(name: &str) -> Result<Platform, String> {
    match name {
        "IOS" => Ok(Platform::Ios),
        "MAC_OS" => Ok(Platform::MacOs),
        "TV_OS" => Ok(Platform::TvOs),
        "VISION_OS" => Ok(Platform::VisionOs),
        other => Err(format!("unknown platform {other:?} in submission plan")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_a_version_that_was_already_submitted() {
        assert!(blocked_reason("WAITING_FOR_REVIEW").is_some());
        assert!(blocked_reason("READY_FOR_DISTRIBUTION").is_some());
        assert_eq!(blocked_reason("PREPARE_FOR_SUBMISSION"), None);
        assert_eq!(blocked_reason("DEVELOPER_REJECTED"), None);
        assert_eq!(blocked_reason("REJECTED"), None);
    }

    #[test]
    fn platform_names_round_trip() {
        for platform in [
            Platform::Ios,
            Platform::MacOs,
            Platform::TvOs,
            Platform::VisionOs,
        ] {
            assert_eq!(platform_from_name(platform_name(platform)), Ok(platform));
        }
        assert!(platform_from_name("WATCH_OS").is_err());
    }
}
