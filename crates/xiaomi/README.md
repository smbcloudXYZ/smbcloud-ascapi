# smbcloud-ascapi-xiaomi

The Xiaomi GetApps developer interface (`/third-api/…` on
global.developer.mi.com): HMAC-SHA1 request signing, the order query, and the
APK upload address. It shares nothing with `smbcloud-ascapi-core`.

```rust
use smbcloud_ascapi_xiaomi::{Credentials, XiaomiClient};

let client = XiaomiClient::new(Credentials::from_env_file("xiaomi.env")?);
let request = client.upload_url_request()?;
let data = client.send(request).await?;
```

`XiaomiClient::prepare` signs a request without sending it, and
`Prepared::describe` shows it with the signature withheld. The signer lives in
`sign.rs` and is tested against vectors computed independently.
