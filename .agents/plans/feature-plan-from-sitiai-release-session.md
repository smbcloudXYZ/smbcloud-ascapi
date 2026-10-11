# smbcloud-ascapi — upload + signing-identity hardening: plan

Findings from the **Siti AI** release session (macOS App Store upload of
`Siti AI.pkg` through App Store Connect). Repo:
`github.com:smbcloudXYZ/smbcloud-ascapi`, base branch `development`, remote
`setoelkahfi` (there is no `origin`, so use `git`/`gh`, not the sigit MCP
tools).

Companion plan: [`feature-plan-from-klepon-xcode-release-session.md`](./feature-plan-from-klepon-xcode-release-session.md)
— that session shipped the builds API + token-source + altool-upload stack
(gh stack #6→#7→#8→#10). **This plan does not overlap it:** klepon added the
upload *mechanism*; this one hardens the *pre-flight and verification* around
it. Shared context between the two:

> **Progress (2025-10-11, this session):** gh stack #27 (bottom→top)
> #24 `docs/list-builds-pagination` → #25 `feat/signing-doctor` →
> #26 `feat/apps-await-processing`, rebased onto `development`, all CI-clean
> (fmt / clippy `-D warnings` / test / `RUSTDOCFLAGS=-D warnings` doc /
> lower-crate solo builds), signed+verified, auto-merge enabled. This landed
> finding **#2** (signing doctor), finding **#5** (await-processing), and
> finding **#6** (list_builds pagination doc). Still open from this plan:
> **#1** (pre-upload `altool --validate-app`), **#3** (body-aware `success`),
> **#4** (`.app`-vs-`.pkg` message). The one live item for all of them is the
> ASC smoke test, still blocked on credentials.

- Both sessions hit the same root pain: a **stale private key / signing
  identity with the right id but wrong contents**, which surfaces as an
  indistinguishable 401 (klepon plan, "Separate / unaddressed bug") or, in
  this session, ~20 min lost to a stale `SIGNING_IDENTITY` in
  `.env.appstore`. `ascapi apps upload` already dodges the *key* half by
  passing `--p8-file-path` (klepon #10); the *local signing identity* half
  (finding #2 below) is still uncovered.
- Klepon's open "Live smoke test against ASC" item is the natural place to
  also exercise findings #1 and #3 once implemented.

## Session context

- Uploaded a macOS build (`Siti AI.pkg`) to ASC successfully. The time sink
  was a stale `SIGNING_IDENTITY` in `.env.appstore` — nothing in ascapi
  inspects the local keychain, so the tool gave no signal it was stale.
- `ascapi` *does* own upload + TestFlight/review orchestration (`asc apps
  upload`, `xcrun altool`). It cannot build or sign — out of scope by
  design. The findings below stay inside what ascapi already owns.
- All six findings re-verified against `development` on 2025-10-11; none of
  the merged PRs (resubmit-rejected, exact bundle-id filters, profile
  bundle-ids) touch the upload or signing-identity paths.

## Findings

### 1. No pre-upload `altool --validate-app` pass  — HIGH
`frontend::upload::validate_package` (`crates/frontend/src/upload.rs:71`) is
a *file* check only: extension is `.ipa`/`.pkg`, and the path
`is_file()`. It never asks Apple whether the binary is acceptable, so the
first real validation happens minutes into `--upload-package`. altool
exposes `--validate-app` for exactly this: a fast server-side pre-flight
(entitlements, provisioning, version collisions) before the slow upload.
- **Fix:** add `asc apps validate <pkg>` (and/or a `--validate` flag on
  `upload`) that runs `xcrun altool --validate-app` with the same
  `--p8-file-path` auth parity as the upload path, surfacing Apple's JSON
  verdict. Reuse `altool_args`/`outcome` shape.
- **Value:** turns a ~minutes-late failure into a seconds-early one; catches
  the common "wrong version / bad entitlement" class before upload.

### 2. No local-keychain `signing doctor`  — HIGH ✅ SHIPPED (PR #25)
The `signing` crate (`certificate.rs`, `profile.rs`, `csr.rs`) talks **only
to the ASC REST API**. `security find-identity` is called nowhere in the
repo; there is no local-keychain code at all. So the exact stale-identity
problem this session hit (and the klepon 401 key-drift bug) has **zero
coverage**: ascapi can tell you what certs/profiles *Apple* has, but not
what's installed and valid locally, nor whether they line up.
- **Fix:** add `asc signing doctor` that runs `security find-identity -v
  -p codesigning`, lists local valid signing identities, and cross-checks
  them against the certificates the ASC API reports
  (`CertificateApi::list_certificates`) — flagging identities that are
  expired, missing locally, or present locally but revoked/absent on Apple.
  Optionally resolve the `.p8` the CLI would use and confirm it exists and
  is readable.
- **Value:** directly closes the gap that cost ~20 min this session and is
  the local half of klepon's key-drift bug. Bigger surface (new keychain
  interaction, macOS-only) → its own branch.

### 3. `success` trusts the exit code, not the JSON body  — HIGH
`outcome()` (`crates/frontend/src/upload.rs:129`) sets
`success: exit_code == Some(0)`. altool has historically returned 0 while
reporting warnings/soft failures in its JSON body, and `--wait` emits two
documents (status then result) — neither is inspected for an error array.
A caller (and the MCP tool) can be told an upload "succeeded" when altool's
own body says otherwise.
- **Fix:** derive `success` from exit code **and** the parsed body — treat a
  non-empty `product-errors`/`errors` array (across altool's key spellings,
  mirroring the `find_delivery_id` multi-spelling approach) as failure even
  on exit 0. Keep exit code in the struct for debugging.
- **Value:** small, same-file change; removes false "success" reports.

### 4. `.app`-vs-`.pkg` gives a generic "does not exist"  — MEDIUM
In `validate_package`, a `.app` **bundle** (a directory) fails the
extension check and falls through to the generic
`"{} does not exist"` branch — misleading, since it very much exists; it's
just not exported yet. People routinely point the tool at the `.app`.
- **Fix:** detect a `.app` (or any directory) and return a precise message:
  "this is an .app bundle, not an uploadable archive — export it first
  (`xcodebuild -exportArchive`) to produce an .ipa/.pkg". Distinguish
  "directory / wrong kind" from "path missing".
- **Value:** tiny, same-file, removes a recurring confusing error.

### 5. No non-blocking `await-processing` loop  — MEDIUM ✅ SHIPPED (PR #26)
The only way to wait for ASC to finish processing a build is altool's
`--wait`, which blocks the altool process for the whole duration. There's
no poll-based path that uses the builds API (`BuildsApi::list_builds`,
shipped in klepon #8) to report processing state incrementally or let the
caller poll on its own cadence.
- **Fix:** add `asc apps await-processing <build-id|--build-number>` that
  polls `/v1/builds` for `processingState` until `VALID`/`FAILED`/timeout,
  with backoff. Complements klepon's builds API; an MCP-friendly
  alternative to a blocking `--wait`.
- **Value:** better UX for CI/agents; builds on existing klepon surface.
- **Shipped (PR #26):** `asc apps await-processing <APP_ID>` with
  `--version`, `--pre-release-version`, `--interval` (30s),
  `--timeout` (1800s). Shared poll loop + pure `classify()` /
  `AwaitConfig` / `AwaitOutcome` in `crates/frontend/src/await_processing.rs`
  (serializes without key material; 7 in-module tests). Terminal map:
  VALID → success, INVALID/FAILED → failure, else timeout → non-zero exit.
  CLI-only for now; the pure core is MCP-ready if a tool is added later.

### 6. `list_builds` limit=200 pagination  — LOW (doc-only) ✅ SHIPPED (PR #24)
`list_builds` caps at limit=200 with no cursor follow-through. Fine for
normal apps; note it as a known limit. Overlaps klepon's open "MCP
`list_builds` doesn't expose `BuildFilter`" item — fold into that rather
than tracking separately.

## The 3 biggest wins (picked)

1. **#1 — pre-upload `altool --validate-app`.** Highest leverage: converts
   late, expensive upload failures into fast pre-flight ones, squarely
   inside what ascapi already owns, reusing existing auth-parity plumbing.
2. **#2 — `asc signing doctor` (local keychain ↔ ASC cross-check).** The
   one that actually cost this session ~20 min and the local half of
   klepon's unresolved key-drift bug. Highest *pain* addressed; larger
   surface, own branch.
3. **#3 — body-aware `success`.** Smallest, same-file change with outsized
   correctness payoff: stops the tool (and its MCP consumers) from
   reporting a failed upload as successful.

**Sequencing:** #3 then #1 together in `upload.rs` (one file, one test
module, shared `--dry-run`/`altool_args` test scaffolding; #4 is cheap to
fold in alongside). #2 on its own branch (new macOS-only keychain surface,
`security` subprocess, cross-check against `CertificateApi`). Exercise #1
and #3 under klepon's still-open "Live smoke test against ASC" item.

**Status:** #2 shipped (PR #25). The `upload.rs` cluster (#3 + #1, with #4
folded in) is still the next natural branch — one file, shared test
scaffolding, all inside what ascapi already owns.
