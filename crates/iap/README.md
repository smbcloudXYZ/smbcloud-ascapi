# smbcloud-ascapi-iap

App Store Connect in-app purchases (the v2 `inAppPurchases` resource): create a
product, add its per-locale display name and description, set its price and the
territories it is sold in, and upload the screenshot App Review asks for.

Extension traits on `smbcloud_ascapi_core::Client`; import `prelude::*`.
Subscriptions (`subscriptionGroups`) are not covered.
