# Architecture

Nine crates, split by domain and layered so neither front end can drift
from the other:

| Crate | What it is |
| --- | --- |
| `smbcloud-ascapi-core` | Shared transport: JWT auth, the HTTP client, JSON:API envelopes, error types |
| `smbcloud-ascapi-play` | Google Play (Edits v3): store listings, listing images, track release notes; service-account auth, independent of `core` |
| `smbcloud-ascapi-amazon` | Amazon Appstore App Submission API (Fire tablets, Fire TV): Login with Amazon client-credentials auth, the open edit, APK upload and replace (ETag), device targeting, validate; no commit, independent of `core` |
| `smbcloud-ascapi-appgallery` | Huawei AppGallery Connect Publishing API (Android): app info and listings, file and package upload, submit, phased release, release time, GMS flag, package list and status; Service Account auth (PS256 JWT), independent of `core` |
| `smbcloud-ascapi-msstore` | Microsoft Store submission API v1.0: listing text on an MSIX app's pending submission; Entra client-credentials auth, independent of `core` |
| `smbcloud-ascapi-aso` | App Metadata: apps, app infos, versions, builds, pre-release versions, review submissions, bundle IDs, localizations, screenshots |
| `smbcloud-ascapi-pricing` | Pricing: app price schedules, and the per-territory prices joined with their price points and currencies |
| `smbcloud-ascapi-signing` | Code signing: certificates and provisioning profiles, plus local RSA key pair and CSR generation |
| `smbcloud-ascapi-reports` | Sales reports: account-level sales data, as Apple's gzip-compressed TSV |
| `smbcloud-ascapi-frontend` | Operations both surfaces share, so the CLI and the MCP server agree by construction, plus the CLI's `xcrun altool` upload |
| `smbcloud-ascapi-mcp` | The MCP contract and stdio server |
| `smbcloud-ascapi-cli` | The `asc` binary: clap command tree, plus `--mcp` |

`aso`, `play`, `appgallery`, `amazon`, `msstore`, `pricing`, `signing` and `reports` know nothing about each other, and neither knows
anything about the front ends. Each adds its calls to
`smbcloud_ascapi_core::Client` as **extension traits**, since Rust only
allows inherent impls in the crate that defines a type:

```rust
use smbcloud_ascapi_core::{ApiKey, Client};
use smbcloud_ascapi_aso::prelude::*;      // or pricing::prelude, signing::prelude
```
