//! One-time products ("in-app products") through the Play monetization API.
//!
//! These calls are not inside an edit: a product is changed directly and takes
//! effect when its purchase option is activated. Prices are set per region, so
//! a price in one currency is converted to every region first
//! (`pricing:convertRegionPrices`), the way Play Console does it.

use serde_json::{json, Value};

use crate::error::{Error, Result};

/// A price split the way the Play API wants it: whole units as a string, and
/// nanos (billionths) as an integer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Money {
    pub currency: String,
    pub units: String,
    pub nanos: i32,
}

impl Money {
    /// Parse `4.99` (up to 9 decimals) in `currency`, for example `USD`.
    pub fn parse(currency: &str, amount: &str) -> Result<Self> {
        let invalid = || Error::Invalid(format!("not a price: {amount:?} (expected e.g. 4.99)"));
        let currency = currency.trim().to_ascii_uppercase();
        if currency.len() != 3 || !currency.chars().all(|c| c.is_ascii_alphabetic()) {
            return Err(Error::Invalid(format!(
                "not a currency code: {currency:?} (expected e.g. USD)"
            )));
        }
        let (whole, frac) = amount.trim().split_once('.').unwrap_or((amount.trim(), ""));
        if whole.is_empty()
            || !whole.chars().all(|c| c.is_ascii_digit())
            || frac.len() > 9
            || !frac.chars().all(|c| c.is_ascii_digit())
        {
            return Err(invalid());
        }
        let nanos: i32 = format!("{frac:0<9}").parse().map_err(|_| invalid())?;
        let units = whole.trim_start_matches('0');
        let units = if units.is_empty() { "0" } else { units };
        if units == "0" && nanos == 0 {
            return Err(Error::Invalid("the price must be above zero".to_string()));
        }
        Ok(Self {
            currency,
            units: units.to_string(),
            nanos,
        })
    }

    pub fn to_json(&self) -> Value {
        json!({ "currencyCode": self.currency, "units": self.units, "nanos": self.nanos })
    }
}

/// What a new one-time product says about itself.
#[derive(Debug, Clone)]
pub struct NewProduct {
    pub package: String,
    pub product_id: String,
    pub language: String,
    pub title: String,
    pub description: String,
    pub purchase_option_id: String,
}

pub const TITLE_MAX_LEN: usize = 55;
pub const DESCRIPTION_MAX_LEN: usize = 200;

impl NewProduct {
    pub fn check(&self) -> Result<()> {
        let id = &self.product_id;
        let id_ok = !id.is_empty()
            && id.len() <= 100
            && id
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            && id.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-')
            });
        if !id_ok {
            return Err(Error::Invalid(format!(
                "product id {id:?}: use lowercase letters, digits, _ . - (starting with a letter or digit)"
            )));
        }
        for (name, value, max) in [
            ("title", &self.title, TITLE_MAX_LEN),
            ("description", &self.description, DESCRIPTION_MAX_LEN),
        ] {
            let len = value.chars().count();
            if len == 0 || len > max {
                return Err(Error::Invalid(format!(
                    "{name} must be 1 to {max} characters (got {len})"
                )));
            }
        }
        Ok(())
    }
}

/// The `OneTimeProduct` body: one listing and one purchase option, priced in
/// every region from a `pricing:convertRegionPrices` answer.
pub fn product_body(product: &NewProduct, converted: &Value) -> Result<Value> {
    let regions = converted
        .get("convertedRegionPrices")
        .and_then(Value::as_object)
        .filter(|m| !m.is_empty())
        .ok_or_else(|| Error::Invalid("Play returned no regional prices".to_string()))?;
    let mut configs: Vec<Value> = regions
        .iter()
        .filter_map(|(code, entry)| {
            let price = entry.get("price")?;
            Some(json!({ "regionCode": code, "price": price, "availability": "AVAILABLE" }))
        })
        .collect();
    configs.sort_by(|a, b| a["regionCode"].as_str().cmp(&b["regionCode"].as_str()));

    let mut option = json!({
        "purchaseOptionId": product.purchase_option_id,
        "buyOption": { "legacyCompatible": true, "multiQuantityEnabled": false },
        "regionalPricingAndAvailabilityConfigs": configs,
    });
    if let Some(other) = converted.get("convertedOtherRegionsPrice") {
        option["newRegionsConfig"] = json!({
            "usdPrice": other.get("usdPrice").cloned().unwrap_or(Value::Null),
            "eurPrice": other.get("eurPrice").cloned().unwrap_or(Value::Null),
            "availability": "AVAILABLE",
        });
    }
    Ok(json!({
        "packageName": product.package,
        "productId": product.product_id,
        "listings": [{
            "languageCode": product.language,
            "title": product.title,
            "description": product.description,
        }],
        "purchaseOptions": [option],
    }))
}

/// The regions version Play priced the conversion against, which the product
/// write must quote back.
pub fn regions_version(converted: &Value) -> Result<String> {
    converted
        .pointer("/regionVersion/version")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| Error::Invalid("Play returned no regions version".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn product() -> NewProduct {
        NewProduct {
            package: "com.example.app".into(),
            product_id: "app.unlock".into(),
            language: "en-GB".into(),
            title: "Unlock".into(),
            description: "Removes the limit.".into(),
            purchase_option_id: "buy".into(),
        }
    }

    #[test]
    fn prices_split_into_units_and_nanos() {
        let m = Money::parse("usd", "4.99").unwrap();
        assert_eq!(
            (m.currency.as_str(), m.units.as_str(), m.nanos),
            ("USD", "4", 990_000_000)
        );
        let m = Money::parse("USD", "5").unwrap();
        assert_eq!((m.units.as_str(), m.nanos), ("5", 0));
        let m = Money::parse("USD", "0.5").unwrap();
        assert_eq!((m.units.as_str(), m.nanos), ("0", 500_000_000));
    }

    #[test]
    fn bad_prices_are_refused() {
        for bad in ["", "abc", "-1", "4.9.9", "0", "0.0", "1.1234567891", ".5"] {
            assert!(Money::parse("USD", bad).is_err(), "{bad:?}");
        }
        assert!(Money::parse("US", "1").is_err());
        assert!(Money::parse("U5D", "1").is_err());
    }

    #[test]
    fn product_ids_and_text_are_checked() {
        assert!(product().check().is_ok());
        let mut p = product();
        p.product_id = "Bad Id".into();
        assert!(p.check().is_err());
        let mut p = product();
        p.title = "x".repeat(56);
        assert!(p.check().is_err());
        let mut p = product();
        p.description = String::new();
        assert!(p.check().is_err());
    }

    #[test]
    fn the_body_prices_every_region_and_keeps_new_regions_in() {
        let converted = json!({
            "regionVersion": { "version": "2022/02" },
            "convertedRegionPrices": {
                "US": { "price": { "currencyCode": "USD", "units": "4", "nanos": 990000000 } },
                "GB": { "price": { "currencyCode": "GBP", "units": "3", "nanos": 990000000 } }
            },
            "convertedOtherRegionsPrice": {
                "usdPrice": { "currencyCode": "USD", "units": "4", "nanos": 990000000 },
                "eurPrice": { "currencyCode": "EUR", "units": "4", "nanos": 990000000 }
            }
        });
        let body = product_body(&product(), &converted).unwrap();
        assert_eq!(body["productId"], "app.unlock");
        let option = &body["purchaseOptions"][0];
        assert_eq!(option["buyOption"]["legacyCompatible"], true);
        let regions = option["regionalPricingAndAvailabilityConfigs"]
            .as_array()
            .unwrap();
        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0]["regionCode"], "GB");
        assert_eq!(regions[0]["availability"], "AVAILABLE");
        assert_eq!(
            option["newRegionsConfig"]["eurPrice"]["currencyCode"],
            "EUR"
        );
        assert_eq!(regions_version(&converted).unwrap(), "2022/02");
    }

    #[test]
    fn an_empty_conversion_is_an_error() {
        assert!(product_body(&product(), &json!({ "convertedRegionPrices": {} })).is_err());
        assert!(regions_version(&json!({})).is_err());
    }
}
