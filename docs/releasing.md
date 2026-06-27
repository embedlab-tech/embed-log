# Releasing

This repo currently has a release path for the `embed-log` CLI binary.

The released CLI is a prebuilt executable. Users do **not** need Rust or Cargo installed.
The frontend assets are embedded into the Rust binary at build time via `rust-embed`.

## Self-hosted runner labels

The workflow `.github/workflows/release-cli.yml` expects these GitHub self-hosted runners:

| Machine | Required labels | Artifact |
| --- | --- | --- |
| Linux x64 mini PC | `self-hosted`, `Linux`, `X64` | `embed-log-x86_64-unknown-linux-gnu.tar.gz` |
| Windows x64 PC | `self-hosted`, `Windows`, `X64` | `embed-log-x86_64-pc-windows-msvc.zip` |
| Apple Silicon Mac | `self-hosted`, `macOS`, `ARM64` | `embed-log-aarch64-apple-darwin.tar.gz` and `embed-log-x86_64-apple-darwin.tar.gz` |

These are GitHub's default OS/architecture labels for self-hosted runners. If your runner labels differ, update `runs-on` in the workflow.

## Create a CLI release

1. Make sure all three runners are online and not sleeping.
2. Create and push a tag:

```bash
git tag v0.1.0
git push origin v0.1.0
```

3. The `Release CLI` workflow will build each platform artifact, generate `SHA256SUMS`, attach `install.sh` / `install.ps1`, and publish a GitHub Release.

You can also run the workflow manually from GitHub Actions. If running from a branch, provide the release tag input, for example `v0.1.0`.

## User install commands

macOS/Linux latest release:

```bash
curl -fsSL https://github.com/krezolekcoder/embed-log/releases/latest/download/install.sh | sh
```

macOS/Linux pinned release:

```bash
curl -fsSL https://github.com/krezolekcoder/embed-log/releases/download/v0.1.0/install.sh | EMBED_LOG_VERSION=v0.1.0 sh
```

macOS/Linux custom install directory:

```bash
curl -fsSL https://github.com/krezolekcoder/embed-log/releases/latest/download/install.sh | INSTALL_DIR=/usr/local/bin sh
```

macOS/Linux automatic shell profile update:

```bash
curl -fsSL https://github.com/krezolekcoder/embed-log/releases/latest/download/install.sh | EMBED_LOG_UPDATE_PATH=1 sh
```

Windows latest release, from PowerShell:

```powershell
irm https://github.com/krezolekcoder/embed-log/releases/latest/download/install.ps1 | iex
```

Windows pinned release:

```powershell
$env:EMBED_LOG_VERSION = "v0.1.0"; irm https://github.com/krezolekcoder/embed-log/releases/download/v0.1.0/install.ps1 | iex
```

Windows custom install directory:

```powershell
$env:INSTALL_DIR = "C:\Tools\embed-log"; irm https://github.com/krezolekcoder/embed-log/releases/latest/download/install.ps1 | iex
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
