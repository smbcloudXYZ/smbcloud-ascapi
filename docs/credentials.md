# Credentials

The CLI signs requests with an App Store Connect API key:

| Flag | Environment variable | Notes |
| --- | --- | --- |
| `--key-id` | `ASC_API_KEY` | The key id. There is no `ASC_KEY_ID`. |
| `--issuer-id` | `ASC_ISSUER_ID` | The issuer id from App Store Connect. |
| `--private-key-path` | `ASC_PRIVATE_KEY_PATH` | Defaults to `~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8`. |

With `--mcp`, credentials are resolved on each tool call, so the server can
list its tools before they are configured.
