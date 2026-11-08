# First release of a new app on Google Play

The Play Developer API cannot do everything for an app that has never been published. This page
lists what stays in Play Console, what `asc play` does, and the exact errors you get if you try the
API anyway. It was written from the first release of `com.ondeinference.OndeOnde` (2026-11-08).
Anything not tried is marked untested.

The short answer to "does the first submission have to be manual?" is yes. After the first
release is published, the API takes over.

## Checklist, in order

| # | Step | Console or `asc` | Notes |
|---|---|---|---|
| 1 | Create the app | Console only | The androidpublisher discovery document has no create endpoint. |
| 2 | Service account | Once per developer account | See [one-time setup](#one-time-setup-per-developer-account). One account serves every app. |
| 3 | First `.aab` | Console | `asc` has no bundle upload yet (issue #61). Upload it as a draft internal release, then roll it out in Console. |
| 4 | Version-code warning ("significantly higher than your previous") | Console | Click "Proceed anyway". A `YYYYMMDDhh` version code stays under the 2,100,000,000 cap through 2099. |
| 5 | One-time products | `asc play products create` | Only after an uploaded build declares `com.android.vending.BILLING`. Play does not allow creating the product before that. |
| 6 | Listing text, images, release notes | `asc play listings update`, `images replace`, `tracks set-notes` | Works on a draft app. All three committed. |
| 7 | Content rating, target audience, ads, app access, account-deletion URL, category and tags | Console only | No API. Data safety has `applications.dataSafety`, but it takes the CSV exported from Console. |
| 8 | Open testing countries and feedback contact | Console only | No API. |
| 9 | First release on production or open testing (`beta`) | **Console only** | Create the release and send it for review there. See below. |

### Why step 9 is manual

On an app with no published release, the API rejects both of these, on `production` and on `beta`:

```
Staged rollout is not permitted on draft app.
Only releases with status draft may be created on draft app.
```

So on a draft app a workflow can only create draft releases. The first public release is a
Console step.

Untested: whether the API can complete (not draft) a release on the internal track of a draft app.
The internal release of Onde Onde was rolled out from Console.

## One-time setup per developer account

Done by hand once; every app on the account reuses it.

1. In Google Cloud, create or pick a project and enable the Google Play Android Developer API.
2. Create a service account in that project and download a JSON key.
3. In Play Console, Users and permissions, invite the service account's email with account-level
   permissions. The commands in this repo need to read app information, manage store presence
   (listings, images), release to testing tracks and production, and manage products. Which set is
   the minimum was not tested.
4. Hand the key to `asc` with `--play-service-account`, `PLAY_SERVICE_ACCOUNT_JSON_PATH`, or the
   contents in `PLAY_SERVICE_ACCOUNT_JSON`. See [credentials](credentials.md).

## After the first publish

Later releases are expected to go through the API: `asc play tracks promote` and staged rollout.
Untested for Onde Onde until Google approves its first open test. When someone runs a dry run of
`tracks promote` against a published app and it succeeds, change "expected" to "verified" here.

## Account rules that affect the first release

- A personal developer account created after November 2023 needs a closed test with at least 12
  testers for 14 days before it can request production access. Organisation accounts are exempt.
- With Managed publishing on, an approved release waits until a person presses Publish in Console.
