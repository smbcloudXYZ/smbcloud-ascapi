# CLAUDE.md

Guidance for Claude Code (and other agents) working in this repository.

## What this is

`smbcloud-ascapi` is a Rust workspace wrapping the App Store Connect API. It
ships two front ends over one shared core:

- the `asc` CLI (`crates/cli`), and
- an MCP stdio server (`crates/mcp`, reachable via `asc --mcp`).

Both go through `smbcloud-ascapi-frontend` so they can't drift. See `README.md`
for the full crate table.

## Workspace layout

Domain crates (`aso`, `pricing`, `signing`, `reports`) know nothing about each
other or the front ends. Each extends `smbcloud_ascapi_core::Client` with an
**extension trait** (Rust only allows inherent impls in the crate that owns the
type). Import a domain's `prelude::*` to pull its traits in at once.

## Conventions

- JSON:API responses deserialize through `ListDocument<T>` / the core
  envelopes; `Resource.id` is the ASC id.
- CLI `ValueEnum` wrappers (`CliPlatform`, `CliBundleIdPlatform`,
  `CliProfileType`, …) wrap the domain enums so clap owns the arg parsing;
  map them to the domain type in the match arm (e.g. `.map(Platform::from)`).
- Tests live in-module: `#[cfg(test)] mod tests { use super::*; … }` with plain
  `#[test]` fns (see `crates/aso/src/bundle_id.rs`, `crates/aso/src/build.rs`).
- Pin exact ASC filter keys in assertions — e.g.
  `filter[preReleaseVersion.version]` vs `filter[version]`; the wrong key
  silently returns the wrong rows.

## Builds API (CLI)

All are subcommands of `apps` (clap kebab-cases the command names):

```sh
# GET /v1/builds?filter[app]={id} — newest first. Check processingState
# (VALID / INVALID / PROCESSING / FAILED).
asc apps builds <APP_ID> \
  [--version <BUILD_NUMBER>] \            # filter[version], e.g. 42
  [--pre-release-version <MARKETING>] \   # filter[preReleaseVersion.version], e.g. 1.1.0
  [--processing-state <STATE>]            # filter[processingState]

# GET /v1/builds/{id} — a single build by its ASC id.
asc apps build <BUILD_ID>

# GET /v1/preReleaseVersions?filter[app]={id} — the marketing version strings
# TestFlight groups builds under.
asc apps pre-release-versions <APP_ID> [--platform <ios|mac-os|tv-os|vision-os>]

# xcrun altool --upload-package, authenticated with the same key id, issuer
# and .p8 (--p8-file-path) as every other command. --dry-run prints the argv.
asc apps upload <PATH.ipa|PATH.pkg> [--wait]
```

The underlying API lives in `crates/aso/src/build.rs`: `BuildsApi` (in the
`aso` prelude) plus `BuildFilter`, whose private `query_pairs` maps each set
field to its ASC filter key. The upload lives in
`crates/frontend/src/upload.rs`. It always passes `--p8-file-path` and never
`--auth-string`: letting altool search its key directories can pick up a
stale `AuthKey_<id>.p8` and fail with a 401, and `--auth-string` would put
the key in the process table. Upload is CLI-only, because a tool call that
blocks for minutes and publishes a build doesn't belong in MCP.

## Verify before committing

```sh
cargo fmt --all
cargo clippy --all-targets
cargo test
```

## Credentials

The CLI needs an ASC API key: `--key-id`/`ASC_API_KEY`,
`--issuer-id`/`ASC_ISSUER_ID`, and the `.p8` at
`--private-key-path`/`ASC_PRIVATE_KEY_PATH` (defaults to
`~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8`). `--mcp` resolves
credentials per tool call, so the server can list tools unconfigured.
