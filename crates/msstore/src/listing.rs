use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::error::{Error, Result};

/// Maximum `title` length, in characters.
pub const TITLE_MAX_LEN: usize = 256;
/// Maximum `description` length, in characters.
pub const DESCRIPTION_MAX_LEN: usize = 10_000;
/// Maximum `shortDescription` length, in characters.
pub const SHORT_DESCRIPTION_MAX_LEN: usize = 1_000;
/// Maximum `releaseNotes` length, in characters.
pub const RELEASE_NOTES_MAX_LEN: usize = 1_500;
/// Maximum number of `features`.
pub const FEATURES_MAX_COUNT: usize = 20;
/// Maximum length of one feature, in characters.
pub const FEATURE_MAX_LEN: usize = 200;
/// Maximum number of `keywords` (search terms).
pub const KEYWORDS_MAX_COUNT: usize = 7;
/// Maximum length of one search term, in characters.
pub const KEYWORD_MAX_LEN: usize = 30;
/// Maximum number of words across all search terms.
pub const KEYWORDS_MAX_WORDS: usize = 21;

/// The listing keys this crate edits. Unset fields are left alone.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ListingFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub short_description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub features: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keywords: Option<Vec<String>>,
}

fn chars(s: &str) -> usize {
    s.chars().count()
}

impl ListingFields {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Check the fields against the Store's limits. `Err` names the first
    /// violation; `Ok` carries warnings (non-ASCII punctuation) to show.
    pub fn validate(&self) -> std::result::Result<Vec<String>, String> {
        let texts = [
            ("title", &self.title, TITLE_MAX_LEN),
            (
                "shortDescription",
                &self.short_description,
                SHORT_DESCRIPTION_MAX_LEN,
            ),
            ("description", &self.description, DESCRIPTION_MAX_LEN),
            ("releaseNotes", &self.release_notes, RELEASE_NOTES_MAX_LEN),
        ];
        for (name, value, max) in texts {
            let len = value.as_deref().map_or(0, chars);
            if len > max {
                return Err(format!(
                    "{name} is {len} characters; the Microsoft Store accepts at most {max}"
                ));
            }
        }
        if let Some(features) = &self.features {
            if features.len() > FEATURES_MAX_COUNT {
                return Err(format!(
                    "features has {} entries; the Microsoft Store accepts at most {FEATURES_MAX_COUNT}",
                    features.len()
                ));
            }
            if let Some((i, f)) = features
                .iter()
                .enumerate()
                .find(|(_, f)| chars(f) > FEATURE_MAX_LEN)
            {
                return Err(format!(
                    "features[{i}] is {} characters; at most {FEATURE_MAX_LEN}",
                    chars(f)
                ));
            }
        }
        if let Some(keywords) = &self.keywords {
            if keywords.len() > KEYWORDS_MAX_COUNT {
                return Err(format!(
                    "keywords has {} entries; the Microsoft Store accepts at most {KEYWORDS_MAX_COUNT}",
                    keywords.len()
                ));
            }
            if let Some((i, k)) = keywords
                .iter()
                .enumerate()
                .find(|(_, k)| chars(k) > KEYWORD_MAX_LEN)
            {
                return Err(format!(
                    "keywords[{i}] is {} characters; at most {KEYWORD_MAX_LEN}",
                    chars(k)
                ));
            }
            let words: usize = keywords.iter().map(|k| k.split_whitespace().count()).sum();
            if words > KEYWORDS_MAX_WORDS {
                return Err(format!(
                    "keywords hold {words} words in total; at most {KEYWORDS_MAX_WORDS}"
                ));
            }
        }
        Ok(self.ascii_warnings())
    }

    /// Em dashes, en dashes and curly quotes, which the repo's listing note
    /// asks to keep out of store copy.
    fn ascii_warnings(&self) -> Vec<String> {
        let bad = |c: char| {
            matches!(
                c,
                '\u{2014}' | '\u{2013}' | '\u{2018}' | '\u{2019}' | '\u{201C}' | '\u{201D}'
            )
        };
        let mut out = Vec::new();
        let mut check = |name: String, text: &str| {
            if let Some(c) = text.chars().find(|c| bad(*c)) {
                out.push(format!(
                    "{name} contains {c:?}; use plain ASCII punctuation"
                ));
            }
        };
        for (name, value) in [
            ("title", &self.title),
            ("shortDescription", &self.short_description),
            ("description", &self.description),
            ("releaseNotes", &self.release_notes),
        ] {
            if let Some(v) = value {
                check(name.to_string(), v);
            }
        }
        for (name, list) in [("features", &self.features), ("keywords", &self.keywords)] {
            for (i, v) in list.iter().flatten().enumerate() {
                check(format!("{name}[{i}]"), v);
            }
        }
        out
    }

    /// Read these fields from a submission's `baseListing` for `locale`.
    /// Fields the listing lacks come back `None`.
    pub fn read(submission: &Value, locale: &str) -> Self {
        let Some(base) = find_locale_key(submission, locale)
            .and_then(|key| submission.pointer(&format!("/listings/{}/baseListing", escape(&key))))
        else {
            return Self::default();
        };
        let text = |k: &str| base.get(k).and_then(Value::as_str).map(str::to_string);
        let list = |k: &str| {
            base.get(k).and_then(Value::as_array).map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
        };
        Self {
            title: text("title"),
            short_description: text("shortDescription"),
            description: text("description"),
            release_notes: text("releaseNotes"),
            features: list("features"),
            keywords: list("keywords"),
        }
    }

    /// Replace only the set keys of `baseListing` for `locale`, creating the
    /// locale when the submission lacks it. Every other key is untouched.
    pub fn apply(&self, submission: &mut Value, locale: &str) -> Result<()> {
        let listings = submission
            .as_object_mut()
            .ok_or_else(|| Error::Shape("submission is not an object".into()))?
            .entry("listings")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or_else(|| Error::Shape("listings is not an object".into()))?;
        let key = listings
            .keys()
            .find(|k| k.eq_ignore_ascii_case(locale))
            .cloned()
            .unwrap_or_else(|| locale.to_ascii_lowercase());
        let entry = listings
            .entry(key)
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or_else(|| Error::Shape("listing is not an object".into()))?;
        let base = entry
            .entry("baseListing")
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| Error::Shape("baseListing is not an object".into()))?;
        let mut set = |k: &str, v: Value| {
            base.insert(k.to_string(), v);
        };
        if let Some(v) = &self.title {
            set("title", json!(v));
        }
        if let Some(v) = &self.short_description {
            set("shortDescription", json!(v));
        }
        if let Some(v) = &self.description {
            set("description", json!(v));
        }
        if let Some(v) = &self.release_notes {
            set("releaseNotes", json!(v));
        }
        if let Some(v) = &self.features {
            set("features", json!(v));
        }
        if let Some(v) = &self.keywords {
            set("keywords", json!(v));
        }
        Ok(())
    }

    /// Names of the fields set here whose stored value is not an exact match.
    pub fn diff(&self, stored: &Self) -> Vec<&'static str> {
        fn differs<T: PartialEq>(sent: &Option<T>, stored: &Option<T>) -> bool {
            sent.as_ref().is_some_and(|s| stored.as_ref() != Some(s))
        }
        [
            ("title", differs(&self.title, &stored.title)),
            (
                "shortDescription",
                differs(&self.short_description, &stored.short_description),
            ),
            (
                "description",
                differs(&self.description, &stored.description),
            ),
            (
                "releaseNotes",
                differs(&self.release_notes, &stored.release_notes),
            ),
            ("features", differs(&self.features, &stored.features)),
            ("keywords", differs(&self.keywords, &stored.keywords)),
        ]
        .into_iter()
        .filter(|(_, d)| *d)
        .map(|(n, _)| n)
        .collect()
    }
}

/// The key in `listings` matching `locale`, ignoring case (`en-us`/`en-US`).
pub fn find_locale_key(submission: &Value, locale: &str) -> Option<String> {
    submission
        .get("listings")?
        .as_object()?
        .keys()
        .find(|k| k.eq_ignore_ascii_case(locale))
        .cloned()
}

/// Escape a JSON-pointer segment.
fn escape(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(len: usize) -> Option<String> {
        Some("x".repeat(len))
    }

    #[test]
    fn each_text_field_over_its_limit_is_rejected_by_name() {
        let cases = [
            (
                ListingFields {
                    title: text(257),
                    ..Default::default()
                },
                "title",
            ),
            (
                ListingFields {
                    short_description: text(1001),
                    ..Default::default()
                },
                "shortDescription",
            ),
            (
                ListingFields {
                    description: text(10_001),
                    ..Default::default()
                },
                "description",
            ),
            (
                ListingFields {
                    release_notes: text(1501),
                    ..Default::default()
                },
                "releaseNotes",
            ),
        ];
        for (fields, name) in cases {
            let message = fields.validate().unwrap_err();
            assert!(message.starts_with(name), "{message}");
        }
    }

    #[test]
    fn fields_at_their_limits_pass_and_lengths_count_characters() {
        let fields = ListingFields {
            title: Some("é".repeat(256)),
            short_description: text(1000),
            description: text(10_000),
            release_notes: text(1500),
            features: Some(vec!["x".repeat(200); 20]),
            keywords: Some(vec!["a b c".into(); 7]),
        };
        assert!(fields.validate().unwrap().is_empty());
    }

    #[test]
    fn list_limits_are_enforced() {
        let too_many = ListingFields {
            features: Some(vec!["f".into(); 21]),
            ..Default::default()
        };
        assert!(too_many
            .validate()
            .unwrap_err()
            .starts_with("features has 21"));
        let long = ListingFields {
            features: Some(vec!["x".repeat(201)]),
            ..Default::default()
        };
        assert!(long.validate().unwrap_err().starts_with("features[0]"));
        let terms = ListingFields {
            keywords: Some(vec!["k".into(); 8]),
            ..Default::default()
        };
        assert!(terms.validate().unwrap_err().starts_with("keywords has 8"));
        let wide = ListingFields {
            keywords: Some(vec!["x".repeat(31)]),
            ..Default::default()
        };
        assert!(wide.validate().unwrap_err().starts_with("keywords[0]"));
        let words = ListingFields {
            keywords: Some(vec!["a b c d".into(); 6]),
            ..Default::default()
        };
        assert!(words.validate().unwrap_err().contains("24 words"));
    }

    #[test]
    fn dashes_and_curly_quotes_warn_without_failing() {
        let fields = ListingFields {
            title: Some("A \u{2014} B".into()),
            features: Some(vec!["it\u{2019}s".into()]),
            ..Default::default()
        };
        let warnings = fields.validate().unwrap();
        assert_eq!(warnings.len(), 2);
        assert!(warnings[0].starts_with("title"));
        assert!(warnings[1].starts_with("features[0]"));
    }

    fn submission() -> Value {
        json!({
            "id": "1",
            "applicationPackages": [{"fileName": "a.msix", "version": "1.3.1.0"}],
            "pricing": {"trialPeriod": "NoFreeTrial"},
            "listings": {
                "en-us": {
                    "baseListing": {
                        "title": "Old",
                        "description": "old desc",
                        "images": [{"fileName": "s.png"}],
                        "copyrightAndTrademarkInfo": "c"
                    },
                    "platformOverrides": {}
                },
                "id": {"baseListing": {"title": "Judul"}}
            }
        })
    }

    #[test]
    fn apply_replaces_only_the_set_keys_and_keeps_everything_else() {
        let mut sub = submission();
        let before = sub.clone();
        ListingFields {
            title: Some("New".into()),
            features: Some(vec!["a".into()]),
            ..Default::default()
        }
        .apply(&mut sub, "en-US")
        .unwrap();
        assert_eq!(sub["listings"]["en-us"]["baseListing"]["title"], "New");
        assert_eq!(
            sub["listings"]["en-us"]["baseListing"]["features"],
            json!(["a"])
        );
        // Everything not named is byte-identical.
        sub["listings"]["en-us"]["baseListing"]["title"] = json!("Old");
        sub["listings"]["en-us"]["baseListing"]
            .as_object_mut()
            .unwrap()
            .remove("features");
        assert_eq!(sub, before);
    }

    #[test]
    fn apply_creates_a_missing_locale_in_lowercase() {
        let mut sub = submission();
        ListingFields {
            title: Some("Hi".into()),
            ..Default::default()
        }
        .apply(&mut sub, "de-DE")
        .unwrap();
        assert_eq!(sub["listings"]["de-de"]["baseListing"]["title"], "Hi");
    }

    #[test]
    fn read_and_diff_round_trip() {
        let mut sub = submission();
        let sent = ListingFields {
            title: Some("New".into()),
            keywords: Some(vec!["git".into()]),
            ..Default::default()
        };
        sent.apply(&mut sub, "en-us").unwrap();
        let stored = ListingFields::read(&sub, "EN-US");
        assert!(sent.diff(&stored).is_empty());
        assert_eq!(stored.description.as_deref(), Some("old desc"));
        let other = ListingFields {
            title: Some("New ".into()),
            ..Default::default()
        };
        assert_eq!(other.diff(&stored), vec!["title"]);
    }
}
