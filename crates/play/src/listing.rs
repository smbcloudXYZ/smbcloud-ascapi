use serde::{Deserialize, Serialize};

/// Maximum `title` length Google Play accepts, in characters.
pub const TITLE_MAX_LEN: usize = 30;
/// Maximum `shortDescription` length, in characters.
pub const SHORT_DESCRIPTION_MAX_LEN: usize = 80;
/// Maximum `fullDescription` length, in characters.
pub const FULL_DESCRIPTION_MAX_LEN: usize = 4000;
/// Maximum length of one release note, per language, in characters.
pub const RELEASE_NOTES_MAX_LEN: usize = 500;

/// One localized store listing, as Play returns it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub short_description: Option<String>,
    #[serde(default)]
    pub full_description: Option<String>,
    #[serde(default)]
    pub video: Option<String>,
}

/// The fields of a listing patch. Unset fields are left off the body, so Play
/// keeps their current value.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub short_description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_description: Option<String>,
}

impl ListingFields {
    pub fn is_empty(&self) -> bool {
        self.title.is_none() && self.short_description.is_none() && self.full_description.is_none()
    }

    /// Check the fields against Play's length limits before anything is sent.
    /// Returns `Err` naming the first field over its limit.
    pub fn validate(&self) -> std::result::Result<(), String> {
        let limits = [
            ("title", &self.title, TITLE_MAX_LEN),
            (
                "shortDescription",
                &self.short_description,
                SHORT_DESCRIPTION_MAX_LEN,
            ),
            (
                "fullDescription",
                &self.full_description,
                FULL_DESCRIPTION_MAX_LEN,
            ),
        ];
        for (name, value, max) in limits {
            let len = value.as_deref().map_or(0, |v| v.chars().count());
            if len > max {
                return Err(format!(
                    "{name} is {len} characters; Google Play accepts at most {max}"
                ));
            }
        }
        Ok(())
    }

    /// Names of the fields sent whose stored value is not an exact match.
    pub fn diff(&self, stored: &Listing) -> Vec<&'static str> {
        [
            ("title", &self.title, &stored.title),
            (
                "shortDescription",
                &self.short_description,
                &stored.short_description,
            ),
            (
                "fullDescription",
                &self.full_description,
                &stored.full_description,
            ),
        ]
        .into_iter()
        .filter(|(_, sent, stored)| match sent {
            Some(sent) => stored.as_deref() != Some(sent.as_str()),
            None => false,
        })
        .map(|(name, _, _)| name)
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_field_over_its_limit_is_rejected_by_name() {
        let cases = [
            (
                ListingFields {
                    title: Some("x".repeat(31)),
                    ..Default::default()
                },
                "title",
            ),
            (
                ListingFields {
                    short_description: Some("x".repeat(81)),
                    ..Default::default()
                },
                "shortDescription",
            ),
            (
                ListingFields {
                    full_description: Some("x".repeat(4001)),
                    ..Default::default()
                },
                "fullDescription",
            ),
        ];
        for (fields, name) in cases {
            let message = fields.validate().unwrap_err();
            assert!(message.starts_with(name), "{message}");
        }
    }

    #[test]
    fn fields_at_their_limit_pass_and_lengths_count_characters() {
        let fields = ListingFields {
            title: Some("é".repeat(30)),
            short_description: Some("x".repeat(80)),
            full_description: Some("x".repeat(4000)),
        };
        assert!(fields.validate().is_ok());
    }

    #[test]
    fn unset_fields_are_left_off_the_body() {
        let fields = ListingFields {
            title: Some("T".into()),
            ..Default::default()
        };
        assert_eq!(serde_json::to_string(&fields).unwrap(), r#"{"title":"T"}"#);
    }

    #[test]
    fn diff_names_only_the_fields_that_were_sent_and_differ() {
        let sent = ListingFields {
            title: Some("A".into()),
            short_description: Some("S".into()),
            full_description: None,
        };
        let stored = Listing {
            title: Some("A".into()),
            short_description: Some("S ".into()),
            full_description: Some("other".into()),
            ..Default::default()
        };
        assert_eq!(sent.diff(&stored), vec!["shortDescription"]);
    }
}
