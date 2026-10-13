---
name: ascapi-release
description: >
  Cut a new release of the smbcloud-ascapi Rust workspace: merge development into
  main, bump the single workspace version with cargo-workspaces, create the
  Release commit and matching tags, keep server.json in sync, then push the tag
  so release-crates.yml publishes the crates to crates.io and dispatches the
  GitHub and MCP Registry releases. Use whenever the user asks to
  release, cut a version, tag, bump the version, publish to crates.io, or publish
  the MCP server for this repo. Covers the forge/remote quirks (GitHub, remote
  named "setoelkahfi"), the signing identity, the tag-push trigger failure mode
  that's been observed on this forge, and the registry workflow's hard invariants
  that will fail a release if violated.
metadata:
  author: smbcloudXYZ
  version: "1.2"
---

# smbcloud-ascapi release runbook

One Rust workspace, one version. The root `Cargo.toml` `[workspace.package]`
`version` (around line 15) is the single source of truth — every crate inherits
it with `version.workspace = true`. A release bumps that one number and tags
it. Pushing the `vX.Y.Z` tag starts `.github/workflows/release-crates.yml`,
which publishes the crates and then dispatches the GitHub release and the MCP
Registry release.

## Facts that are easy to get wrong

- **Forge is GitHub** (`github.com/smbcloudXYZ`). The `mcp__sigit__*` tools do
  NOT apply here. Use `git` / `gh` via the shell for any forge work.
- **The remote is named `setoelkahfi`, not `origin`.** Push with
  `git push setoelkahfi main` and `git push setoelkahfi --tags`. Confirm with
  `git remote -v` before pushing.
- **Signing identity** (confirmed, and already the repo's local git config):
  - `user.email = 1797197+setoelkahfi@users.noreply.github.com`
  - `user.signingkey = 2B76DF0F04014E36`
  - A signed Release commit shows `sig=G key=2B76DF0F04014E36` under
    `git log -1 --format='sig=%G? key=%GK'`. If it doesn't, stop and fix the
    identity before tagging — do not release unsigned or under the wrong key.
    (The `seto@smbcloud.xyz` / `B5690EEE…` key is a different, non-release
    identity — don't use it here.)
- **Release tool is `cargo-workspaces`** (binary `cargo workspaces`). Install
  with `cargo install cargo-workspaces` if missing.

## Step 1 — merge `development` into `main`

```sh
git checkout main
git pull setoelkahfi main
git merge --no-ff development
```

Use `--no-ff` so the merge from development produces a merge commit — it
makes the release-boundary history legible. Conflicts are usually
`Cargo.lock` only; auto-merge almost always wins because the only real
changes are new crate entries or new dependency additions (the recent
development branches added `tokio` to `mcp`, for instance). Confirm with:

```sh
cargo check --workspace --locked         # MUST pass — this validates Cargo.lock
```

Other files that have diverged between branches in the past:
`release-github.yml`, `release-crates.yml`, `release-mcp-registry.yml`,
`.agents/skills/ascapi-release/SKILL.md`. These are usually non-conflicting
(one side changed, the other didn't); the merge takes the change. If a real
conflict appears, take the version on the branch where the file is currently
in production use — for this repo, that's almost always `main` for docs and
release-mechanics, and `development` for code.

## Step 2 — bump the version and tag

Pick `patch` / `minor` / `major`. To bump to an explicit version (e.g. when
`minor` would compute a version already published to crates.io), use the
`custom` form:

```sh
cargo workspaces version minor \
  --yes --allow-branch main --no-git-push --force '*'
# or:
cargo workspaces version custom 0.3.0 \
  --yes --allow-branch main --no-git-push --force '*'
```

- `--allow-branch main` because releases go out on `main`.
- `--no-git-push` so you control the push (to the `setoelkahfi` remote).
- `--force '*'` bumps every crate to the shared version even if cargo-workspaces
  thinks some are unchanged (single-version workspace).

This creates the `Release X.Y.Z` commit, the `vX.Y.Z` tag, and **8 per-crate
tags** `smbcloud-ascapi-{aso,cli,core,frontend,mcp,pricing,reports,signing}@X.Y.Z`,
all pointing at the Release commit.

## Step 3 — sync `server.json`

`server.json` is NOT touched by cargo-workspaces. The MCP Registry workflow
rewrites it from the tag at publish time, but keep the repo honest by bumping it
in the Release commit anyway. Two version fields:

- top-level `.version`
- `.packages[0].version`

Edit both to the new version, then fold them into the Release commit and move
every tag to the new commit:

```sh
git add server.json
git commit --amend --no-edit
NEW=$(git rev-parse HEAD)
for t in \
  smbcloud-ascapi-aso@X.Y.Z smbcloud-ascapi-cli@X.Y.Z \
  smbcloud-ascapi-core@X.Y.Z smbcloud-ascapi-frontend@X.Y.Z \
  smbcloud-ascapi-mcp@X.Y.Z smbcloud-ascapi-pricing@X.Y.Z \
  smbcloud-ascapi-reports@X.Y.Z smbcloud-ascapi-signing@X.Y.Z \
  vX.Y.Z; do
  git tag -f "$t" "$NEW"
done
```

Beware: any tooling that **amends a co-author trailer** after your commit
changes the hash again. After all amends settle, re-point the tags to the final
HEAD and verify they all land on one commit:

```sh
git tag --points-at HEAD   # must list all 9 tags
git log -1 --format='sig=%G? key=%GK'   # must be sig=G key=2B76DF0F04014E36
```

## Step 4 — final consistency check, then push

All three must read the same version, and the locked build must pass:

```sh
grep -n '"version"' server.json          # both -> X.Y.Z
sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1   # -> X.Y.Z
git status --short                        # clean
cargo check --workspace --locked
```

Push the branch first, then the tags. The tag push is what starts the
publish, and the workflow checks out the tag, so `main` has to be on the
remote before it runs:

```sh
git push setoelkahfi main
git push setoelkahfi --tags --force       # --force only if you moved tags in step 3
```

## Step 5 — `release-crates.yml` publishes the crates

Pushing a `vX.Y.Z` tag starts `.github/workflows/release-crates.yml`. The
per-crate `smbcloud-ascapi-*@X.Y.Z` tags don't match its `v*.*.*` filter, so
they don't start extra runs. Three jobs run in order:

1. **`verify`** fails the run if the tag differs from the workspace version in
   `Cargo.toml`, then runs `cargo fmt --check`, clippy with `-D warnings`,
   `cargo test --workspace --locked`, and `cargo check` on `core`, `aso`,
   `pricing`, `signing` and `reports` on their own.
2. **`publish`** (environment `crates-io`) publishes `core`, `aso`, `pricing`,
   `signing`, `reports`, `frontend`, `mcp`, `cli`, in that order, with
   `cargo publish -p <crate> --locked`. Authentication is crates.io trusted
   publishing: the job exchanges its GitHub OIDC token through
   `rust-lang/crates-io-auth-action`, so there is no registry token secret.
   A crate version that is already on crates.io is skipped, so re-running the
   workflow after a partial failure resumes where it stopped.
3. **`dispatch-distributions`** checks that the GitHub release for the tag
   does not already exist, then dispatches `release-github.yml` (six binaries
   attached to a GitHub release) and `release-mcp-registry.yml`, both with the
   same tag. The existence check makes this job idempotent: a late or duplicate
   tag-push trigger can't re-build the binaries or re-publish the MCP server
   for a release that's already out.

Watch it with:

```sh
gh run list --workflow release-crates.yml --limit 3
gh run watch <run-id>
```

### One-time setup it depends on

- Each of the eight crates has a trusted publisher on crates.io for repository
  `smbcloudXYZ/smbcloud-ascapi` and workflow `release-crates.yml`. If a crate
  is missing one, `cargo publish` fails for that crate and the later crates are
  not published. Add the publisher and re-run.
- If a trusted publisher names an environment, it must be `crates-io`, which
  is the environment the `publish` job runs in.

### When the tag push does not fire the workflow

This has been observed on this forge at least once (v0.2.0 and v0.3.0 first
pushes both failed to trigger `release-crates.yml`, even though the workflow
file was on `main` at HEAD with a valid `push: tags: 'v*.*.*'` filter).
The trigger works on a re-push of the same tag, so the webhook delivery is
just occasionally missed. If a push doesn't fire the workflow within a couple
of minutes:

```sh
gh run list --workflow release-crates.yml --limit 5
# if no recent run for the tag, dispatch by hand:
gh workflow run release-crates.yml --ref vX.Y.Z -f tag=vX.Y.Z
gh run watch $(gh run list --workflow release-crates.yml --limit 1 --json databaseId -q | jq -r '.[0].databaseId')
```

The `dispatch-distributions` existence check makes the manual dispatch safe
even if the tag-push trigger fires later too: the second run will skip the
dispatch step and finish as a no-op.

## Step 6 — MCP Registry release

`release-mcp-registry.yml` is dispatched by step 5, so there is normally
nothing to do here. It authenticates to the MCP Registry via GitHub OIDC (no
secret) and enforces these invariants. A release fails if any is violated:

1. **Version match:** root `Cargo.toml` workspace version == tag (minus the `v`).
   Step 2 guarantees this.
2. **Ownership marker in source:** `mcp-name: io.github.smbcloudXYZ/ascapi` must
   be present in BOTH `README.md` and `crates/cli/README.md`. (The server name
   comes from `server.json` `.name`.)
3. **Ownership marker in the published crate:** the CLI crate downloaded from
   crates.io for this exact version must contain the same marker in its README.
   crates.io versions are immutable, so **the marker must be in the README
   BEFORE you tag**. If a published version lacks it, you cannot fix that
   version; you must cut a new release.

The workflow rewrites `server.json`'s version from the tag, publishes with
`mcp-publisher` (treating "duplicate version" as success), and verifies the
server is listed in the registry. It waits up to ten minutes for crates.io to
index the CLI crate, so it is safe that it starts while step 5 is still
publishing.

To publish the server again for a tag, dispatch it yourself:

```sh
gh workflow run release-mcp-registry.yml --ref vX.Y.Z -f tag=vX.Y.Z
```

## Manual fallback

Only if the workflow can't run (Actions outage, trusted publishing not set up
yet, etc.). Publish in dependency order, because the CLI depends on the rest:

```sh
cargo workspaces publish --from-git --allow-branch main --yes
```

`--from-git` publishes exactly what the tags describe and skips versions
already on crates.io. Then dispatch `release-github.yml` and
`release-mcp-registry.yml` yourself with the tag. **After a manual publish,
still go back to step 5 the next time around** — the workflow is the source
of truth and the manual path exists for emergencies, not as a default.

## Release-order summary

merge → bump+tag → sync server.json into Release commit → verify sig & versions
→ push `main`, then tags → `release-crates.yml` verifies and publishes → it
dispatches the GitHub release and the MCP Registry release. Check the run in
Actions; nothing after the tag push is manual — unless the tag-push trigger
didn't fire, in which case dispatch `release-crates.yml` by hand and continue.

## References

- `../../../.github/workflows/release-crates.yml` — verify, publish and
  dispatch, in full.
- `../../../.github/workflows/release-github.yml` — the binary build and GitHub
  release.
- `../../../.github/workflows/release-mcp-registry.yml` — the registry workflow
  and its invariant checks, in full.
- Past release design notes live in `.agents/plans/` (upload / signing-identity
  behaviour) — check there before reworking release mechanics.