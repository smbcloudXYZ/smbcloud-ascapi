# smbcloud-ascapi-msstore

Microsoft Store submission API v1.0 for store-listing work: the text of a
listing (title, short description, description, release notes, features and
search terms) on an MSIX app's submission. It is a sibling of
`smbcloud-ascapi-play` and shares nothing with `smbcloud-ascapi-core`: the
Store uses Entra client-credentials tokens and its own error envelope.

```rust
use smbcloud_ascapi_msstore::{Credentials, MsStoreClient};

let credentials = Credentials::from_env_file("partner-center.env")?;
let product = credentials.product_id.clone();
let client = MsStoreClient::new(credentials);
let app = client.get_app(&product).await?;
```

Submissions are kept as `serde_json::Value`. Only the listing keys are
replaced, so packages, pricing and everything else round-trip untouched.
Committing a submission to certification is deliberately not implemented.
