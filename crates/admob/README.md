# smbcloud-ascapi-admob

AdMob API client behind `asc admob`: accounts, apps, ad units, and rewarded
ad unit creation. AdMob accepts user OAuth only (no service accounts), so
sign-in is an installed-app flow with PKCE and a loopback redirect. The
refresh token is cached at `~/.config/asc/admob-token.json` (mode 0600).

See [`docs/admob.md`](../../docs/admob.md) for the OAuth client setup and the
limited-access note on `adUnits.create`.
