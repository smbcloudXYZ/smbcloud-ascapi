# Xiaomi GetApps

`asc xiaomi` calls the GetApps developer interface on
`global.developer.mi.com/third-api/…`. Each request is signed with HMAC-SHA1
using the app's App Secret. Xiaomi documents two endpoints publicly: the
order query and the APK upload address. The rest of the upload and the
submission are documented on a private Xiaomi wiki, so the listing, the APK
upload and the review submission still happen in the console.

## Credentials

The App ID and App Secret are per app: open the app in the console and click
the eye icon next to them. The email, developer id and Xiaomi user id belong
to the developer account, and GetApps checks that they all match. Put them in
a dotenv file, pass it with `--xiaomi-env` or `XIAOMI_ENV_PATH`, or export
the variables:

```bash
XIAOMI_APP_ID=...
XIAOMI_APP_SECRET=...
XIAOMI_PACKAGE=com.example.app
XIAOMI_EMAIL=...           # the developer account's email
XIAOMI_DEVELOPER_ID=...    # a number
XIAOMI_USER_ID=...         # the Xiaomi account id, a number
```

The App Secret is never printed, and `--dry-run` withholds the signature too.

## Commands

```bash
asc xiaomi upload-url                                   # credentials and signature check
asc xiaomi orders --start 2026-11-01 --end 2026-11-19   # up to 30 days, UTC dates or Unix seconds
asc xiaomi orders --start 2026-11-01 --end 2026-11-19 --pay-status 2,6 --regions ES,RO
asc --dry-run xiaomi orders --start 2026-11-01 --end 2026-11-19   # print the signed request, send nothing
```

`upload-url` fetches the address and uploads nothing. Signed URLs in the answer
are cut to scheme, host and path.

`orders` only works once Xiaomi has allowlisted the account. Ask
overseas-dev@xiaomi.com. Until then GetApps answers code 999 with "does not
have /third-api/order/v1/query permission". The limit is one request a second.

## Error codes

| Code | Meaning |
|---|---|
| 999 | A header is missing or wrong, the ids don't belong together, or the account lacks permission for the endpoint |
| 1001 | No App Secret found for the App ID |
| 1002 | Signature mismatch. Xiaomi's docs spell the label both `Timestamp:` and `TimeStamp:`; `asc` signs `Timestamp:`, as both worked examples do |
| 3001 | The timestamp is more than 30 minutes old: check the clock |
| 3002 | Duplicate nonce |

## Publishing without the API

For an app that is also on Google Play, Xiaomi asks for a PSV build: the same
package name, the same signing key as Play (Google's app signing key, not
your upload key) and a higher versionCode than Play's. To get an APK signed
with Google's key, upload the bundle to Play's internal testing track as a
draft, then download the signed universal APK from the App bundle explorer.
