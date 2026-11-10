use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub name: String,
    #[serde(default)]
    pub publisher_id: Option<String>,
    #[serde(default)]
    pub reporting_time_zone: Option<String>,
    #[serde(default)]
    pub currency_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct App {
    pub name: String,
    #[serde(default)]
    pub app_id: Option<String>,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub manual_app_info: Option<serde_json::Value>,
    #[serde(default)]
    pub linked_app_info: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdUnit {
    pub name: String,
    #[serde(default)]
    pub ad_unit_id: Option<String>,
    #[serde(default)]
    pub app_id: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub ad_format: Option<String>,
    #[serde(default)]
    pub ad_types: Vec<String>,
    #[serde(default)]
    pub reward_settings: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdFormat {
    Rewarded,
}

impl AdFormat {
    pub fn api_name(self) -> &'static str {
        match self {
            AdFormat::Rewarded => "REWARDED",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value.to_ascii_lowercase().as_str() {
            "rewarded" => Ok(AdFormat::Rewarded),
            other => Err(format!(
                "unsupported ad format {other:?}; only rewarded is supported"
            )),
        }
    }
}

/// `unitAmount` is an int64, which proto3 JSON writes as a string.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RewardSettings {
    pub unit_amount: String,
    pub unit_type: String,
}

/// Body of `accounts.adUnits.create` (v1beta).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CreateAdUnit {
    pub display_name: String,
    pub app_id: String,
    pub ad_format: String,
    pub reward_settings: RewardSettings,
}

impl CreateAdUnit {
    pub fn rewarded(
        app_id: &str,
        display_name: &str,
        reward_amount: u64,
        reward_item: &str,
    ) -> Self {
        Self {
            display_name: display_name.to_string(),
            app_id: app_id.to_string(),
            ad_format: AdFormat::Rewarded.api_name().to_string(),
            reward_settings: RewardSettings {
                unit_amount: reward_amount.to_string(),
                unit_type: reward_item.to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_body_serializes_in_camel_case_with_a_string_amount() {
        let body = CreateAdUnit::rewarded("ca-app-pub-1~2", "Karaoke rewarded", 50, "Semitones");
        assert_eq!(
            serde_json::to_value(&body).unwrap(),
            serde_json::json!({
                "displayName": "Karaoke rewarded",
                "appId": "ca-app-pub-1~2",
                "adFormat": "REWARDED",
                "rewardSettings": {"unitAmount": "50", "unitType": "Semitones"}
            })
        );
    }

    #[test]
    fn only_rewarded_parses() {
        assert_eq!(AdFormat::parse("Rewarded"), Ok(AdFormat::Rewarded));
        assert!(AdFormat::parse("banner").is_err());
    }
}
