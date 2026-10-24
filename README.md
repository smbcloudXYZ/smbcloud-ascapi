# smbcloud-ascapi

`smbcloud-ascapi` is the app stores coolest API.

Eight crates, split by domain and layered so neither front end can drift
from the other:

| Crate | What it is |
| --- | --- |
| `smbcloud-ascapi-core` | Shared transport: JWT auth, the HTTP client, JSON:API envelopes, error types |
| `smbcloud-ascapi-play` | Google Play (Edits v3): store listings, listing images, track release notes; service-account auth, independent of `core` |
| `smbcloud-ascapi-aso` | App Metadata: apps, app infos, versions, builds, pre-release versions, review submissions, bundle IDs, localizations, screenshots |
| `smbcloud-ascapi-pricing` | Pricing: app price schedules, and the per-territory prices joined with their price points and currencies |
| `smbcloud-ascapi-signing` | Code signing: certificates and provisioning profiles, plus local RSA key pair and CSR generation |
| `smbcloud-ascapi-reports` | Sales reports: account-level sales data, as Apple's gzip-compressed TSV |
| `smbcloud-ascapi-frontend` | Operations both surfaces share, so the CLI and the MCP server agree by construction, plus the CLI's `xcrun altool` upload |
| `smbcloud-ascapi-mcp` | The MCP contract and stdio server |
| `smbcloud-ascapi-cli` | The `asc` binary: clap command tree, plus `--mcp` |

`aso`, `play`, `pricing`, `signing` and `reports` know nothing about each other, and neither knows
anything about the front ends. Each adds its calls to
`smbcloud_ascapi_core::Client` as **extension traits**, since Rust only
allows inherent impls in the crate that defines a type:

```rust
use smbcloud_ascapi_core::{ApiKey, Client};
use smbcloud_ascapi_aso::prelude::*;      // or pricing::prelude, signing::prelude
```

MCP Registry name: `mcp-name: io.github.smbcloudXYZ/ascapi`

## Credentials

The CLI signs requests with an App Store Connect API key:

| Flag | Environment variable | Notes |
| --- | --- | --- |
| `--key-id` | `ASC_API_KEY` | The key id. There is no `ASC_KEY_ID`. |
| `--issuer-id` | `ASC_ISSUER_ID` | The issuer id from App Store Connect. |
| `--private-key-path` | `ASC_PRIVATE_KEY_PATH` | Defaults to `~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8`. |

With `--mcp`, credentials are resolved on each tool call, so the server can
list its tools before they are configured.

## Copyright

© 2026 PT Sigit Mitra Bangun ([smbCloud](https://smbcloud.xyz)).
