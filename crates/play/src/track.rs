use serde::{Deserialize, Serialize};

/// Release notes for one language on a track release.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalizedText {
    pub language: String,
    pub text: String,
}

/// One release on a track. Fields this crate does not edit are carried
/// through untouched so a `tracks.update` does not drop them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackRelease {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub version_codes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_fraction: Option<f64>,
    #[serde(default)]
    pub release_notes: Vec<LocalizedText>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country_targeting: Option<serde_json::Value>,
    /// Release fields this crate does not model, kept so an update sends
    /// them back unchanged.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub track: String,
    #[serde(default)]
    pub releases: Vec<TrackRelease>,
}

/// What a promotion changes on the target track.
#[derive(Debug, Clone, Serialize)]
pub struct PromotePlan {
    /// The target already holds the code at the requested status and
    /// fraction, so there is nothing to write.
    pub noop: bool,
    /// The target track as it will be after the promotion.
    pub target: Track,
}

const FRACTION_EPSILON: f64 = 1e-9;

impl Track {
    /// Plan promoting `version_code` from `source` onto this (target) track.
    ///
    /// The new release copies the source release's name, version codes and
    /// release notes. It is `inProgress` at `user_fraction` below 1.0 and
    /// `completed` at 1.0. Any `inProgress` or `draft` release on the target
    /// is replaced; `completed` releases stay, because Play needs the one
    /// serving users during a staged rollout. A target already holding the
    /// code at the same status and fraction is a no-op, a higher fraction
    /// updates it, and a lower one is an error.
    pub fn promote_from(
        &self,
        source: &Track,
        version_code: &str,
        user_fraction: f64,
    ) -> std::result::Result<PromotePlan, String> {
        if !(user_fraction > 0.0 && user_fraction <= 1.0) {
            return Err(format!("user fraction {user_fraction} is outside (0, 1]"));
        }
        let from = source
            .releases
            .iter()
            .find(|r| r.version_codes.iter().any(|c| c == version_code))
            .ok_or_else(|| {
                format!(
                    "no release on track {} carries version code {version_code}",
                    source.track
                )
            })?;
        let full = user_fraction >= 1.0;
        let release = TrackRelease {
            name: from.name.clone(),
            version_codes: from.version_codes.clone(),
            status: Some(if full { "completed" } else { "inProgress" }.to_string()),
            user_fraction: (!full).then_some(user_fraction),
            release_notes: from.release_notes.clone(),
            country_targeting: None,
            extra: Default::default(),
        };

        if let Some(held) = self
            .releases
            .iter()
            .find(|r| r.version_codes.iter().any(|c| c == version_code))
        {
            let held_full = held.status.as_deref() == Some("completed");
            let held_fraction = if held_full {
                1.0
            } else {
                held.user_fraction.unwrap_or(0.0)
            };
            let rolling = matches!(
                held.status.as_deref(),
                Some("inProgress") | Some("completed")
            );
            if rolling {
                if (held_fraction - user_fraction).abs() < FRACTION_EPSILON {
                    return Ok(PromotePlan {
                        noop: true,
                        target: self.clone(),
                    });
                }
                if user_fraction < held_fraction {
                    return Err(format!(
                        "track {} already serves version code {version_code} to {held_fraction}; a rollout cannot be lowered to {user_fraction}",
                        self.track
                    ));
                }
            }
        }

        let mut releases: Vec<TrackRelease> = self
            .releases
            .iter()
            .filter(|r| {
                !matches!(r.status.as_deref(), Some("inProgress") | Some("draft"))
                    && !r.version_codes.iter().any(|c| c == version_code)
            })
            .cloned()
            .collect();
        releases.insert(0, release);
        Ok(PromotePlan {
            noop: false,
            target: Track {
                track: self.track.clone(),
                releases,
            },
        })
    }

    /// Set `locale`'s release notes on the release carrying `version_code`,
    /// replacing any existing note for that language. Returns an error if no
    /// release carries the code, or the text is over Play's limit.
    pub fn set_release_notes(
        &mut self,
        version_code: &str,
        locale: &str,
        text: &str,
    ) -> std::result::Result<(), String> {
        let len = text.chars().count();
        if len > crate::listing::RELEASE_NOTES_MAX_LEN {
            return Err(format!(
                "release notes are {len} characters; Google Play accepts at most {}",
                crate::listing::RELEASE_NOTES_MAX_LEN
            ));
        }
        let release = self
            .releases
            .iter_mut()
            .find(|r| r.version_codes.iter().any(|c| c == version_code))
            .ok_or_else(|| {
                format!(
                    "no release on track {} carries version code {version_code}",
                    self.track
                )
            })?;
        release.release_notes.retain(|n| n.language != locale);
        release.release_notes.push(LocalizedText {
            language: locale.to_string(),
            text: text.to_string(),
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> Track {
        serde_json::from_str(
            r#"{"track":"internal","releases":[{"name":"1.2.0","versionCodes":["120"],
            "status":"draft","releaseNotes":[{"language":"en-US","text":"old"},
            {"language":"id","text":"lama"}]}]}"#,
        )
        .unwrap()
    }

    #[test]
    fn notes_replace_only_the_given_language() {
        let mut t = track();
        t.set_release_notes("120", "en-US", "new").unwrap();
        let notes = &t.releases[0].release_notes;
        assert_eq!(notes.len(), 2);
        assert!(notes.iter().any(|n| n.language == "id" && n.text == "lama"));
        assert!(notes
            .iter()
            .any(|n| n.language == "en-US" && n.text == "new"));
    }

    #[test]
    fn an_unknown_version_code_is_an_error() {
        assert!(track().set_release_notes("999", "en-US", "x").is_err());
    }

    #[test]
    fn notes_over_five_hundred_characters_are_rejected() {
        assert!(track()
            .set_release_notes("120", "en-US", &"x".repeat(501))
            .is_err());
    }

    #[test]
    fn untouched_release_fields_survive_a_round_trip() {
        let mut t = track();
        t.set_release_notes("120", "id", "baru").unwrap();
        let value = serde_json::to_value(&t).unwrap();
        assert_eq!(value["releases"][0]["status"], "draft");
        assert_eq!(value["releases"][0]["name"], "1.2.0");
    }

    fn source() -> Track {
        serde_json::from_str(
            r#"{"track":"internal","releases":[{"name":"1.3.1","versionCodes":["131"],
            "status":"completed","releaseNotes":[{"language":"en-US","text":"new"}]}]}"#,
        )
        .unwrap()
    }

    fn production(release: &str) -> Track {
        serde_json::from_str(&format!(
            r#"{{"track":"production","releases":[{release}]}}"#
        ))
        .unwrap()
    }

    const LIVE: &str = r#"{"name":"1.3.0","versionCodes":["130"],"status":"completed",
        "countryTargeting":{"includeRestOfWorld":true},"inAppUpdatePriority":3}"#;

    #[test]
    fn promoting_adds_a_staged_release_and_keeps_the_live_one() {
        let plan = production(LIVE)
            .promote_from(&source(), "131", 0.2)
            .unwrap();
        assert!(!plan.noop);
        let r = &plan.target.releases;
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].status.as_deref(), Some("inProgress"));
        assert_eq!(r[0].user_fraction, Some(0.2));
        assert_eq!(r[0].name.as_deref(), Some("1.3.1"));
        assert_eq!(r[0].release_notes[0].text, "new");
        assert_eq!(r[1].status.as_deref(), Some("completed"));
    }

    #[test]
    fn the_same_code_at_the_same_fraction_is_a_noop() {
        let held =
            r#"{"name":"1.3.1","versionCodes":["131"],"status":"inProgress","userFraction":0.2}"#;
        let plan = production(held)
            .promote_from(&source(), "131", 0.2)
            .unwrap();
        assert!(plan.noop);
    }

    #[test]
    fn a_higher_fraction_updates_the_rollout() {
        let held =
            r#"{"name":"1.3.1","versionCodes":["131"],"status":"inProgress","userFraction":0.2}"#;
        let plan = production(held)
            .promote_from(&source(), "131", 0.5)
            .unwrap();
        assert!(!plan.noop);
        assert_eq!(plan.target.releases.len(), 1);
        assert_eq!(plan.target.releases[0].user_fraction, Some(0.5));
    }

    #[test]
    fn a_lower_fraction_is_an_error() {
        let held =
            r#"{"name":"1.3.1","versionCodes":["131"],"status":"inProgress","userFraction":0.5}"#;
        assert!(production(held)
            .promote_from(&source(), "131", 0.2)
            .is_err());
    }

    #[test]
    fn a_fraction_of_one_completes_the_release() {
        let plan = production(LIVE)
            .promote_from(&source(), "131", 1.0)
            .unwrap();
        let first = &plan.target.releases[0];
        assert_eq!(first.status.as_deref(), Some("completed"));
        assert_eq!(first.user_fraction, None);
    }

    #[test]
    fn a_draft_on_the_target_is_replaced() {
        let draft = r#"{"name":"old","versionCodes":["129"],"status":"draft"}"#;
        let t = production(&format!("{LIVE},{draft}"));
        let plan = t.promote_from(&source(), "131", 0.2).unwrap();
        assert_eq!(plan.target.releases.len(), 2);
        assert!(plan
            .target
            .releases
            .iter()
            .all(|r| r.name.as_deref() != Some("old")));
    }

    #[test]
    fn an_unknown_code_or_bad_fraction_is_an_error() {
        let t = production(LIVE);
        assert!(t.promote_from(&source(), "999", 0.2).is_err());
        assert!(t.promote_from(&source(), "131", 0.0).is_err());
        assert!(t.promote_from(&source(), "131", 1.5).is_err());
    }

    #[test]
    fn unknown_fields_on_kept_releases_survive() {
        let plan = production(LIVE)
            .promote_from(&source(), "131", 0.2)
            .unwrap();
        let value = serde_json::to_value(&plan.target).unwrap();
        assert_eq!(value["releases"][1]["inAppUpdatePriority"], 3);
        assert_eq!(
            value["releases"][1]["countryTargeting"]["includeRestOfWorld"],
            true
        );
    }
}
