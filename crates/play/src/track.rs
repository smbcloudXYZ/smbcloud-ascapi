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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub track: String,
    #[serde(default)]
    pub releases: Vec<TrackRelease>,
}

impl Track {
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
}
