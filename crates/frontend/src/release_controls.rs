//! How an approved App Store version goes live: the release type, the
//! release request for a manual release, and the phased release.
//!
//! Every write is followed by a read, and a read that disagrees with the
//! write is an error, so a script never takes a rejected change for a
//! success. The result types carry identifiers and states only.

use serde::Serialize;
use smbcloud_ascapi_aso::app_store_version::{
    phased_release_create_body, phased_release_update_body, AppStoreVersion,
    AppStoreVersionUpdateFields, PhasedRelease, PhasedReleaseAction, ReleaseRequestBody,
    ReleaseType, PENDING_DEVELOPER_RELEASE,
};
use smbcloud_ascapi_aso::prelude::*;
use smbcloud_ascapi_core::Client;

fn fail(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// Whether `value` looks like an ISO 8601 date-time with an offset, such as
/// `2026-11-10T09:00:00+01:00` or `2026-11-10T08:00:00Z`.
fn is_iso8601_with_offset(value: &str) -> bool {
    let bytes = value.as_bytes();
    let digits = |range: std::ops::Range<usize>| {
        bytes
            .get(range)
            .is_some_and(|b| !b.is_empty() && b.iter().all(u8::is_ascii_digit))
    };
    let shape = digits(0..4)
        && bytes.get(4) == Some(&b'-')
        && digits(5..7)
        && bytes.get(7) == Some(&b'-')
        && digits(8..10)
        && bytes.get(10) == Some(&b'T')
        && digits(11..13)
        && bytes.get(13) == Some(&b':')
        && digits(14..16)
        && bytes.get(16) == Some(&b':')
        && digits(17..19);
    if !shape {
        return false;
    }
    let mut rest = &value[19..];
    if let Some(fraction) = rest.strip_prefix('.') {
        let end = fraction
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(fraction.len());
        if end == 0 {
            return false;
        }
        rest = &fraction[end..];
    }
    if rest == "Z" {
        return true;
    }
    let Some(offset) = rest.strip_prefix('+').or_else(|| rest.strip_prefix('-')) else {
        return false;
    };
    let offset = offset.as_bytes();
    offset.len() == 5
        && offset[2] == b':'
        && [0, 1, 3, 4].iter().all(|&i| offset[i].is_ascii_digit())
}

/// Check the release-control flags against each other before anything is
/// sent: `SCHEDULED` needs a date, and a date needs `SCHEDULED`.
pub fn validate_release_controls(
    release_type: Option<ReleaseType>,
    earliest_release_date: Option<&str>,
) -> Result<(), String> {
    match (release_type, earliest_release_date) {
        (Some(ReleaseType::Scheduled), None) => {
            Err("--release-type SCHEDULED needs --earliest-release-date".to_string())
        }
        (Some(ReleaseType::Scheduled), Some(date)) if !is_iso8601_with_offset(date) => Err(format!(
            "--earliest-release-date {date:?} is not an ISO 8601 date-time with an offset, e.g. 2026-11-10T09:00:00+01:00"
        )),
        (Some(ReleaseType::Scheduled), Some(_)) => Ok(()),
        (_, Some(_)) => Err("--earliest-release-date only goes with --release-type SCHEDULED".to_string()),
        (_, None) => Ok(()),
    }
}

/// Apply `fields` to the version, then read it back and fail unless the
/// release type is the one asked for.
pub async fn update_release_controls(
    client: &Client,
    id: &str,
    fields: AppStoreVersionUpdateFields,
) -> Result<AppStoreVersion, String> {
    let wanted = fields.release_type;
    client
        .update_app_store_version(id, fields)
        .await
        .map_err(fail)?;
    let read = client.get_app_store_version(id).await.map_err(fail)?;
    if let Some(wanted) = wanted {
        if read.attributes.release_type.as_deref() != Some(wanted.as_str()) {
            return Err(format!(
                "asked for release type {}, but the version reports {}",
                wanted.as_str(),
                read.attributes.release_type.as_deref().unwrap_or("none")
            ));
        }
    }
    Ok(read)
}

/// The result of [`release_version`].
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseOutcome {
    pub version_id: String,
    pub dry_run: bool,
    /// The version's state before the request.
    pub state_before: Option<String>,
    /// The state read back after the request. `None` on a dry run.
    pub state_after: Option<String>,
    /// What was sent, or on a dry run what would be.
    pub sent: ReleaseRequestBody,
}

/// Why a version cannot be released, or `None` when it can.
pub fn release_blocker(state: Option<&str>) -> Option<String> {
    if state == Some(PENDING_DEVELOPER_RELEASE) {
        None
    } else {
        Some(format!(
            "the version is {}, not {PENDING_DEVELOPER_RELEASE}; only an approved version waiting for its developer can be released",
            state.unwrap_or("without a state")
        ))
    }
}

/// Release an approved version that waits in `PENDING_DEVELOPER_RELEASE`,
/// then read its state back. Refuses any other state without sending.
pub async fn release_version(
    client: &Client,
    id: &str,
    dry_run: bool,
) -> Result<ReleaseOutcome, String> {
    let before = client.get_app_store_version(id).await.map_err(fail)?;
    let state_before = before.attributes.app_version_state.clone();
    if let Some(reason) = release_blocker(state_before.as_deref()) {
        return Err(reason);
    }
    let sent = ReleaseRequestBody::for_version(id);
    if dry_run {
        return Ok(ReleaseOutcome {
            version_id: id.to_string(),
            dry_run,
            state_before,
            state_after: None,
            sent,
        });
    }
    client.release_app_store_version(id).await.map_err(fail)?;
    let after = client.get_app_store_version(id).await.map_err(fail)?;
    Ok(ReleaseOutcome {
        version_id: id.to_string(),
        dry_run,
        state_before,
        state_after: after.attributes.app_version_state,
        sent,
    })
}

/// The result of [`phased_release`].
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhasedReleaseOutcome {
    pub version_id: String,
    pub dry_run: bool,
    /// The release was already in the requested state; nothing was sent.
    pub noop: bool,
    /// The phased release before the change, if the version had one.
    pub before: Option<PhasedRelease>,
    /// What was sent, or on a dry run what would be.
    pub sent: Option<serde_json::Value>,
    /// The phased release read back after the change. `None` on a dry run
    /// and on a no-op.
    pub after: Option<PhasedRelease>,
}

/// Create, pause, resume or complete a version's phased release, then read
/// it back and fail unless it is in the state the action leads to.
pub async fn phased_release(
    client: &Client,
    version_id: &str,
    action: PhasedReleaseAction,
    dry_run: bool,
) -> Result<PhasedReleaseOutcome, String> {
    let before = client
        .get_app_store_version_phased_release(version_id)
        .await
        .map_err(fail)?;
    let current = before
        .as_ref()
        .and_then(|r| r.attributes.phased_release_state.as_deref());
    let noop = action.check(current)?;
    let target = action.target_state();
    let sent = match (&before, noop) {
        (_, true) => None,
        (None, false) => {
            Some(serde_json::to_value(phased_release_create_body(version_id)).map_err(fail)?)
        }
        (Some(existing), false) => Some(
            serde_json::to_value(phased_release_update_body(&existing.id, target)).map_err(fail)?,
        ),
    };
    if dry_run || noop {
        return Ok(PhasedReleaseOutcome {
            version_id: version_id.to_string(),
            dry_run,
            noop,
            before,
            sent,
            after: None,
        });
    }
    match &before {
        None => client
            .create_phased_release(version_id)
            .await
            .map_err(fail)?,
        Some(existing) => client
            .update_phased_release(&existing.id, target)
            .await
            .map_err(fail)?,
    };
    let after = client
        .get_app_store_version_phased_release(version_id)
        .await
        .map_err(fail)?
        .ok_or_else(|| "sent the change, but the version reports no phased release".to_string())?;
    if after.attributes.phased_release_state.as_deref() != Some(target) {
        return Err(format!(
            "asked for phased release state {target}, but the version reports {}",
            after
                .attributes
                .phased_release_state
                .as_deref()
                .unwrap_or("none")
        ));
    }
    Ok(PhasedReleaseOutcome {
        version_id: version_id.to_string(),
        dry_run,
        noop,
        before,
        sent,
        after: Some(after),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_need_a_time_and_an_offset() {
        for good in [
            "2026-11-10T09:00:00+01:00",
            "2026-11-10T08:00:00Z",
            "2026-11-10T08:00:00.250-08:00",
        ] {
            assert!(is_iso8601_with_offset(good), "{good}");
        }
        for bad in [
            "2026-11-10",
            "2026-11-10T09:00:00",
            "2026-11-10 09:00:00Z",
            "2026-11-10T09:00:00+0100",
            "tomorrow",
            "",
        ] {
            assert!(!is_iso8601_with_offset(bad), "{bad}");
        }
    }

    #[test]
    fn scheduled_and_the_date_go_together() {
        let date = "2026-11-10T09:00:00+01:00";
        assert!(validate_release_controls(Some(ReleaseType::Scheduled), Some(date)).is_ok());
        assert!(validate_release_controls(Some(ReleaseType::Scheduled), None).is_err());
        assert!(validate_release_controls(Some(ReleaseType::Scheduled), Some("soon")).is_err());
        assert!(validate_release_controls(Some(ReleaseType::Manual), Some(date)).is_err());
        assert!(validate_release_controls(None, Some(date)).is_err());
        assert!(validate_release_controls(Some(ReleaseType::Manual), None).is_ok());
        assert!(validate_release_controls(None, None).is_ok());
    }

    #[test]
    fn only_a_version_pending_developer_release_can_be_released() {
        assert!(release_blocker(Some("PENDING_DEVELOPER_RELEASE")).is_none());
        for state in [
            Some("WAITING_FOR_REVIEW"),
            Some("READY_FOR_SALE"),
            Some("PREPARE_FOR_SUBMISSION"),
            None,
        ] {
            assert!(release_blocker(state).is_some(), "{state:?}");
        }
    }
}
