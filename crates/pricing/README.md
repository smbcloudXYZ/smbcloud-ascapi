# smbcloud-ascapi-pricing

App Store Connect pricing: an app's price schedule, and the per-territory
prices and price points hanging off it.

Reading a price takes a join across three resources, because an `appPrices`
row carries no money of its own. This crate does the join, so you get one
flat `TerritoryPrice` per territory with the amount and currency attached.

```toml
[dependencies]
smbcloud-ascapi-core = "0.1"
smbcloud-ascapi-pricing = "0.1"
```

```rust
use smbcloud_ascapi_core::{ApiKey, Client};
use smbcloud_ascapi_pricing::app_price::PriceKind;
use smbcloud_ascapi_pricing::prelude::*;

let client = Client::new(ApiKey::from_p8_file("KEY_ID", "ISSUER_ID", "AuthKey.p8")?);
let prices = client
    .list_app_prices("APP_ID", PriceKind::Automatic, Some("IDN"))
    .await?;
```

See the [API docs](https://docs.rs/smbcloud-ascapi-pricing).

## Copyright

© 2026 PT Sigit Mitra Bangun ([smbCloud](https://smbcloud.xyz)).
