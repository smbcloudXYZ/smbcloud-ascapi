# AppGallery

`asc appgallery` uses an AppGallery Connect Service Account (developer-level,
with a publishing role), not the App Store Connect key:

| Flag | Environment variable | Notes |
| --- | --- | --- |
| `--service-account` | `APPGALLERY_SERVICE_ACCOUNT_JSON_PATH` | The `*private.json` file; or its contents in `APPGALLERY_SERVICE_ACCOUNT_JSON`. |
| `--client-id` | `APPGALLERY_CLIENT_ID` | Optional `client_id` header. |

```sh
asc appgallery app-id xyz.smbcloud.SiGit
asc appgallery listings list <APP> [--lang en-US]          # every field the API returns
asc appgallery listings update <APP> --lang en-US --brief "..." --release-notes-file notes.txt
asc appgallery listings delete <APP> --lang id
asc appgallery info update <APP> --privacy-policy URL [--category ID --category-tag ID]
    [--publish-country CODES] [--developer-website URL] [--device-types JSON] ...
asc appgallery files upload <APP> --type icon --lang en-US icon-216.png
asc appgallery files upload <APP> --type screenshots --orientation portrait --lang en-US 1.png 2.png
asc appgallery files upload <APP> --type package siGit.apk      # chineseMainlandFlag=0 unless --chinese-mainland
asc appgallery packages list <APP>
asc appgallery packages status <APP> <PKG_ID>...
asc appgallery gms <APP> --needs-gms false
asc appgallery submit <APP> [--release-time T] [--remark TEXT] [--phased-start T --phased-end T --phased-percent P --phased-description D]
asc appgallery phased-release <APP> (--to-full | --state SUSPEND|RELEASE | --phased-percent P ...)
asc appgallery release-time <APP> (--immediately | --at T)
```

`<APP>` is the numeric app id or the package name. Every write takes
`--dry-run`. Binding an image or video for a language clears that language's
text; `files upload` reads the text first and writes it back if it went
missing. MCP has three read-only tools: `appgallery_app_info_get`,
`appgallery_package_list` and `appgallery_package_status`.

Not covered: the "download mode" submit and package calls, which need a
public package URL and a callback server for Huawei to report to.
