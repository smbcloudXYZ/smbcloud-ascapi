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

/// The `batchUpdateStates` request that activates or deactivates one purchase
/// option.
pub fn purchase_option_state_body(
    package: &str,
    product_id: &str,
    purchase_option_id: &str,
    activate: bool,
) -> Value {
    let key = if activate {
        "activatePurchaseOptionRequest"
    } else {
        "deactivatePurchaseOptionRequest"
    };
    json!({ "requests": [{
        key: {
            "packageName": package,
            "productId": product_id,
            "purchaseOptionId": purchase_option_id,
        }
    }]})
}

fn options(product: &Value) -> &[Value] {
    product
        .get("purchaseOptions")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// The purchase option to act on: the one asked for, or the product's only
/// one. A product with several options needs `--purchase-option`.
pub fn pick_purchase_option(product: &Value, requested: Option<&str>) -> Result<String> {
    let ids: Vec<&str> = options(product)
        .iter()
        .filter_map(|o| o.get("purchaseOptionId").and_then(Value::as_str))
        .collect();
    match (requested, ids.as_slice()) {
        (Some(id), _) if ids.contains(&id) => Ok(id.to_string()),
        (Some(id), _) => Err(Error::Invalid(format!(
            "the product has no purchase option {id:?} (it has: {})",
            if ids.is_empty() {
                "none".to_string()
            } else {
                ids.join(", ")
            }
        ))),
        (None, [only]) => Ok((*only).to_string()),
        (None, []) => Err(Error::Invalid(
            "the product has no purchase option".to_string(),
        )),
        (None, many) => Err(Error::Invalid(format!(
            "the product has {} purchase options ({}); pass --purchase-option",
            many.len(),
            many.join(", ")
        ))),
    }
}

/// The `state` Play reports for a purchase option (`ACTIVE`, `INACTIVE`, ...).
pub fn purchase_option_state<'a>(product: &'a Value, purchase_option_id: &str) -> Option<&'a str> {
    options(product)
        .iter()
        .find(|o| o.get("purchaseOptionId").and_then(Value::as_str) == Some(purchase_option_id))
        .and_then(|o| o.get("state"))
        .and_then(Value::as_str)
}

/// What `products update` changes. Anything left `None` stays as it is.
#[derive(Debug, Clone, Default)]
pub struct ProductChanges {
    pub title: Option<String>,
    pub description: Option<String>,
    /// Which listing to change; optional when the product has one listing.
    pub language: Option<String>,
    pub price: Option<Money>,
    /// Which purchase option a price change re-prices; optional when the
    /// product has one option.
    pub purchase_option: Option<String>,
}

impl ProductChanges {
    pub fn touches_listings(&self) -> bool {
        self.title.is_some() || self.description.is_some()
    }

    /// The `updateMask` for this change: only the parts that are sent.
    pub fn update_mask(&self) -> &'static str {
        match (self.touches_listings(), self.price.is_some()) {
            (true, true) => "listings,purchaseOptions",
            (true, false) => "listings",
            _ => "purchaseOptions",
        }
    }
}

/// A price read back from a product, to reuse as the probe for a regions
/// version when only text changes.
pub fn existing_price(product: &Value) -> Option<Money> {
    let price = options(product)
        .iter()
        .filter_map(|o| o.get("regionalPricingAndAvailabilityConfigs")?.as_array())
        .flatten()
        .find_map(|c| c.get("price"))?;
    Some(Money {
        currency: price.get("currencyCode")?.as_str()?.to_string(),
        units: price
            .get("units")
            .and_then(Value::as_str)
            .unwrap_or("0")
            .to_string(),
        nanos: price.get("nanos").and_then(Value::as_i64).unwrap_or(0) as i32,
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

/// The patch body for `changes` applied to the product as read. Only the
/// parts named by [`ProductChanges::update_mask`] are present, and inside
/// them everything not being changed is carried through as read, because Play
/// replaces a masked list as a whole. `converted` is the
/// `pricing:convertRegionPrices` answer, needed when a price changes. Regions
/// the product does not already price are not added, and a region's
/// availability is left as it was.
pub fn update_body(
    product: &Value,
    changes: &ProductChanges,
    converted: Option<&Value>,
) -> Result<Value> {
    if !changes.touches_listings() && changes.price.is_none() {
        return Err(Error::Invalid(
            "nothing to change: pass --title, --description or --price".to_string(),
        ));
    }
    let mut body = json!({
        "packageName": product.get("packageName").cloned().unwrap_or(Value::Null),
        "productId": product.get("productId").cloned().unwrap_or(Value::Null),
    });

    if changes.touches_listings() {
        let mut listings: Vec<Value> = product
            .get("listings")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let language = match (&changes.language, listings.as_slice()) {
            (Some(l), _) => l.clone(),
            (None, [only]) => only
                .get("languageCode")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            _ => {
                return Err(Error::Invalid(
                    "pass --language to name the listing to change (a new one also needs \
                     --title and --description)"
                        .to_string(),
                ))
            }
        };
        let index = listings
            .iter()
            .position(|l| l.get("languageCode").and_then(Value::as_str) == Some(language.as_str()));
        let mut listing = match index {
            Some(i) => listings[i].clone(),
            None => {
                if changes.title.is_none() || changes.description.is_none() {
                    return Err(Error::Invalid(format!(
                        "the product has no {language} listing; pass both --title and --description to add one"
                    )));
                }
                json!({ "languageCode": language })
            }
        };
        if let Some(title) = &changes.title {
            check_len("title", title, TITLE_MAX_LEN)?;
            listing["title"] = json!(title);
        }
        if let Some(description) = &changes.description {
            check_len("description", description, DESCRIPTION_MAX_LEN)?;
            listing["description"] = json!(description);
        }
        match index {
            Some(i) => listings[i] = listing,
            None => listings.push(listing),
        }
        body["listings"] = Value::Array(listings);
    }

    if changes.price.is_some() {
        let converted = converted
            .ok_or_else(|| Error::Invalid("a price change needs converted prices".to_string()))?;
        let prices = converted
            .get("convertedRegionPrices")
            .and_then(Value::as_object)
            .filter(|m| !m.is_empty())
            .ok_or_else(|| Error::Invalid("Play returned no regional prices".to_string()))?;
        let target = pick_purchase_option(product, changes.purchase_option.as_deref())?;
        let mut all: Vec<Value> = options(product).to_vec();
        for option in all.iter_mut() {
            if option.get("purchaseOptionId").and_then(Value::as_str) != Some(target.as_str()) {
                continue;
            }
            if let Some(configs) = option
                .get_mut("regionalPricingAndAvailabilityConfigs")
                .and_then(Value::as_array_mut)
            {
                for config in configs {
                    let region = config.get("regionCode").and_then(Value::as_str);
                    if let Some(price) = region
                        .and_then(|r| prices.get(r))
                        .and_then(|e| e.get("price"))
                    {
                        config["price"] = price.clone();
                    }
                }
            }
            if let (Some(other), Some(new_regions)) = (
                converted.get("convertedOtherRegionsPrice"),
                option.get_mut("newRegionsConfig"),
            ) {
                new_regions["usdPrice"] = other.get("usdPrice").cloned().unwrap_or(Value::Null);
                new_regions["eurPrice"] = other.get("eurPrice").cloned().unwrap_or(Value::Null);
            }
        }
        // `state` is output only; the state changes have their own calls.
        for option in all.iter_mut() {
            if let Some(map) = option.as_object_mut() {
                map.remove("state");
            }
        }
        body["purchaseOptions"] = Value::Array(all);
    }
    Ok(body)
}

/// Check the product as read back carries what was sent: each sent listing's
/// text, and each sent regional price.
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
        for field in ["title", "description"] {
            if found.and_then(|l| l.get(field)) != listing.get(field) {
                return Err(format!(
                    "updated, but the read-back {field} for {} differs",
                    code.and_then(Value::as_str).unwrap_or("?")
                ));
            }
        }
    }
    for option in sent
        .get("purchaseOptions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let id = option.get("purchaseOptionId");
        let stored_option = options(stored)
            .iter()
            .find(|o| o.get("purchaseOptionId") == id);
        for config in option
            .get("regionalPricingAndAvailabilityConfigs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let region = config.get("regionCode");
            let stored_price = stored_option
                .and_then(|o| o.get("regionalPricingAndAvailabilityConfigs"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|c| c.get("regionCode") == region)
                .and_then(|c| c.get("price"));
            if stored_price != config.get("price") {
                return Err(format!(
                    "updated, but the read-back price for {} differs",
                    region.and_then(Value::as_str).unwrap_or("?")
                ));
            }
        }
    }
    Ok(())
}

/// Whether a failed product write looks like Play refusing because no build
/// declaring `com.android.vending.BILLING` has been uploaded. Google's exact
/// wording was never captured, so this matches the word "billing" in a 4xx
/// detail, case-insensitively.
pub fn is_billing_refusal(error: &Error) -> bool {
    matches!(error, Error::Api { status, detail }
        if (400..500).contains(status) && detail.to_ascii_lowercase().contains("billing"))
}

/// The line appended to a billing refusal.
pub const BILLING_HINT: &str = "Play wants a build that declares com.android.vending.BILLING \
uploaded first; upload one with `asc play bundles upload`, then retry.";

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

    fn stored_product() -> Value {
        json!({
            "packageName": "p", "productId": "app.unlock",
            "listings": [{ "languageCode": "en-GB", "title": "Unlock", "description": "Old." }],
            "purchaseOptions": [{
                "purchaseOptionId": "buy", "state": "ACTIVE",
                "regionalPricingAndAvailabilityConfigs": [
                    { "regionCode": "GB", "availability": "AVAILABLE",
                      "price": { "currencyCode": "GBP", "units": "3", "nanos": 990000000 } },
                    { "regionCode": "DE", "availability": "UNAVAILABLE",
                      "price": { "currencyCode": "EUR", "units": "4", "nanos": 0 } }
                ],
                "newRegionsConfig": { "availability": "AVAILABLE", "usdPrice": {}, "eurPrice": {} }
            }]
        })
    }

    #[test]
    fn state_requests_name_the_right_kind() {
        let on = purchase_option_state_body("p", "x", "buy", true);
        assert_eq!(
            on["requests"][0]["activatePurchaseOptionRequest"]["purchaseOptionId"],
            "buy"
        );
        let off = purchase_option_state_body("p", "x", "buy", false);
        assert!(off["requests"][0]
            .get("activatePurchaseOptionRequest")
            .is_none());
        assert_eq!(
            off["requests"][0]["deactivatePurchaseOptionRequest"]["productId"],
            "x"
        );
    }

    #[test]
    fn the_purchase_option_defaults_to_the_only_one() {
        let one = stored_product();
        assert_eq!(pick_purchase_option(&one, None).unwrap(), "buy");
        assert!(pick_purchase_option(&one, Some("other")).is_err());
        let mut two = stored_product();
        two["purchaseOptions"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "purchaseOptionId": "rent" }));
        let e = pick_purchase_option(&two, None).unwrap_err().to_string();
        assert!(e.contains("--purchase-option") && e.contains("rent"));
        assert_eq!(pick_purchase_option(&two, Some("rent")).unwrap(), "rent");
        assert_eq!(purchase_option_state(&one, "buy"), Some("ACTIVE"));
    }

    #[test]
    fn a_text_update_sends_only_listings_and_keeps_the_rest() {
        let changes = ProductChanges {
            description: Some("New.".into()),
            ..Default::default()
        };
        assert_eq!(changes.update_mask(), "listings");
        let body = update_body(&stored_product(), &changes, None).unwrap();
        assert!(body.get("purchaseOptions").is_none());
        assert_eq!(body["listings"][0]["title"], "Unlock");
        assert_eq!(body["listings"][0]["description"], "New.");
    }

    #[test]
    fn listings_need_a_language_when_ambiguous_or_new() {
        let mut p = stored_product();
        p["listings"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "languageCode": "id", "title": "a", "description": "b" }));
        let changes = ProductChanges {
            title: Some("T".into()),
            ..Default::default()
        };
        assert!(update_body(&p, &changes, None).is_err());
        let changes = ProductChanges {
            title: Some("T".into()),
            language: Some("fr".into()),
            ..Default::default()
        };
        assert!(update_body(&p, &changes, None)
            .unwrap_err()
            .to_string()
            .contains("both"));
        let changes = ProductChanges {
            title: Some("T".into()),
            description: Some("D".into()),
            language: Some("fr".into()),
            ..Default::default()
        };
        assert_eq!(
            update_body(&p, &changes, None).unwrap()["listings"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        let changes = ProductChanges {
            title: Some("x".repeat(56)),
            language: Some("id".into()),
            ..Default::default()
        };
        assert!(update_body(&p, &changes, None).is_err());
        assert!(update_body(&p, &ProductChanges::default(), None).is_err());
    }

    #[test]
    fn a_price_update_keeps_availability_and_unlisted_regions_out() {
        let converted = json!({
            "regionVersion": { "version": "2022/02" },
            "convertedRegionPrices": {
                "GB": { "price": { "currencyCode": "GBP", "units": "5", "nanos": 0 } },
                "DE": { "price": { "currencyCode": "EUR", "units": "6", "nanos": 0 } },
                "FR": { "price": { "currencyCode": "EUR", "units": "6", "nanos": 0 } }
            },
            "convertedOtherRegionsPrice": {
                "usdPrice": { "currencyCode": "USD", "units": "6", "nanos": 0 },
                "eurPrice": { "currencyCode": "EUR", "units": "6", "nanos": 0 }
            }
        });
        let changes = ProductChanges {
            price: Some(Money::parse("USD", "6").unwrap()),
            ..Default::default()
        };
        assert_eq!(changes.update_mask(), "purchaseOptions");
        let body = update_body(&stored_product(), &changes, Some(&converted)).unwrap();
        assert!(body.get("listings").is_none());
        let option = &body["purchaseOptions"][0];
        assert!(option.get("state").is_none());
        let configs = option["regionalPricingAndAvailabilityConfigs"]
            .as_array()
            .unwrap();
        assert_eq!(configs.len(), 2);
        assert_eq!(configs[0]["price"]["units"], "5");
        assert_eq!(configs[1]["availability"], "UNAVAILABLE");
        assert_eq!(option["newRegionsConfig"]["eurPrice"]["units"], "6");
        assert_eq!(existing_price(&stored_product()).unwrap().currency, "GBP");
    }

    #[test]
    fn a_price_update_names_the_option_when_the_product_has_several() {
        let converted = json!({
            "regionVersion": { "version": "2022/02" },
            "convertedRegionPrices": {
                "GB": { "price": { "currencyCode": "GBP", "units": "5", "nanos": 0 } }
            }
        });
        let mut product = stored_product();
        product["purchaseOptions"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "purchaseOptionId": "rent", "state": "ACTIVE",
                "regionalPricingAndAvailabilityConfigs": [
                    { "regionCode": "GB", "availability": "AVAILABLE",
                      "price": { "currencyCode": "GBP", "units": "2", "nanos": 0 } }
                ]
            }));
        let changes = ProductChanges {
            price: Some(Money::parse("USD", "5").unwrap()),
            ..Default::default()
        };
        assert!(update_body(&product, &changes, Some(&converted))
            .unwrap_err()
            .to_string()
            .contains("--purchase-option"));
        let changes = ProductChanges {
            price: Some(Money::parse("USD", "5").unwrap()),
            purchase_option: Some("rent".into()),
            ..Default::default()
        };
        let body = update_body(&product, &changes, Some(&converted)).unwrap();
        let repriced = body["purchaseOptions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["purchaseOptionId"] == "rent")
            .unwrap();
        assert_eq!(
            repriced["regionalPricingAndAvailabilityConfigs"][0]["price"]["units"],
            "5"
        );
    }

    #[test]
    fn read_back_differences_are_caught() {
        let changes = ProductChanges {
            title: Some("New".into()),
            ..Default::default()
        };
        let sent = update_body(&stored_product(), &changes, None).unwrap();
        assert!(check_applied(&stored_product(), &sent).is_err());
        let mut applied = stored_product();
        applied["listings"][0]["title"] = json!("New");
        assert!(check_applied(&applied, &sent).is_ok());
    }

    #[test]
    fn billing_refusals_are_recognised_in_4xx_only() {
        let api = |status, detail: &str| Error::Api {
            status,
            detail: detail.to_string(),
        };
        assert!(is_billing_refusal(&api(
            400,
            "App must have BILLING permission"
        )));
        assert!(is_billing_refusal(&api(403, "Billing is not set up")));
        assert!(!is_billing_refusal(&api(400, "invalid price")));
        assert!(!is_billing_refusal(&api(500, "billing")));
        assert!(!is_billing_refusal(&Error::Invalid("billing".into())));
    }
}
