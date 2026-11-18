# Amazon Appstore

`asc amazon` uses a Login with Amazon Security Profile with App Submission API
access enabled. Save `{"client_id": "...", "client_secret": "..."}` to a file
and point `AMAZON_APPSTORE_CREDENTIALS_JSON_PATH` at it (or pass
`--credentials`). The token is requested with `client_credentials` and scope
`appstore::apps:readwrite`. The secret is never printed.

```bash
asc amazon edits get <APP_ID>                  # the open edit, or null
asc amazon edits create <APP_ID>
asc amazon apks upload <APP_ID> app.apk        # replaces (ETag) the edit's only APK
asc amazon apks upload <APP_ID> app.apk --apk-id <APK_ID>   # required when the edit has several
asc amazon apks list <APP_ID>
asc amazon targeting get <APP_ID> <APK_ID>
asc amazon targeting set <APP_ID> <APK_ID> --file targeting.json
asc amazon edits validate <APP_ID>
```

Every write honors `--dry-run`. There is no submit: review the edit and submit
it in the Developer Console. Targeting documents are passed through as Amazon
returns them, so read with `get`, change the fields you mean to, then `set`.
