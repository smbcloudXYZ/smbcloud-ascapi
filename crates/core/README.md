# smbcloud-ascapi-core

Shared transport for the App Store Connect API: ES256 JWT auth, an HTTP
client that caches tokens, JSON:API envelopes, and the error type.

This is the base of the [smbcloud-ascapi](https://github.com/smbcloudXYZ/smbcloud-ascapi)
workspace. The domain crates add their calls to `Client` as extension
traits, so most users depend on one of those and get this one with it.

```toml
[dependencies]
smbcloud-ascapi-core = "0.1"
```

```rust
use smbcloud_ascapi_core::{ApiKey, Client};

let api_key = ApiKey::from_p8_file("KEY_ID", "ISSUER_ID", "AuthKey.p8")?;
let client = Client::new(api_key);
```

See the [API docs](https://docs.rs/smbcloud-ascapi-core).

## Copyright

© 2026 PT Sigit Mitra Bangun ([smbCloud](https://smbcloud.xyz)).
