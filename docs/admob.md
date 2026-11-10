# AdMob (`asc admob`)

`asc admob` reads AdMob accounts, apps and ad units, and can create a rewarded
ad unit. It uses the AdMob API, which accepts user OAuth only. There are no
service accounts, so this is a one-time browser sign-in.

## One-time setup

1. In Google Cloud Console, pick a project and enable the **AdMob API**.
2. Create an OAuth client of type **Desktop app** (APIs & Services, Credentials).
   Configure the consent screen if asked; add yourself as a test user while
   the screen is in testing.
3. Download the client JSON and keep it outside any repo.
4. Sign in as the Google user that owns the AdMob publisher account:

```bash
export ADMOB_OAUTH_CLIENT_JSON_PATH=~/path/to/client.json   # or --admob-oauth-client
asc admob login
```

The browser opens, Google redirects to a loopback port on `127.0.0.1`, and the
code is exchanged with PKCE. Only the refresh token is stored, at
`~/.config/asc/admob-token.json` with mode 0600. Later calls refresh access
tokens silently. Delete the file to sign out. Tokens and the client secret are
never printed.

## Commands

```bash
asc admob accounts
asc admob apps list [--account pub-...]
asc admob adunits list [--app ca-app-pub-...~...] [--account pub-...]
asc admob adunits create --app ca-app-pub-...~... --name "Karaoke rewarded" \
    --format rewarded --reward-amount 50 --reward-item Semitones
```

`--account` defaults to the only account the user has. `--dry-run` on `create`
prints the request body and needs neither credentials nor a sign-in. After a
real create, `asc` reads the units back to confirm. If that read fails or the unit is not listed yet, the create still counts as done and the result carries a `warning`, so check `adunits list` before retrying.

The MCP server exposes `admob_apps_list` and `admob_adunits_list`, both
read-only. They never open a browser; without a cached sign-in they say to run
`asc admob login`.

## Limited access on create

`accounts.adUnits.create` (v1beta, scope `admob.monetization`) is limited
access. Without a grant from your AdMob account manager it answers **403**, and
`asc` reports that as a limited-access error. When that happens:

1. Create the unit in the console at
   `https://admob.google.com/v2/apps/<numeric app id>/adunits/create`.
2. Run `asc admob adunits list --app <appId>` to read its ID.

## What the API cannot do

These stay manual console steps:

- the server-side verification (SSV) callback URL on a rewarded unit
- GDPR / consent messages
- app-ads.txt verification
- Google Play's "Contains ads" and Data safety forms (those are Play Console)
