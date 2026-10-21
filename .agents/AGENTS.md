# AGENTS.md

This file provides guidance to AI coding agents (siGit Code, Claude Code, etc.) when working with code in this repository. It is the canonical agent doc, symlinked from the repo root as both `AGENTS.md` and `CLAUDE.md`.

Guidance for AI coding agents working in this repository.

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

# PATCH /v1/builds/{id} usesNonExemptEncryption — answer export compliance on a
# build showing "Missing Compliance". Without an answer, review-submissions
# submit fails with 409 STATE_ERROR.ENTITY_STATE_INVALID. false = "None of the
# algorithms mentioned above" (HTTPS through Apple's OS only).
asc apps set-encryption <BUILD_ID> --uses-non-exempt-encryption false

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

## Git branching workflow

Branches flow in one direction: `main` ← `development` ← feature branch.

- `main` is the release branch. Never commit to it directly; it only advances
  by merging `development`.
- `development` is the integration/base branch. Never commit to it directly
  either — every change lands through a pull request.
- Do the work on a short-lived **feature branch** cut from `development`
  (`feat/…`, `fix/…`, `docs/…`), then **open a PR into `development`**. Let CI
  (fmt / clippy `-D warnings` / test / doc / lower-crate solo builds) pass
  before merge.
- When `development` is ready to release, it is merged into `main` via its own
  PR — not by pushing commits onto `main`.

So: branch off `development`, push the feature branch, open a PR → `development`.
Do not commit straight to `development` or `main`.

## Verify before committing

CI (`.github/workflows/ci.yml`, ubuntu + macOS) runs these, and warnings are
errors, so run the strict forms locally:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace --locked
cargo check -p smbcloud-ascapi-core --locked   # also -aso, -signing: each lower crate must build alone
```

Single test: `cargo test -p smbcloud-ascapi-aso build::tests::<name>`.
Intra-doc links are load-bearing (a broken one fails CI), and the CSR tests
shell out to `openssl`, whose output differs between OpenSSL 3 and macOS
LibreSSL.

## Verify before relying on a freshly built `asc`

The released/installed `asc` binary can silently lag `development`. This
hides because `asc apps upload` only shells out to `xcrun altool` and
doesn't touch newer code paths, so an upload succeeds against a stale
binary that lacks e.g. the `testflight` subcommand. Before relying on a
newly-added command, rebuild from `development`:

```sh
cargo build --release
# or, to be sure you're on the current branch's binary:
cargo install --path crates/cli --locked
```

`unrecognized subcommand` on a command that's been merged is the symptom;
the cure is a rebuild, not a workaround.

## Architecture notes

- Layering is `core` → `aso`/`pricing`/`signing`/`reports` → `frontend` →
  `mcp` / `cli`. Don't add a dependency that points up or sideways; CI checks
  the lower crates build alone.
- The MCP tool set is declared by the `ascapi_mcp_tools!` macro
  (`crates/mcp/src/lib.rs`), which takes every tool name as a literal;
  `crates/mcp/src/server.rs` supplies them. Adding a tool means touching the
  macro, the invocation, and `requests.rs`.
- stdout belongs to the MCP JSON-RPC stream: nothing under a tool call may
  `println!`. Results sent to a model never contain private keys or
  certificate/profile bodies; return the path written instead. That is
  enforced by the result types in `crates/frontend`, so keep new result types
  free of key material rather than filtering at the edge.
- Destructive or irreversible operations stay CLI-only (certificate revoke,
  review submission, upload). Recreatable deletes may be MCP tools, annotated
  destructive.
- `.agents/plans/` holds design plans from past release sessions; check them
  before reworking upload or signing-identity behaviour.

## Credentials

The CLI needs an ASC API key: `--key-id`/`ASC_API_KEY`,
`--issuer-id`/`ASC_ISSUER_ID`, and the `.p8` at
`--private-key-path`/`ASC_PRIVATE_KEY_PATH` (defaults to
`~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8`). `--mcp` resolves
credentials per tool call, so the server can list tools unconfigured.

### App Store Connect key location

`ascapi` itself defaults to `~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8`.
Some sibling repos use a different default and can drift when two copies of
the same key id are on disk:

- `onde-store/scripts/build-ios.sh` and `smbcloud-browser/scripts/build-ios.sh`
  default to `~/private_keys/AuthKey_<id>.p8`.

A stale `AuthKey_<id>.p8` with the same id surfaces as an indistinguishable
401 on ASC API calls — ASC says "valid key id, signature doesn't verify."
`ascapi apps upload` dodges the half of the drift that goes through altool
by passing `--p8-file-path` explicitly (so altool can't search for a stale
key). For every other command the resolution rule is the one above; if a
401 appears where it shouldn't, the first thing to check is which `.p8`
both sides are actually reading. If you need the scripts to use a single
key file, set `ASC_PRIVATE_KEY_PATH` explicitly in the environment
instead of relying on either default.

## Finish-the-release recipe (TestFlight)

The shortest path from a signed `.pkg` to a TestFlight group:

1. **Upload:** `asc apps upload <PKG> --wait` (always passes
   `--p8-file-path`; never call `altool` by hand — its key search can pick a
   stale `AuthKey_<id>.p8` and fail with a 401).
2. **Poll to VALID:** `asc apps builds <APP_ID> --version <BUILD_NUMBER>`
   until `processingState` is `VALID`, then read the build id off the
   response. (`asc apps await-processing` is the non-blocking alternative
   to `--wait` if you're driving this from CI.)
3. **Beta-review contact details** (one-time per app):
   `asc testflight app-review-detail update ...`.
4. **Submit:** find the Public Link group with
   `asc testflight groups list <APP_ID>`, then either
   `groups add-build --build-id <BUILD_ID> <GROUP_ID>` or
   `app-review-submissions submit <BUILD_ID>`. Both are CLI-only — adding
   to an external group / submitting to Beta App Review can trigger review,
   which an MCP tool call shouldn't do unattended.

## Review submissions (CLI)

```sh
# GET /v1/appStoreVersions/{id}/build — the build attached to a version, or
# null (with "no build attached" on stderr). Run it before submitting.
asc app-store-versions build <APP_STORE_VERSION_ID>

# PATCH the version's build relationship, then read the build back and fail if
# it isn't the one requested. Prints the build's id, version, processingState
# and usesNonExemptEncryption. --no-verify skips the read-back; --dry-run
# stays offline.
asc app-store-versions set-build <APP_STORE_VERSION_ID> --build-id <BUILD_ID> [--no-verify]

# GET /v1/reviewSubmissions?filter[app]={id}
asc review-submissions list <APP_ID> [--platform <...>] [--state READY_FOR_REVIEW]

# Reuse the platform's open (READY_FOR_REVIEW) submission or create one, add
# the version as an item, then PATCH submitted: true. --dry-run reads and
# prints the plan. Refuses a version that is already WAITING_FOR_REVIEW,
# IN_REVIEW or live, before any write.
asc review-submissions submit <APP_ID> --version-id <APP_STORE_VERSION_ID>

# PATCH canceled: true, for a submission App Review hasn't picked up.
asc review-submissions cancel <SUBMISSION_ID>
```

The API is `ReviewSubmissionsApi` in `crates/aso/src/review_submission.rs`;
the plan/submit orchestration is `crates/frontend/src/review.rs`. Submitting is
CLI-only on purpose: it sends a build to App Review, which an MCP tool call
shouldn't do on a model's say-so.
