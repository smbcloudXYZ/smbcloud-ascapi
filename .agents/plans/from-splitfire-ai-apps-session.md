# Findings — SplitFire AI apps release session (KaroKowe Jam TestFlight) (closed)

Context: shipping a macOS build (KaroKowe Jam) to TestFlight via the `asc`
CLI. Concrete app id / bundle id / build number are omitted — this is a public
repo; keep plan files to generalizable `asc` findings, not per-app identifiers.

## 1. The release `asc` binary can silently lag `development`  — DOC-NOTE ✅ SHIPPED

The installed/release `asc` predated the `testflight` subcommand, yet
`asc apps upload` still worked — because upload only shells out to `altool`
and doesn't touch the newer code paths. So an upload succeeding is **not**
evidence that the binary is current.

**Rule:** before relying on a newly-added subcommand (e.g. `testflight`),
rebuild `asc` from `development` first (`cargo build --release`), or you'll hit
`unrecognized subcommand` on a binary that otherwise looks healthy.

**Where this lives:** the rule is captured in `AGENTS.md` § "Verify before
relying on a freshly built `asc`" so a future session doesn't relearn it.

## 2. `testflight groups` takes a nested subcommand, not an app id  — ✅ SHIPPED

`asc testflight groups <APP_ID>` would have failed with
`error: unrecognized subcommand '<APP_ID>'`. Confirmed command surface:

```
asc testflight groups list <APP_ID>
asc testflight groups get <GROUP_ID>
asc testflight groups add-build --build-id <BUILD_ID>... <GROUP_ID>

asc testflight app-review-detail get <APP_ID>
asc testflight app-review-detail update ...        # contact + demo-account details

asc testflight app-review-submissions list <BUILD_ID>
asc testflight app-review-submissions submit <BUILD_ID>
```

Notes:
- `groups add-build` and `app-review-submissions submit` are **CLI-only** by
  design (adding a build to an external group / submitting to Beta App Review
  can trigger review — not something an MCP tool call should do unattended).
- Both support `--dry-run` to print the request body before writing.

## 3. Finish-the-release recipe  — DOC-NOTE

1. Upload: `asc apps upload <PKG> --wait` (always `--p8-file-path`; never the
   script's raw `altool`, which can pick a stale `AuthKey_<id>.p8` → 401).
2. Poll to VALID: `asc apps builds <APP_ID> --version <BUILD_NUMBER>` → grab the
   build id once `processingState` is VALID.
3. Beta review contact details: `asc testflight app-review-detail update ...`.
4. Submit: find the Public Link group via `asc testflight groups list <APP_ID>`,
   then either `groups add-build --build-id <BUILD_ID> <GROUP_ID>` or
   `app-review-submissions submit <BUILD_ID>`.

This recipe is folded into the "Builds API (CLI)" section of `AGENTS.md`
under the upload + processing sequence.

## 4. Process note  — DOC-NOTE ✅ SHIPPED

Plan files and any other changes land on a feature branch and go in through a
PR — never a direct commit to `development` (or `main`). It's easy to slip when
a change feels like "just docs"; treat the branching rule as unconditional.
See the **Git branching workflow** section of `AGENTS.md`.

## Status
Closed. Finding #2 shipped as code (testflight CLI tree plus
build-localizations for "What to Test"). Findings #1, #3, #4 are now
doc-notes absorbed into `AGENTS.md`. The recipe in #3 is the operational
TL;DR for what `AGENTS.md` already documents command-by-command.