# App Store Connect Signing

`ascapi` issues and inspects Apple signing certificates from the terminal
or as an MCP server, without a trip to the developer portal. It also
covers App Store Connect's App Metadata surface: apps, app infos, app
store versions, bundle IDs, screenshots, and their localizations.

MCP Registry name: `mcp-name: io.github.smbcloudXYZ/ascapi`

## Install

```bash
cargo install smbcloud-ascapi-cli
```

## Credentials

All three come from App Store Connect → Users and Access → Integrations →
App Store Connect API:

```bash
export ASC_API_KEY=<key id>
export ASC_ISSUER_ID=<issuer id>
# Optional. Defaults to ~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8
export ASC_PRIVATE_KEY_PATH=/path/to/AuthKey_XXXXXXXXXX.p8
```

## Sales reports

`sales-reports download` retrieves account-level App Store sales data. It is
for paid-app and in-app purchase reporting across the team; it is not the
per-customer transaction-history API. Reports are returned as a JSON array
whose keys are Apple's original TSV column names and whose values are strings.

Set the vendor number shown in App Store Connect → Payments and Financial
Reports, then request a report date and frequency:

```bash
export ASC_VENDOR_NUMBER=12345678

# Yesterday's paid-app and in-app purchase sales.
ascapi sales-reports download \
  --frequency DAILY \
  --report-date 2026-09-11

# Save a monthly report rather than printing it to stdout.
ascapi sales-reports download \
  --frequency MONTHLY \
  --report-date 2026-08 \
  --output sales-2026-08.json
```

`--report-type` and `--report-subtype` default to `SALES` and `SUMMARY`.
Pass the exact Apple values to request another supported sales-report layout;
use `--version` where that layout requires a report format version.

## Signing certificates

```bash
ascapi certificates list --type distribution
ascapi certificates create --type mac-installer-distribution --out-dir ~/certs
```

`create` generates an RSA 2048 key pair locally, sends Apple only a
signing request, and writes both halves to `--out-dir`. Two things follow
from that and are worth knowing before you run it:

- **Apple never has your private key.** A certificate whose key file is
  lost is permanently unusable, and no re-download recovers it. Back up
  the output directory somewhere encrypted.
- **An expired certificate cannot be renewed.** There is no such
  operation; you issue a new one, and every provisioning profile that
  embedded the old certificate has to be regenerated.

The private key is written before the request is sent, so a failure
mid-flight leaves an unused key rather than a certificate whose key was
never saved.

## MCP server

```bash
ascapi --mcp
```

Speaks MCP over stdio and exposes 28 tools: every operation the command
line has except `certificates revoke`. That covers apps, bundle IDs, app
store versions, both kinds of localization, screenshot sets, screenshot
upload, and certificates.

Twelve are read-only. Five delete something and are annotated
`destructiveHint`, so a client can gate them: version, localization,
screenshot set, and screenshot deletes, all of which can be recreated by
re-running the tool that made them.

Credentials resolve per call, so the server starts and answers
`tools/list` even when unconfigured, then fails with a message naming what
is missing.

Two deliberate absences:

- **No revocation tool.** Revoking a signing certificate invalidates every
  provisioning profile embedding it, for every teammate and every CI job,
  at once and irreversibly. That is the one delete no confirmation string a
  model types on your behalf makes safe, so `ascapi certificates revoke`
  stays on the command line, where a human is the one typing. A test fails
  the build if it ever appears in the tool list.
- **No key material in tool results.** `certificate_create` returns the
  path it wrote the key to, never the key itself, because tool results are
  read by a model and end up in transcripts.

## Copyright

© 2026 [Splitfire AB](https://5mb.app) ([smbCloud](https://smbcloud.xyz)).
