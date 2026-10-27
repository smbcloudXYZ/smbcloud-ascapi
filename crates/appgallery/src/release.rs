//! Release calls: submit for review, phased release, the release time, the
//! GMS dependency flag, and the package list and its compile status.
//!
//! The "download mode" submit and package calls are not here: Huawei fetches
//! the package from a public URL and reports back to a callback server, which
//! a CLI does not have.

use reqwest::Method;
use serde::{Deserialize, Serialize};

use crate::client::AppGalleryClient;
use crate::error::{Error, Result};
use crate::publish::{parse_ret, RetCode};

/// Phased-release settings. Times are `yyyy-MM-ddTHH:mm:ssZZ`, e.g.
/// `2026-11-01T09:00:00+0100`; the percentage is `0.00`-`100.00` with no `%`.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhasedRelease {
    /// `SUSPEND` or `RELEASE` when updating a running phased release.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phased_release_start_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phased_release_end_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phased_release_percent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phased_release_description: Option<String>,
}

/// `HH:mm:ss` plus a `±HHMM` offset after a `yyyy-MM-ddT` date.
pub fn is_release_time(value: &str) -> bool {
    let b = value.as_bytes();
    let digits = |r: std::ops::Range<usize>| r.into_iter().all(|i| b[i].is_ascii_digit());
    b.len() == 24
        && digits(0..4)
        && b[4] == b'-'
        && digits(5..7)
        && b[7] == b'-'
        && digits(8..10)
        && b[10] == b'T'
        && digits(11..13)
        && b[13] == b':'
        && digits(14..16)
        && b[16] == b':'
        && digits(17..19)
        && (b[19] == b'+' || b[19] == b'-')
        && digits(20..24)
}

/// `0.00`-`100.00`, at most two decimals, no `%`.
pub fn is_percent(value: &str) -> bool {
    let (whole, frac) = value.split_once('.').unwrap_or((value, ""));
    if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) || frac.len() > 2 {
        return false;
    }
    if !frac.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    value
        .parse::<f64>()
        .map(|v| (0.0..=100.0).contains(&v))
        .unwrap_or(false)
}

impl PhasedRelease {
    pub fn validate(&self) -> Result<()> {
        for (field, value) in [
            ("phasedReleaseStartTime", &self.phased_release_start_time),
            ("phasedReleaseEndTime", &self.phased_release_end_time),
        ] {
            if let Some(v) = value {
                if !is_release_time(v) {
                    return Err(Error::Invalid {
                        field,
                        detail: format!("{v:?} is not yyyy-MM-ddTHH:mm:ss+HHMM"),
                    });
                }
            }
        }
        if let Some(p) = &self.phased_release_percent {
            if !is_percent(p) {
                return Err(Error::Invalid {
                    field: "phasedReleasePercent",
                    detail: format!("{p:?} is not 0.00-100.00"),
                });
            }
        }
        if let Some(state) = &self.state {
            if state != "SUSPEND" && state != "RELEASE" {
                return Err(Error::Invalid {
                    field: "state",
                    detail: "use SUSPEND or RELEASE".to_string(),
                });
            }
        }
        if let Some(d) = &self.phased_release_description {
            if d.chars().count() > 500 {
                return Err(Error::TooLong {
                    field: "phasedReleaseDescription",
                    len: d.chars().count(),
                    max: 500,
                });
            }
        }
        Ok(())
    }
}

/// Options for `POST app-submit`.
#[derive(Debug, Clone, Default)]
pub struct Submit {
    /// Release at this time after approval; immediately when unset.
    pub release_time: Option<String>,
    /// 10-300 characters when set.
    pub remark: Option<String>,
    /// A phased release (`releaseType=3`); needs a version already released.
    pub phased: Option<PhasedRelease>,
}

impl Submit {
    pub fn validate(&self) -> Result<()> {
        if let Some(t) = &self.release_time {
            if !is_release_time(t) {
                return Err(Error::Invalid {
                    field: "releaseTime",
                    detail: format!("{t:?} is not yyyy-MM-ddTHH:mm:ss+HHMM"),
                });
            }
        }
        if let Some(r) = &self.remark {
            let n = r.chars().count();
            if !(10..=300).contains(&n) {
                return Err(Error::Invalid {
                    field: "remark",
                    detail: format!("{n} characters; it must be 10-300"),
                });
            }
        }
        if let Some(p) = &self.phased {
            p.validate()?;
            if p.phased_release_start_time.is_none()
                || p.phased_release_end_time.is_none()
                || p.phased_release_percent.is_none()
                || p.phased_release_description.is_none()
            {
                return Err(Error::Invalid {
                    field: "phased",
                    detail: "a phased submit needs start, end, percent and description".into(),
                });
            }
        }
        Ok(())
    }
}

/// When a version in Releasing state goes live.
#[derive(Debug, Clone)]
pub enum ReleaseTimeChange {
    /// From a scheduled time to now (`changeType` 2).
    Immediately,
    /// To a new scheduled time (`changeType` 3).
    At(String),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageInfo {
    /// The package id other calls take as `pkgIds`.
    pub pkg_version: String,
    pub file_name: Option<String>,
    pub version_code: Option<i64>,
    pub version_name: Option<String>,
    pub package_size: Option<i64>,
    pub upload_time: Option<serde_json::Value>,
    pub sha_code: Option<String>,
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageList {
    #[serde(default)]
    pub ret_count: i64,
    #[serde(default)]
    pub total_count: i64,
    #[serde(default)]
    pub pkg_list: Vec<PackageInfo>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageState {
    pub pkg_id: String,
    /// 0 normal, 1 parsing, 2 failed (the package is unusable).
    pub success_status: Option<i64>,
    /// Deprecated by Huawei in favour of `success_status`.
    pub aab_compile_status: Option<i64>,
    pub fail_reason: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackageListEnvelope {
    ret: RetCode,
    #[serde(flatten)]
    body: PackageList,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackageStateEnvelope {
    ret: RetCode,
    #[serde(default)]
    pkg_state_list: Vec<PackageState>,
}

fn parse_package_list(bytes: &[u8]) -> Result<PackageList> {
    let envelope: PackageListEnvelope = serde_json::from_slice(bytes)?;
    envelope.ret.check()?;
    Ok(envelope.body)
}

fn parse_package_states(bytes: &[u8]) -> Result<Vec<PackageState>> {
    let envelope: PackageStateEnvelope = serde_json::from_slice(bytes)?;
    envelope.ret.check()?;
    Ok(envelope.pkg_state_list)
}

impl AppGalleryClient {
    /// `POST app-submit`: send the app for review. This publishes once
    /// approved; call it only on purpose.
    pub async fn submit(&self, app_id: &str, options: &Submit) -> Result<()> {
        options.validate()?;
        let mut path = format!("/publish/v2/app-submit?appId={app_id}");
        if let Some(t) = &options.release_time {
            path.push_str(&format!("&releaseTime={}", crate::upload::encode(t)));
        }
        if let Some(r) = &options.remark {
            path.push_str(&format!("&remark={}", crate::upload::encode(r)));
        }
        let body = match &options.phased {
            Some(phased) => {
                path.push_str("&releaseType=3");
                Some(serde_json::to_value(phased)?)
            }
            None => None,
        };
        let bytes = self
            .send(Method::POST, &path, body.as_ref())
            .await?
            .bytes()
            .await?;
        parse_ret(&bytes)
    }

    /// `PUT phased-release`: update a phased release, or with `to_full`
    /// switch it to a release on the entire network.
    pub async fn update_phased_release(
        &self,
        app_id: &str,
        phased: &PhasedRelease,
        to_full: bool,
    ) -> Result<()> {
        let (path, body) = if to_full {
            (
                format!("/publish/v2/phased-release?appId={app_id}&releaseType=1"),
                serde_json::json!({}),
            )
        } else {
            phased.validate()?;
            (
                format!("/publish/v2/phased-release?appId={app_id}&releaseType=3"),
                serde_json::to_value(phased)?,
            )
        };
        let bytes = self
            .send(Method::PUT, &path, Some(&body))
            .await?
            .bytes()
            .await?;
        parse_ret(&bytes)
    }

    /// `PUT properties/gms`: report whether the app depends on Google Mobile
    /// Services.
    pub async fn set_gms_dependency(&self, app_id: &str, needs_gms: bool) -> Result<()> {
        let body = serde_json::json!({ "needGms": u8::from(needs_gms) });
        let path = format!("/publish/v2/properties/gms?appId={app_id}");
        let bytes = self
            .send(Method::PUT, &path, Some(&body))
            .await?
            .bytes()
            .await?;
        parse_ret(&bytes)
    }

    /// `PUT on-shelf-time`: change when a version in Releasing state goes live.
    pub async fn update_release_time(
        &self,
        app_id: &str,
        change: &ReleaseTimeChange,
    ) -> Result<()> {
        let body = match change {
            ReleaseTimeChange::Immediately => {
                serde_json::json!({ "changeType": 2, "releaseTime": "", "releaseType": 1 })
            }
            ReleaseTimeChange::At(time) => {
                if !is_release_time(time) {
                    return Err(Error::Invalid {
                        field: "releaseTime",
                        detail: format!("{time:?} is not yyyy-MM-ddTHH:mm:ss+HHMM"),
                    });
                }
                serde_json::json!({ "changeType": 3, "releaseTime": time, "releaseType": 1 })
            }
        };
        let path = format!("/publish/v2/on-shelf-time?appId={app_id}");
        let bytes = self
            .send(Method::PUT, &path, Some(&body))
            .await?
            .bytes()
            .await?;
        parse_ret(&bytes)
    }

    /// `GET package-list`: packages uploaded for the app, `max` up to 100.
    pub async fn package_list(
        &self,
        app_id: &str,
        from: Option<u32>,
        max: Option<u32>,
    ) -> Result<PackageList> {
        let mut path = format!("/publish/v2/package-list?appId={app_id}");
        if let Some(from) = from {
            path.push_str(&format!("&fromRecCount={from}"));
        }
        if let Some(max) = max {
            path.push_str(&format!("&maxReqCount={}", max.min(100)));
        }
        let bytes = self.send(Method::GET, &path, None).await?.bytes().await?;
        parse_package_list(&bytes)
    }

    /// `GET package/compile/status`: whether uploaded packages parsed.
    pub async fn package_status(
        &self,
        app_id: &str,
        pkg_ids: &[&str],
    ) -> Result<Vec<PackageState>> {
        if pkg_ids.is_empty() {
            return Err(Error::Missing("pkgIds"));
        }
        let path = format!(
            "/publish/v2/package/compile/status?appId={app_id}&pkgIds={}",
            pkg_ids.join(",")
        );
        let bytes = self.send(Method::GET, &path, None).await?.bytes().await?;
        parse_package_states(&bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_times_follow_the_documented_format() {
        assert!(is_release_time("2026-11-01T09:00:00+0100"));
        assert!(is_release_time("2015-01-01T01:01:01-0500"));
        assert!(!is_release_time("2026-11-01 09:00:00+0100"));
        assert!(!is_release_time("2026-11-01T09:00:00Z"));
        assert!(!is_release_time("2026-11-01T09:00:00+01:00"));
    }

    #[test]
    fn percentages_have_at_most_two_decimals_and_stay_in_range() {
        for ok in ["0", "50", "50.5", "50.00", "100.00"] {
            assert!(is_percent(ok), "{ok}");
        }
        for bad in ["", "50%", "100.01", "-1", "1.234", ".5", "abc"] {
            assert!(!is_percent(bad), "{bad}");
        }
    }

    #[test]
    fn a_phased_submit_needs_every_window_field() {
        let mut submit = Submit {
            phased: Some(PhasedRelease {
                phased_release_start_time: Some("2026-11-01T09:00:00+0100".into()),
                phased_release_end_time: Some("2026-11-08T09:00:00+0100".into()),
                phased_release_percent: Some("10.00".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(submit.validate().is_err());
        submit.phased.as_mut().unwrap().phased_release_description = Some("10% first".into());
        assert!(submit.validate().is_ok());
    }

    #[test]
    fn remarks_are_ten_to_three_hundred_characters() {
        let short = Submit {
            remark: Some("too short".into()),
            ..Default::default()
        };
        assert!(short.validate().is_err());
        let fine = Submit {
            remark: Some("Sign in with the demo account in the notes.".into()),
            ..Default::default()
        };
        assert!(fine.validate().is_ok());
    }

    #[test]
    fn the_phased_body_uses_the_documented_names() {
        let body = serde_json::to_value(PhasedRelease {
            state: Some("SUSPEND".into()),
            phased_release_percent: Some("50.00".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            body,
            serde_json::json!({"state":"SUSPEND","phasedReleasePercent":"50.00"})
        );
    }

    #[test]
    fn package_list_and_states_parse() {
        let list = parse_package_list(
            br#"{"ret":{"code":0,"msg":"ok"},"retCount":1,"totalCount":1,
                "pkgList":[{"pkgVersion":"10004151","fileName":"siGit.apk","versionCode":2026102700,
                            "packageSize":1,"uploadTime":1,"packageType":1}]}"#,
        )
        .unwrap();
        assert_eq!(list.pkg_list[0].pkg_version, "10004151");
        assert_eq!(list.pkg_list[0].other["packageType"], 1);
        let states = parse_package_states(
            br#"{"ret":{"code":0,"msg":"ok"},"pkgStateList":[{"pkgId":"1","successStatus":1}]}"#,
        )
        .unwrap();
        assert_eq!(states[0].success_status, Some(1));
    }
}
