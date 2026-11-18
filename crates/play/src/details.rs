use serde::{Deserialize, Serialize};

/// `edits.details`: the app's contact details and default language.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Details {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_website: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_phone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_language: Option<String>,
}

impl Details {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }

    /// Names of the fields set here whose value differs in `stored`.
    pub fn diff(&self, stored: &Details) -> Vec<&'static str> {
        let mut wrong = Vec::new();
        let mut check = |name, want: &Option<String>, got: &Option<String>| {
            if want.is_some() && want != got {
                wrong.push(name);
            }
        };
        check(
            "contactWebsite",
            &self.contact_website,
            &stored.contact_website,
        );
        check("contactEmail", &self.contact_email, &stored.contact_email);
        check("contactPhone", &self.contact_phone, &stored.contact_phone);
        check(
            "defaultLanguage",
            &self.default_language,
            &stored.default_language,
        );
        wrong
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_fields_are_left_out_of_the_body() {
        let d = Details {
            contact_website: Some("https://a.app".into()),
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_value(&d).unwrap(),
            serde_json::json!({"contactWebsite": "https://a.app"})
        );
    }

    #[test]
    fn diff_names_only_requested_fields_that_differ() {
        let want = Details {
            contact_website: Some("x".into()),
            contact_email: Some("e".into()),
            ..Default::default()
        };
        let got = Details {
            contact_website: Some("x".into()),
            contact_email: Some("other".into()),
            contact_phone: Some("1".into()),
            ..Default::default()
        };
        assert_eq!(want.diff(&got), vec!["contactEmail"]);
        assert!(Details::default().is_empty());
    }
}
