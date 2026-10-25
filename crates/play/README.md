# smbcloud-ascapi-play

Google Play Developer API (Edits v3) for store-listing work: localized
listings, listing images, and release notes on a track release. It is a
sibling of `smbcloud-ascapi-aso` and shares nothing with `smbcloud-ascapi-core`:
Play uses OAuth2 service-account tokens and its own error envelope.

```rust
use smbcloud_ascapi_play::{PlayClient, ServiceAccount};

let account = ServiceAccount::from_json_file("play-service-account.json")?;
let client = PlayClient::new(account);
let edit = client.insert_edit("com.example.app").await?;
let listings = client.list_listings("com.example.app", &edit.id).await?;
client.delete_edit("com.example.app", &edit.id).await?;
```

Every read and write happens inside an edit. Nothing is visible to users until
`commit_edit`; `delete_edit` throws the edit away.
