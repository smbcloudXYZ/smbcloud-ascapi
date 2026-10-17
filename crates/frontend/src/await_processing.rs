//! Block until App Store Connect finishes processing an uploaded build.
//!
//! `xcrun altool --upload-package` returns as soon as the binary is
//! accepted for delivery, long before App Store Connect finishes
//! processing it. Processing is the step that turns a freshly uploaded
//! build `VALID` (ready to attach to a version) or `INVALID`/`FAILED`
//! (a binary rejection) — the signal that actually confirms an upload
//! or a binary-rejection fix worked. This module polls
//! [`BuildsApi::list_builds`] until a build reaches a terminal state or a
//! timeout elapses.
//!
//! The classification of a `processingState` string is a pure function
//! ([`classify`]) so the terminal-vs-pending decision is unit-tested
//! against the exact strings Apple returns, with the async polling loop
//! kept separate. Like every result type in this crate, [`AwaitOutcome`]
//! carries only identifiers and state — never key or binary material.

use std::time::Duration;

use serde::Serialize;
use smbcloud_ascapi_aso::build::{Build, BuildFilter};
use smbcloud_ascapi_aso::prelude::*;
use smbcloud_ascapi_core::Client;

/// How a build's `processingState` reads: either it's settled (and how), or
/// App Store Connect is still working on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessingClass {
    /// `VALID` — processing finished and the build can be attached to a
    /// version.
    Valid,
    /// `INVALID` or `FAILED` — a binary rejection; processing is over but
    /// the build is unusable.
    Failed,
    /// `PROCESSING`, a missing state, or any string Apple adds later: keep
    /// polling. Treating the unknown as pending means a new terminal state
    /// we don't recognize just times out rather than being reported as
    /// success.
    Pending,
}

impl ProcessingClass {
    /// Whether this class ends the poll. `Valid` and `Failed` are terminal;
    /// `Pending` is not.
    pub fn is_terminal(self) -> bool {
        !matches!(self, ProcessingClass::Pending)
    }
}

/// Classify a build's `processingState` (case-insensitively, since the field
/// is a fixed Apple enum but we don't want a casing change to read as
/// "still pending" forever).
pub fn classify(processing_state: Option<&str>) -> ProcessingClass {
    match processing_state
        .map(str::trim)
        .unwrap_or("")
        .to_ascii_uppercase()
        .as_str()
    {
        "VALID" => ProcessingClass::Valid,
        "INVALID" | "FAILED" => ProcessingClass::Failed,
        _ => ProcessingClass::Pending,
    }
}

/// How long to wait and how often to re-check, for [`await_processing`].
#[derive(Debug, Clone, Copy)]
pub struct AwaitConfig {
    /// Delay between polls.
    pub interval: Duration,
    /// Give up after this long without a terminal state.
    pub timeout: Duration,
}

impl Default for AwaitConfig {
    fn default() -> Self {
        // 30s between polls, 30min ceiling: processing usually finishes in a
        // few minutes but can stall, and a tighter poll just burns API quota.
        Self {
            interval: Duration::from_secs(30),
            timeout: Duration::from_secs(30 * 60),
        }
    }
}

/// The result of waiting on a build, as reported to callers. Identifiers and
/// state only — no key or binary material, per the crate's result-type rule.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct AwaitOutcome {
    /// The build's App Store Connect id.
    pub build_id: String,
    /// The build (upload) number, e.g. `"42"`.
    pub version: Option<String>,
    /// The `processingState` the build ended on, verbatim from Apple.
    pub processing_state: Option<String>,
    /// Whether that state is `VALID`. `false` means `INVALID`/`FAILED`.
    pub valid: bool,
    /// Whether the wait timed out before the build settled. When true,
    /// `valid` is false and `processing_state` is the last state seen.
    pub timed_out: bool,
    /// How long the wait took, in seconds.
    pub waited_secs: u64,
}

/// Shape an [`AwaitOutcome`] from the build the poll settled on.
fn outcome(build: &Build, timed_out: bool, waited: Duration) -> AwaitOutcome {
    let processing_state = build.attributes.processing_state.clone();
    AwaitOutcome {
        build_id: build.id.clone(),
        version: build.attributes.version.clone(),
        valid: !timed_out && classify(processing_state.as_deref()) == ProcessingClass::Valid,
        timed_out,
        processing_state,
        waited_secs: waited.as_secs(),
    }
}

/// Poll [`BuildsApi::list_builds`] for `app_id` narrowed by `filter` until the
/// newest matching build reaches a terminal `processingState`, or `config`'s
/// timeout elapses.
///
/// `list_builds` returns newest-first, so the first element is the build a
/// fresh upload produced; narrow with `filter.version` /
/// `filter.pre_release_version` when more than one upload is in flight.
///
/// Returns `Err` only when the API call fails or no matching build exists at
/// all. A build that stays `PROCESSING` past the timeout is a successful
/// `Ok` with `timed_out: true` — the caller decides whether a timeout is a
/// failure, the same way `valid: false` signals a binary rejection.
pub async fn await_processing(
    client: &Client,
    app_id: &str,
    filter: &BuildFilter,
    config: AwaitConfig,
) -> Result<AwaitOutcome, String> {
    let start = std::time::Instant::now();
    loop {
        let builds = client
            .list_builds(app_id, filter)
            .await
            .map_err(|error| error.to_string())?;
        let build = builds
            .into_iter()
            .next()
            .ok_or_else(|| format!("no build found for app {app_id} matching the given filter"))?;

        let class = classify(build.attributes.processing_state.as_deref());
        if class.is_terminal() {
            return Ok(outcome(&build, false, start.elapsed()));
        }

        let elapsed = start.elapsed();
        if elapsed >= config.timeout {
            return Ok(outcome(&build, true, elapsed));
        }

        // Don't overshoot the timeout waiting for the next poll.
        let remaining = config.timeout - elapsed;
        tokio::time::sleep(config.interval.min(remaining)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smbcloud_ascapi_aso::build::BuildAttributes;
    use smbcloud_ascapi_core::jsonapi::Resource;

    #[test]
    fn classifies_terminal_and_pending_states() {
        // Pin the exact Apple strings: the whole point is to not report an
        // unrecognized or still-processing state as success.
        assert_eq!(classify(Some("VALID")), ProcessingClass::Valid);
        assert_eq!(classify(Some("INVALID")), ProcessingClass::Failed);
        assert_eq!(classify(Some("FAILED")), ProcessingClass::Failed);
        assert_eq!(classify(Some("PROCESSING")), ProcessingClass::Pending);
    }

    #[test]
    fn classifies_unknown_and_missing_as_pending() {
        // A state we don't recognize (a casing change, a new Apple value, or
        // an absent field) must read as pending so it times out rather than
        // being mistaken for a terminal result.
        assert_eq!(classify(None), ProcessingClass::Pending);
        assert_eq!(classify(Some("")), ProcessingClass::Pending);
        assert_eq!(classify(Some("SOMETHING_NEW")), ProcessingClass::Pending);
    }

    #[test]
    fn classify_is_case_and_whitespace_insensitive() {
        assert_eq!(classify(Some(" valid ")), ProcessingClass::Valid);
        assert_eq!(classify(Some("invalid")), ProcessingClass::Failed);
    }

    #[test]
    fn only_valid_and_failed_are_terminal() {
        assert!(ProcessingClass::Valid.is_terminal());
        assert!(ProcessingClass::Failed.is_terminal());
        assert!(!ProcessingClass::Pending.is_terminal());
    }

    fn build(id: &str, state: Option<&str>) -> Build {
        Resource {
            id: id.to_string(),
            resource_type: "builds".to_string(),
            attributes: BuildAttributes {
                version: Some("42".to_string()),
                uploaded_date: Some("2024-01-02T03:04:05Z".to_string()),
                expired: Some(false),
                processing_state: state.map(str::to_string),
                min_os_version: Some("17.0".to_string()),
                uses_non_exempt_encryption: None,
            },
            relationships: None,
        }
    }

    #[test]
    fn outcome_reports_valid_build() {
        let out = outcome(&build("B1", Some("VALID")), false, Duration::from_secs(5));
        assert_eq!(out.build_id, "B1");
        assert_eq!(out.version.as_deref(), Some("42"));
        assert_eq!(out.processing_state.as_deref(), Some("VALID"));
        assert!(out.valid);
        assert!(!out.timed_out);
        assert_eq!(out.waited_secs, 5);
    }

    #[test]
    fn outcome_reports_failed_build_as_not_valid() {
        let out = outcome(&build("B2", Some("INVALID")), false, Duration::from_secs(1));
        assert!(!out.valid);
        assert!(!out.timed_out);
    }

    #[test]
    fn timed_out_outcome_is_never_valid() {
        // Even if the last state seen were VALID, a timed-out wait must not
        // claim success: `valid` is gated on `!timed_out`.
        let out = outcome(&build("B3", Some("VALID")), true, Duration::from_secs(1800));
        assert!(out.timed_out);
        assert!(!out.valid);
    }
}
