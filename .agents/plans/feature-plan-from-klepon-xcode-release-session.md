# smbcloud-ascapi — token-source + builds API: plan (closed)

This file tracks the remaining work from the builds-API / token-source
session on `smbcloud-ascapi` (repo: `github.com:smbcloudXYZ/smbcloud-ascapi`,
base branch `development`).

Shipped as gh stack #9 (draft PRs, bottom to top):
#6 `fix/list-pagination` → #7 `feature/token-source` →
#8 `feature/builds-api` → #10 `feature/altool-upload`.

## Verification (done)
- [x] `cargo build` — clean. (Fixed: `--platform` must use the `CliPlatform`
      ValueEnum wrapper, not `Platform` directly, then `.map(Platform::from)`
      in the match arm.)
- [x] `cargo clippy --all-targets` — clean, on every stack layer.
- [x] `cargo test` — all passing (incl. doc-tests), on every stack layer.

## Feature work
- [x] altool upload orchestration (frontend crate): `frontend::upload`,
      CLI `ascapi apps upload <pkg> [--wait]` (#10). Passes
      `--p8-file-path` so altool uses the same key as the API calls.
      CLI-only; deliberately not an MCP tool.
- [x] Unit tests for the new builds API (filter keys, list/pre-release
      deserialization) (#8).
- [x] Fixed: builds/pre-release filters were sent to
      `/v1/apps/{id}/builds` and `/v1/apps/{id}/preReleaseVersions`, which
      don't accept `filter[...]`. Now `/v1/builds` and
      `/v1/preReleaseVersions` with `filter[app]` (#8).
- [x] Unit tests for `token.rs` (`StaticToken`, `Client::new` over a
      custom `TokenSource`) (#7) — `CountingSource` mint-counting tests in
      `crates/core/src/client.rs::tests`, `StaticToken`/boxed-source tests
      in `crates/core/src/token.rs::tests`.
- [x] MCP `list_builds` exposes `BuildFilter` — see
      `crates/mcp/src/lib.rs:201` (constructs `BuildFilter` from the
      request, including `processing-state`).
- [x] Documented `list_builds` limit=200 as page size, not a cap
      (sitiai finding #6, PR #24).
- [x] `asc apps await-processing` poll loop built on `BuildsApi::list_builds`
      (sitiai finding #5, PR #26) — a non-blocking alternative to altool
      `--wait`.

## Docs
- [x] README crate table — builds API surface, altool upload.
- [x] CLAUDE.md — builds, build, pre-release-versions, upload.

## Release
- [x] Release build + offline smoke test (`--help`, `--dry-run apps upload`,
      package validation errors).
- [ ] Live smoke test against ASC (needs `ASC_ISSUER_ID`): run
      `apps builds <id> --pre-release-version …` and
      `apps pre-release-versions <id> --platform ios` to confirm the
      `/v1/builds` filter fix, then one real `apps upload`. **Still blocked
      on credentials; no owner.**
- [x] Commits with co-author trailer, signed (GitHub: verified).
- [x] `gh stack` PRs (`remote.pushDefault` is `setoelkahfi`, there is no
      `origin`).
- [x] Mark PRs ready for review and merge (`gh stack merge --yes`).

## Separate / unaddressed bug — documented in AGENTS.md
- [→] ASC API 401 on builds query. Not in this repo: Klepon has no
      `build-ios.sh`. `onde-store/scripts/build-ios.sh` and
      `smbcloud-browser/scripts/build-ios.sh` default `ASC_PRIVATE_KEY_PATH`
      to `~/private_keys/AuthKey_<id>.p8`, while ascapi defaults to
      `~/.appstoreconnect/private_keys/AuthKey_<id>.p8`. Two copies with
      the same id can drift. Decide which file is current, then either
      point the scripts at it or set `ASC_PRIVATE_KEY_PATH` once for both.
      `ascapi apps upload` avoids the drift by passing the key explicitly.
      **No code change here.** The drift is between sibling repos, not
      inside `smbcloud-ascapi`; tracked as the "App Store Connect key
      location" section in `AGENTS.md` so future sessions don't
      re-investigate.

## Cleanup
- [x] Main working tree on `development` clean — the uncommitted copy of
      #6–#8 was discarded once it landed on the branches.
- [x] Old local `feature/token` (`0eac45b`, unpushed) is superseded by #7;
      branch is gone.
- [x] Worktree at `/tmp/ascapi-stack/wt` removed (`git worktree remove`).

## Status
Closed. All plan items in scope of this repo are merged. The one remaining
item (live ASC smoke test) is credential-blocked and not actionable from
here. The cross-repo key-drift finding lives in `AGENTS.md`.
