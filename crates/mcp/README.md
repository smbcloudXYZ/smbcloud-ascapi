# smbcloud-ascapi-mcp

The MCP contract and stdio server for App Store Connect, behind
`asc --mcp`. It exposes the tool set the CLI has, minus certificate
revocation, which stays on the command line on purpose: it invalidates every
profile embedding the certificate, irreversibly.

stdout belongs to the protocol, so nothing in this crate writes to it.
Tool results never include private keys or certificate bodies.

Most users want the binary:

```bash
cargo install smbcloud-ascapi-cli
asc --mcp
```

Embedders can use `ascapi_mcp_tools!` and `AscapiMcpServer` to re-expose
the same contract. See the [API docs](https://docs.rs/smbcloud-ascapi-mcp).

## Copyright

© 2026 PT Sigit Mitra Bangun ([smbCloud](https://smbcloud.xyz)).
