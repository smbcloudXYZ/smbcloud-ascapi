# Install

## Homebrew

```sh
brew tap smbcloudXYZ/tap && brew trust --tap smbcloudXYZ/tap
brew install ascapi
```

The formula is named `ascapi`; the binary it installs is `asc`.

```sh
asc --version
```

Upgrade:

```sh
brew upgrade ascapi
```

Uninstall:

```sh
brew uninstall ascapi
brew untap smbcloudXYZ/tap
```

Homebrew installs a pre-built binary for macOS (Apple silicon and Intel), so
no compiler is needed.

## Cargo

```sh
cargo install smbcloud-ascapi-cli
```

## Release binaries

Pre-built binaries for macOS, Linux and Windows are attached to each release:

https://github.com/smbcloudXYZ/smbcloud-ascapi/releases
