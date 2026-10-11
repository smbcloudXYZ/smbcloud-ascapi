# smbcloud-ascapi-aso

App Store Connect's App Metadata resources: apps, app infos and their
localizations, app store versions and their localizations, builds, beta
groups and review details, review submissions, bundle IDs, and screenshots.

Every call is an extension trait on `smbcloud_ascapi_core::Client`. Import
the prelude to get all of them:

```toml
[dependencies]
smbcloud-ascapi-core = "0.1"
smbcloud-ascapi-aso = "0.1"
```

```rust
use smbcloud_ascapi_core::{ApiKey, Client};
use smbcloud_ascapi_aso::prelude::*;

let api_key = ApiKey::from_p8_file("KEY_ID", "ISSUER_ID", "AuthKey.p8")?;
let client = Client::new(api_key);
let apps = client.list_apps(Some("com.example.app")).await?;
```

See the [API docs](https://docs.rs/smbcloud-ascapi-aso).

## Copyright

© 2026 PT Sigit Mitra Bangun ([smbCloud](https://smbcloud.xyz)).
