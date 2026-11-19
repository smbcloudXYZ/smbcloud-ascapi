//! Auto-renewing subscriptions through the Play monetization API
//! (`monetization.subscriptions`).
//!
//! Like one-time products these are not inside an edit. A subscription is
//! created with one base plan, priced in every region from one price via
//! `pricing:convertRegionPrices`, and is not sold until that base plan is
//! activated.
//!
//! The API has no field for the subscription's icon: Play Console's "Icon"
//! upload has no API counterpart (checked against the v3 discovery document),
//! so icons stay a Console step.

use serde_json::{json, Value};

use crate::error::{Error, Result};
use crate::product::{regions_version, Money};

pub const TITLE_MAX_LEN: usize = 55;
pub const DESCRIPTION_MAX_LEN: usize = 200;
pub const BENEFIT_MAX_LEN: usize = 40;
pub const BENEFITS_MAX: usize = 4;

/// What a new subscription says about itself.
#[derive(Debug, Clone)]
pub struct NewSubscription {
    pub package: String,
    pub product_id: String,
    pub language: String,
    pub title: String,
    pub description: Option<String>,
    pub benefits: Vec<String>,
    pub base_plan_id: String,
    /// ISO 8601, for example `P1M`.
    pub billing_period: String,
    /// ISO 8601 grace period after a failed renewal, for example `P3D`.
    /// `None` leaves Play's default.
    pub grace_period: Option<String>,
}

fn valid_id(id: &str, max: usize, allow_dot: bool) -> bool {
    !id.is_empty()
        && id.len() <= max
        && id
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && id.chars().all(|c| {
            c.is_ascii_lowercase()
                || c.is_ascii_digit()
                || c == '_'
                || c == '-'
                || (allow_dot && c == '.')
        })
}

fn check_len(name: &str, value: &str, max: usize) -> Result<()> {
    let len = value.chars().count();
    if len == 0 || len > max {
        return Err(Error::Invalid(format!(
            "{name} must be 1 to {max} characters (got {len})"
        )));
    }
    Ok(())
}

impl NewSubscription {
    pub fn check(&self) -> Result<()> {
        if !valid_id(&self.product_id, 100, true) {
            return Err(Error::Invalid(format!(
                "product id {:?}: use lowercase letters, digits, _ . - (starting with a letter or digit)",
                self.product_id
            )));
        }
        // Base plan ids allow no dots and at most 63 characters.
        if !valid_id(&self.base_plan_id, 63, false) {
            return Err(Error::Invalid(format!(
                "base plan id {:?}: use lowercase letters, digits and - (starting with a letter or digit)",
                self.base_plan_id
            )));
        }
        check_len("title", &self.title, TITLE_MAX_LEN)?;
        if let Some(description) = &self.description {
            check_len("description", description, DESCRIPTION_MAX_LEN)?;
        }
        if self.benefits.len() > BENEFITS_MAX {
            return Err(Error::Invalid(format!(
                "at most {BENEFITS_MAX} benefits (got {})",
                self.benefits.len()
            )));
        }
        for benefit in &self.benefits {
            check_len("each benefit", benefit, BENEFIT_MAX_LEN)?;
        }
        if !self.billing_period.starts_with('P') {
            return Err(Error::Invalid(format!(
                "billing period {:?} is not ISO 8601 (expected e.g. P1M)",
                self.billing_period
            )));
        }
        Ok(())
    }
}

/// The `Subscription` body for `monetization.subscriptions.create`: one
/// listing and one auto-renewing base plan, priced in every region from a
/// `pricing:convertRegionPrices` answer. The base plan is created in draft;
/// activate it to start selling.
pub fn subscription_body(subscription: &NewSubscription, converted: &Value) -> Result<Value> {
    let regions = converted
        .get("convertedRegionPrices")
        .and_then(Value::as_object)
        .filter(|m| !m.is_empty())
        .ok_or_else(|| Error::Invalid("Play returned no regional prices".to_string()))?;
    let mut configs: Vec<Value> = regions
        .iter()
        .filter_map(|(code, entry)| {
            let price = entry.get("price")?;
            Some(json!({ "regionCode": code, "price": price, "newSubscriberAvailability": true }))
        })
        .collect();
    configs.sort_by(|a, b| a["regionCode"].as_str().cmp(&b["regionCode"].as_str()));

    let mut base_plan = json!({
        "basePlanId": subscription.base_plan_id,
        "autoRenewingBasePlanType": {
            "billingPeriodDuration": subscription.billing_period,
            "legacyCompatible": true,
            "resubscribeState": "RESUBSCRIBE_STATE_ACTIVE",
            "prorationMode": "SUBSCRIPTION_PRORATION_MODE_CHARGE_ON_NEXT_BILLING_DATE",
        },
        "regionalConfigs": configs,
    });
    if let Some(grace) = &subscription.grace_period {
        base_plan["autoRenewingBasePlanType"]["gracePeriodDuration"] = json!(grace);
    }
    if let Some(other) = converted.get("convertedOtherRegionsPrice") {
        base_plan["otherRegionsConfig"] = json!({
            "usdPrice": other.get("usdPrice").cloned().unwrap_or(Value::Null),
            "eurPrice": other.get("eurPrice").cloned().unwrap_or(Value::Null),
            "newSubscriberAvailability": true,
        });
    }

    let mut listing = json!({
        "languageCode": subscription.language,
        "title": subscription.title,
    });
    if let Some(description) = &subscription.description {
        listing["description"] = json!(description);
    }
    if !subscription.benefits.is_empty() {
        listing["benefits"] = json!(subscription.benefits);
    }

    Ok(json!({
        "packageName": subscription.package,
        "productId": subscription.product_id,
        "listings": [listing],
        "basePlans": [base_plan],
    }))
}

/// The `basePlans.activate` request body.
pub fn activate_body(package: &str, product_id: &str, base_plan_id: &str) -> Value {
    json!({ "packageName": package, "productId": product_id, "basePlanId": base_plan_id })
}

/// The regions version Play priced the conversion against.
pub fn converted_regions_version(converted: &Value) -> Result<String> {
    regions_version(converted)
}

/// A compact row for `subscriptions list`: id, title, and each base plan with
/// its state and period.
pub fn summary(subscription: &Value) -> Value {
    let plans: Vec<Value> = subscription
        .get("basePlans")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|plan| {
            json!({
                "basePlanId": plan.get("basePlanId"),
                "state": plan.get("state"),
                "billingPeriod": plan.pointer("/autoRenewingBasePlanType/billingPeriodDuration"),
                "regions": plan.get("regionalConfigs").and_then(Value::as_array).map(Vec::len),
            })
        })
        .collect();
    json!({
        "productId": subscription.get("productId"),
        "title": subscription.pointer("/listings/0/title"),
        "archived": subscription.get("archived"),
        "basePlans": plans,
    })
}

/// The state Play reports for a base plan (`DRAFT`, `ACTIVE`, `INACTIVE`).
pub fn base_plan_state<'a>(subscription: &'a Value, base_plan_id: &str) -> Option<&'a str> {
    subscription
        .get("basePlans")
        .and_then(Value::as_array)?
        .iter()
        .find(|p| p.get("basePlanId").and_then(Value::as_str) == Some(base_plan_id))?
        .get("state")
        .and_then(Value::as_str)
}

/// Check the subscription as read back carries the listing that was sent.
pub fn check_applied(stored: &Value, sent: &Value) -> std::result::Result<(), String> {
    for listing in sent
        .get("listings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let code = listing.get("languageCode");
        let found = stored
            .get("listings")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|l| l.get("languageCode") == code);
        for field in ["title", "description", "benefits"] {
            if found.and_then(|l| l.get(field)) != listing.get(field) {
                return Err(format!(
                    "created, but the read-back {field} for {} differs",
                    code.and_then(Value::as_str).unwrap_or("?")
                ));
            }
        }
    }
    Ok(())
}

/// Parse a price for a new subscription.
pub fn price(currency: &str, amount: &str) -> Result<Money> {
    Money::parse(currency, amount)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> NewSubscription {
        NewSubscription {
            package: "xyz.example.app".into(),
            product_id: "business_starter_monthly".into(),
            language: "en-US".into(),
            title: "Business Starter".into(),
            description: Some("Custom-domain email for a small team.".into()),
            benefits: vec!["Up to 5 team members".into(), "Your own domain".into()],
            base_plan_id: "monthly".into(),
            billing_period: "P1M".into(),
            grace_period: Some("P3D".into()),
        }
    }

    fn converted() -> Value {
        json!({
            "convertedRegionPrices": {
                "US": { "regionCode": "US", "price": { "currencyCode": "USD", "units": "15", "nanos": 0 } },
                "SE": { "regionCode": "SE", "price": { "currencyCode": "SEK", "units": "169", "nanos": 0 } }
            },
            "convertedOtherRegionsPrice": {
                "usdPrice": { "currencyCode": "USD", "units": "15", "nanos": 0 },
                "eurPrice": { "currencyCode": "EUR", "units": "14", "nanos": 0 }
            },
            "regionVersion": { "version": "2025/03" }
        })
    }

    #[test]
    fn the_body_has_one_listing_and_one_monthly_base_plan_priced_per_region() {
        let body = subscription_body(&sample(), &converted()).unwrap();
        assert_eq!(body["productId"], "business_starter_monthly");
        assert_eq!(body["listings"][0]["title"], "Business Starter");
        assert_eq!(body["listings"][0]["benefits"][0], "Up to 5 team members");
        let plan = &body["basePlans"][0];
        assert_eq!(plan["basePlanId"], "monthly");
        assert_eq!(
            plan["autoRenewingBasePlanType"]["billingPeriodDuration"],
            "P1M"
        );
        assert_eq!(
            plan["autoRenewingBasePlanType"]["gracePeriodDuration"],
            "P3D"
        );
        let regions = plan["regionalConfigs"].as_array().unwrap();
        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0]["regionCode"], "SE");
        assert_eq!(regions[1]["price"]["units"], "15");
        assert_eq!(regions[1]["newSubscriberAvailability"], true);
        assert_eq!(
            plan["otherRegionsConfig"]["eurPrice"]["currencyCode"],
            "EUR"
        );
        assert!(plan.get("state").is_none());
    }

    #[test]
    fn checks_reject_bad_ids_long_benefits_and_too_many_benefits() {
        let mut bad = sample();
        bad.base_plan_id = "monthly.plan".into();
        assert!(bad.check().is_err());

        let mut long = sample();
        long.benefits = vec!["x".repeat(41)];
        assert!(long.check().is_err());

        let mut many = sample();
        many.benefits = vec!["a".into(); 5];
        assert!(many.check().is_err());

        assert!(sample().check().is_ok());
        assert_eq!(converted_regions_version(&converted()).unwrap(), "2025/03");
    }

    #[test]
    fn summary_and_state_read_base_plans() {
        let stored = json!({
            "productId": "p", "listings": [{ "languageCode": "en-US", "title": "P" }],
            "basePlans": [{ "basePlanId": "monthly", "state": "ACTIVE",
              "autoRenewingBasePlanType": { "billingPeriodDuration": "P1M" },
              "regionalConfigs": [{}, {}] }]
        });
        assert_eq!(base_plan_state(&stored, "monthly"), Some("ACTIVE"));
        let row = summary(&stored);
        assert_eq!(row["basePlans"][0]["regions"], 2);
        assert_eq!(row["title"], "P");
    }

    #[test]
    fn read_back_must_match_the_sent_listing() {
        let sent = subscription_body(&sample(), &converted()).unwrap();
        let mut stored = sent.clone();
        assert!(check_applied(&stored, &sent).is_ok());
        stored["listings"][0]["title"] = json!("Other");
        assert!(check_applied(&stored, &sent).is_err());
    }
}
