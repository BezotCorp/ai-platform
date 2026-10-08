# Native Binary Packages for BCAIP

This directory contains the npm package scaffolding for distributing the
`bcaip` Rust binary as platform-specific npm packages.

## Packages

| Package | Platform |
|---------|----------|
| `@bezotcorp/bcaip-binary-darwin-arm64` | macOS Apple Silicon |
| `@bezotcorp/bcaip-binary-darwin-x64` | macOS Intel |
| `@bezotcorp/bcaip-binary-linux-arm64` | Linux ARM64 |
| `@bezotcorp/bcaip-binary-linux-x64` | Linux x64 |
| `@bezotcorp/bcaip-binary-win32-x64` | Windows x64 |

## Usage

These are platform-specific implementation dependencies and are not intended
to be installed directly. Install `@bezotcorp/bcaip-acp` instead. It installs the
appropriate package automatically and provides the `bcaip` command. Each
binary package contains its native executable. Its platform-specific internal
command preserves executable permissions during npm packing;
`@bezotcorp/bcaip-acp` remains the sole owner of the supported `bcaip` command.

## Release preparation

The `.github/workflows/publish-npm.yml` workflow downloads the binaries from an
exact versioned BCAIP release and prepares the platform package tarballs.
By default it only uploads the verified tarballs as a workflow artifact. Set
the manual `publish` input to publish them through the protected npm production
environment.
