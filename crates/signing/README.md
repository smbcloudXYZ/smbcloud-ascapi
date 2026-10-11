# smbcloud-ascapi-signing

Apple code signing through App Store Connect: certificates and provisioning
profiles, plus local RSA key pair and CSR generation.

Apple never receives or returns a private key. You generate the key pair
locally, send Apple only a signing request, and keep the private key. A
certificate whose key file is lost is permanently unusable, and an expired
certificate cannot be renewed, only replaced.

```toml
[dependencies]
smbcloud-ascapi-core = "0.1"
smbcloud-ascapi-signing = "0.1"
```

```rust
use smbcloud_ascapi_core::{ApiKey, Client};
use smbcloud_ascapi_signing::prelude::*;

let client = Client::new(ApiKey::from_p8_file("KEY_ID", "ISSUER_ID", "AuthKey.p8")?);
```

See the [API docs](https://docs.rs/smbcloud-ascapi-signing) for `csr`,
`certificate` and `profile`.

## Copyright

© 2026 PT Sigit Mitra Bangun ([smbCloud](https://smbcloud.xyz)).
