# Releasing

This repo currently has a release path for the `embed-log` CLI binary.

The released CLI is a prebuilt executable. Users do **not** need Rust or Cargo installed.
The frontend assets are embedded into the Rust binary at build time via `rust-embed`.

## Hosted build matrix

`.github/workflows/release-cli.yml` uses standard GitHub-hosted runners. Its native build/test matrix produces:

| Runner | Artifact |
| --- | --- |
| `ubuntu-latest` | `embed-log-x86_64-unknown-linux-gnu.tar.gz` and `.deb` |
| `macos-14` | `embed-log-aarch64-apple-darwin.tar.gz` |
| `macos-13` | `embed-log-x86_64-apple-darwin.tar.gz` |
| `windows-latest` | `embed-log-x86_64-pc-windows-msvc.zip` |

Every matrix entry runs the CLI/core/TUI Rust tests, builds a native release binary, packages it, and smoke-tests the extracted archive before the publish job can create a release. Unix entries also run the checksum-verified self-update fixture against that packaged binary; Windows verifies its documented installer-only update guidance.

## Create a CLI release

1. Create and push a tag:

```bash
git tag v1.0.0
git push origin v1.0.0
```

2. The `Release CLI` workflow will test, build, package, and smoke-test each platform artifact, generate `SHA256SUMS`, attach `install.sh` / `install.ps1`, and publish a GitHub Release only after every platform succeeds.

You can also run the workflow manually from GitHub Actions. To test every hosted build/test/package job without publishing, select the branch, set `tag` to a prospective value such as `v0.0.0-test`, and leave **publish release** unchecked. To publish from a branch, provide the final tag and check **publish release**.

## User install commands

macOS/Linux latest release:

```bash
curl -fsSL https://github.com/embedlab-tech/embed-log/releases/latest/download/install.sh | sh
```

macOS/Linux pinned release:

```bash
curl -fsSL https://github.com/embedlab-tech/embed-log/releases/download/v1.0.0/install.sh | EMBED_LOG_VERSION=v1.0.0 sh
```

macOS/Linux custom install directory:

```bash
curl -fsSL https://github.com/embedlab-tech/embed-log/releases/latest/download/install.sh | INSTALL_DIR=/usr/local/bin sh
```

macOS/Linux automatic shell profile update:

```bash
curl -fsSL https://github.com/embedlab-tech/embed-log/releases/latest/download/install.sh | EMBED_LOG_UPDATE_PATH=1 sh
```

Windows latest release, from PowerShell:

```powershell
irm https://github.com/embedlab-tech/embed-log/releases/latest/download/install.ps1 | iex
```

Windows pinned release:

```powershell
$env:EMBED_LOG_VERSION = "v1.0.0"; irm https://github.com/embedlab-tech/embed-log/releases/download/v1.0.0/install.ps1 | iex
```

Windows custom install directory:

```powershell
$env:INSTALL_DIR = "C:\Tools\embed-log"; irm https://github.com/embedlab-tech/embed-log/releases/latest/download/install.ps1 | iex
```

Default install locations are:

```text
macOS/Linux: ~/.local/bin/embed-log
Windows:     %LOCALAPPDATA%\Programs\embed-log\bin\embed-log.exe
```

The installers ask before updating PATH when run interactively. Windows still updates the user PATH by default for non-interactive installs; macOS/Linux prints PATH instructions by default for non-interactive installs, or updates your shell profile when `EMBED_LOG_UPDATE_PATH=1` is set. Set `EMBED_LOG_NO_PROMPT=1` for non-interactive installs.

## Direct downloads

Each GitHub Release contains:

```text
embed-log-x86_64-unknown-linux-gnu.tar.gz
embed-log-aarch64-apple-darwin.tar.gz
embed-log-x86_64-apple-darwin.tar.gz
embed-log-x86_64-pc-windows-msvc.zip
install.sh
install.ps1
SHA256SUMS
```

Windows users can either use `install.ps1` or download and extract the `.zip` from the release page.

## Local packaging smoke test

From macOS/Linux:

```bash
cargo build --locked --release --package embed-log-cli --bin embed-log
./scripts/package-cli.sh aarch64-apple-darwin # or x86_64-apple-darwin / x86_64-unknown-linux-gnu
```

From Windows PowerShell:

```powershell
cargo build --locked --release --package embed-log-cli --bin embed-log
./scripts/package-cli.ps1 -Target x86_64-pc-windows-msvc
```

## Tauri desktop app

The desktop/Tauri release path is intentionally separate. When ready, add a second workflow that builds native Tauri bundles on each OS runner:

- Linux: `.AppImage` / `.deb`
- Windows: `.msi` / `.exe`
- macOS: `.dmg` / `.app`

Signing/notarization can be added later for Windows/macOS.
