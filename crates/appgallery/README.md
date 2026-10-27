# smbcloud-ascapi-appgallery

Huawei AppGallery Connect API. A sibling of `smbcloud-ascapi-play`: it shares
nothing with `smbcloud-ascapi-core` and has its own credentials and error
envelope.

Authentication is Service Account mode, the mode Huawei recommends. Create a
developer-level Service Account under Users and permissions > API key >
Connect API, with a role that may publish (Administrator, App administrator or
Operations). Its `*private.json` credential file holds `key_id`,
`private_key`, `sub_account` and `token_uri`. The JWT signed from it is itself
the token: `Authorization: Bearer <jwt>` to `connect-api.cloud.huawei.com`.

```rust
use smbcloud_ascapi_appgallery::{AppGalleryClient, ServiceAccount};

let account = ServiceAccount::from_json_file("xxxprivate.json")?;
let client = AppGalleryClient::new(account);
```

Status: authentication and the client are in. The Publishing API calls
(app info, language info) are not.
