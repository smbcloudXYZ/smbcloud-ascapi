<p align="center">
  <img src="https://avatars.githubusercontent.com/u/89791739?s=200&v=4" alt="smbCloud Logo" width="96">
</p>

<h1 align="center">smbcloud-ascapi</h1>

<p align="center"><strong>App Store Connect, Google Play, Microsoft Store and Huawei AppGallery from your terminal, your scripts and your agents.</strong></p>

<p align="center">
  <a href="https://crates.io/crates/smbcloud-ascapi-cli"><img alt="Crates.io" src="https://img.shields.io/crates/v/smbcloud-ascapi-cli"></a>
  <a href="https://github.com/smbcloudXYZ/homebrew-tap"><img alt="Homebrew" src="https://img.shields.io/badge/homebrew-tap-orange?logo=homebrew"></a>
  <a href="https://registry.modelcontextprotocol.io/v0.1/servers?search=io.github.smbcloudXYZ/ascapi"><img alt="MCP server" src="https://badge.mcpx.dev?type=server&features=resources,tools" title="MCP server with features"></a>
  <a href="https://github.com/smbcloudXYZ/smbcloud-ascapi/actions/workflows/ci.yml"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/smbcloudXYZ/smbcloud-ascapi/ci.yml?label=ci"></a>
  <a href="https://github.com/smbcloudXYZ/smbcloud-ascapi/blob/main/LICENSE"><img alt="Apache-2.0 License" src="https://img.shields.io/badge/license-Apache%202.0-blue.svg"></a>
</p>

`asc` manages your app store listings and releases: App Store Connect, Google Play, the Microsoft Store and Huawei AppGallery. The CLI and the MCP server sit on one shared core, so a person at a terminal, a CI job and an AI agent run the same operations.

MCP Registry name: `mcp-name: io.github.smbcloudXYZ/ascapi`

## Install

```sh
brew tap smbcloudXYZ/tap && brew trust --tap smbcloudXYZ/tap && brew install ascapi
cargo install smbcloud-ascapi-cli
```

Pre-built binaries are on [GitHub Releases](https://github.com/smbcloudXYZ/smbcloud-ascapi/releases). More detail in [docs/install.md](docs/install.md).

## Use it your way

- **In the terminal.** `asc apps ...` lists apps, builds and versions. Credentials are in [docs/credentials.md](docs/credentials.md).
- **In scripts and CI.** Every write takes `--dry-run`, and credentials come from environment variables. See [docs/credentials.md](docs/credentials.md).
- **With AI agents.** `asc --mcp` serves the [Model Context Protocol](https://modelcontextprotocol.io) over stdio. See [docs/architecture.md](docs/architecture.md).

## Docs

[Install](docs/install.md) · [Credentials](docs/credentials.md) · [AppGallery](docs/appgallery.md) · [Architecture](docs/architecture.md)

## Contributing

Open pull requests against `development`.

## License

[Apache-2.0](LICENSE)

## Copyright

© 2026 Seto Elkahfi ([smbCloud](https://smbcloud.xyz)). Distributed by [Splitfire AB](https://5mb.app).
