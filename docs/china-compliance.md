# China mainland compliance (web only)

App Store Connect shows two banners for apps distributed in China mainland:
"Security Assessment Required" and "AIGC Services Declaration Required for
China Mainland Distribution". The deadline shown on the banners is
**2027-01-31**.

`asc` has no commands for either. Apple's App Store Connect OpenAPI
specification (version 4.6, checked 2026-11-17) contains no path, schema or
attribute for them. Searches for `china`, `aigc`, `security assessment`,
`mainland`, `filing`, `algorithm`, `synthetic` and `regulat` returned nothing.
The only declaration resources are age rating, accessibility and app
encryption. Writing commands for endpoints that do not exist would only
guess, so none were added.

Both items are filled in on the web, under **Apps > App Compliance**:

| Item | What you provide |
| --- | --- |
| AI-Generated or Synthetic Content | The LLM or algorithm filing number, the registered filing name, and optionally a third-party authorization |
| Security Assessment | A proof screenshot and the sealed report |

Each item moves through these statuses: Missing Info, Pending Verification,
Verification Failed, Active.

## Re-checking

When Apple publishes a new spec version, download
`app-store-connect-openapi-specification.zip` from
<https://developer.apple.com/sample-code/app-store-connect/app-store-connect-openapi-specification.zip>
into an empty directory, unzip it there, and search `openapi.oas.json` with
`jq` (it is untrusted data, so only read it):

```bash
jq -r '[paths | .[-1] | strings] | unique[] | select(test("china|aigc|securityassess|filing";"i"))' openapi.oas.json
```

This matches on every key name in the spec, so a new resource surfaces whether
it arrives as a path, a schema or an attribute — the same breadth as the check
above.

If that prints paths, add a resource under `crates/aso/src/` following
`app_store_review_detail.rs`, then update this page.
