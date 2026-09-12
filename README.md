# smbcloud-ascapi

`smbcloud-ascapi` is the app stores coolest API.

Seven crates, split by domain and layered so neither front end can drift
from the other:

| Crate | What it is |
| --- | --- |
| `smbcloud-ascapi-core` | Shared transport: JWT auth, the HTTP client, JSON:API envelopes, error types |
| `smbcloud-ascapi-aso` | App Metadata: apps, app infos, versions, bundle IDs, localizations, screenshots |
| `smbcloud-ascapi-pricing` | Pricing: app price schedules, and the per-territory prices joined with their price points and currencies |
| `smbcloud-ascapi-signing` | Code signing: certificates and provisioning profiles, plus local RSA key pair and CSR generation |
| `smbcloud-ascapi-frontend` | Operations both surfaces share, so the CLI and the MCP server agree by construction |
| `smbcloud-ascapi-mcp` | The MCP contract and stdio server |
| `smbcloud-ascapi-cli` | The `ascapi` binary: clap command tree, plus `--mcp` |

`aso`, `pricing` and `signing` know nothing about each other, and neither knows
anything about the front ends. Each adds its calls to
`smbcloud_ascapi_core::Client` as **extension traits**, since Rust only
allows inherent impls in the crate that defines a type:

```rust
use smbcloud_ascapi_core::{ApiKey, Client};
use smbcloud_ascapi_aso::prelude::*;      // or pricing::prelude, signing::prelude
```

MCP Registry name: `mcp-name: io.github.smbcloudXYZ/ascapi`

## Copyright

© 2026 [Splitfire AB](https://5mb.app) ([smbCloud](https://smbcloud.xyz)).
