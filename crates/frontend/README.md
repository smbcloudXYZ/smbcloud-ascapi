# smbcloud-ascapi-frontend

The operations the `asc` command line and its MCP server share: credential
resolution from the environment, result shaping, certificate issuance,
profile download, review submission, build processing waits, and the
`xcrun altool` upload.

Keeping this in one crate means the two front ends agree by construction.
It also enforces the rule that results never carry key material or
certificate bodies, since one of the consumers is a language model.

This is an internal building block of
[smbcloud-ascapi](https://github.com/smbcloudXYZ/smbcloud-ascapi). If you want
the tool, install [`smbcloud-ascapi-cli`](https://crates.io/crates/smbcloud-ascapi-cli).

See the [API docs](https://docs.rs/smbcloud-ascapi-frontend).

## Copyright

© 2026 PT Sigit Mitra Bangun ([smbCloud](https://smbcloud.xyz)).
