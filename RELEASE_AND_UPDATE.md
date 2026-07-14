# Release and self-update plan

## Goal

Ship the Rust rewrite as a self-contained `embed-log` CLI with embedded frontend assets and a boring install/update story across Linux, macOS, and Windows.

Primary user paths:

1. First install via one-line installer.
2. Subsequent upgrades via `embed-log update`.
3. Manual fallback via direct download of release archives.

Non-goals for the first implementation:

- Tauri desktop app updater
- commit-SHA update channel parity with the legacy Python flow
- package-manager-specific distribution (`brew`, `winget`, `apt`, etc.)
- delta patches / binary diffs
- automatic rollback orchestration beyond a local backup file

---

## Current state

### Already present in the rewrite

- release packaging workflow: `.github/workflows/release-cli.yml`
- packaged release archives per platform
- `install.sh` for macOS/Linux
- `install.ps1` for Windows
- embedded frontend assets inside the release binary
- `embed-log version` command already exists

### Gaps relative to the old install story on `main`

`main` currently documents:

- install from raw branch scripts
- uninstall scripts
- `embed-log update`
- `embed-log update --sha <sha>`

The rewrite already moved in a better direction for first install — release artifacts rather than branch scripts — but does not yet provide a built-in self-update path.

That means merging the rewrite without an update plan would regress the current documented workflow.

---

## Recommended end-state

### User-facing install

#### macOS / Linux

```bash
curl -fsSL https://github.com/embedlab-tech/embed-log/releases/latest/download/install.sh | sh
```

#### Windows PowerShell

```powershell
irm https://github.com/embedlab-tech/embed-log/releases/latest/download/install.ps1 | iex
```

Properties:

- no Rust/Cargo required
- no Node/frontend checkout required
- installs a single self-contained binary
- versioned release artifacts, not repository HEAD

### User-facing update

```bash
embed-log update
```

Optional flags:

```bash
embed-log update --check
embed-log update --version v1.2.0
embed-log update --prerelease
embed-log update --yes
embed-log update --repo owner/repo
embed-log update --allow-downgrade
embed-log update --output json
```

Recommended first-scope subset:

- `embed-log update`
- `embed-log update --check`
- `embed-log update --version <tag>`
- `embed-log update --yes`
- `embed-log update --json`

Do not implement `--sha` in the first rewrite release. Release-tag updates are simpler, safer, and align with packaged binaries.

---

## Release artifact model

Each GitHub Release should publish:

```text
embed-log-x86_64-unknown-linux-gnu.tar.gz
embed-log-aarch64-apple-darwin.tar.gz
embed-log-x86_64-apple-darwin.tar.gz
embed-log-x86_64-pc-windows-msvc.zip
install.sh
install.ps1
SHA256SUMS
```

This already matches the rewrite release workflow and should remain the single source of truth.

### Required embedded build metadata

The binary should expose at least:

- semantic version/tag
- git commit SHA
- build date/time
- target triple
- frontend asset build/version identifier if available

Extend `embed-log version` to report this, both human and JSON.

Example human output:

```text
embed-log 1.1.6
  commit:   abcdef123456
  built:    2026-06-23T19:00:00Z
  target:   x86_64-unknown-linux-gnu
  path:     /home/user/.local/bin/embed-log
```

Example JSON:

```json
{
  "version": "1.1.6",
  "commit": "abcdef123456",
  "built_at": "2026-06-23T19:00:00Z",
  "target": "x86_64-unknown-linux-gnu",
  "executable": "/home/user/.local/bin/embed-log"
}
```

This metadata is also the basis for updater diagnostics.

---

## Updater architecture

## Command surface

Add a new top-level CLI command:

```text
embed-log update
```

Suggested sub-behaviors:

- `embed-log update` — install latest newer stable release
- `embed-log update --check` — report whether an update is available, make no changes
- `embed-log update --version vX.Y.Z` — install an explicit release tag
- `embed-log update --prerelease` — allow pre-release targets when selecting latest
- `embed-log update --yes` — skip interactive confirmation
- `embed-log update --json` — machine-readable result
- `embed-log update --repo owner/repo` — override source repo for testing/forks
- `embed-log update --allow-downgrade` — explicit downgrade escape hatch

Recommended first implementation: no subcommand nesting. Keep it flat.

---

## Update source

Use GitHub Releases as the only update source in phase 1.

Required lookups:

1. Resolve target repo.
2. Resolve requested version:
   - latest stable release by default
   - explicit tag when `--version` is supplied
   - latest prerelease only when `--prerelease` is enabled
3. Find platform-matching archive asset.
4. Download matching `SHA256SUMS`.
5. Verify checksum before replacement.

### Why releases, not branch HEAD

- reproducible artifacts
- immutable version tags
- clear provenance
- aligned with first-install scripts
- avoids source build requirements on end-user machines

---

## Target detection

Updater must map runtime platform to the release asset name.

Initial supported targets:

| Runtime | Asset |
| --- | --- |
| Linux x86_64 | `embed-log-x86_64-unknown-linux-gnu.tar.gz` |
| macOS arm64 | `embed-log-aarch64-apple-darwin.tar.gz` |
| macOS x86_64 | `embed-log-x86_64-apple-darwin.tar.gz` |
| Windows x86_64 | `embed-log-x86_64-pc-windows-msvc.zip` |

If the running platform is unsupported, fail explicitly with a clear error.

Do not guess or silently fall back to another asset.

---

## Install-path model

Updater should replace the currently running binary in place.

That requires discovering:

- absolute path to current executable
- whether its parent directory is writable
- whether replacement should proceed directly or via helper

The updater must never modify user config or session logs.

### Installer defaults

Current installer defaults are already reasonable:

- macOS/Linux: `~/.local/bin/embed-log`
- Windows: `%LOCALAPPDATA%\Programs\embed-log\bin\embed-log.exe`

The updater should work regardless of install location, as long as the path is writable.

---

## Platform-specific replacement strategy

### Linux / macOS

Safe direct replacement flow:

1. download archive to temp dir
2. verify SHA256
3. extract binary
4. chmod executable
5. copy to sibling temp path, e.g. `embed-log.new`
6. optionally retain `embed-log.old`
7. atomic rename into final path
8. report success

Notes:

- rename on the same filesystem is atomic
- replacing the running executable path is typically safe on Unix
- avoid editing shell profiles during update; that belongs to install only

### Windows

The running `embed-log.exe` cannot replace itself in place.

Recommended flow:

1. current process downloads archive and verifies checksum
2. extract new `embed-log.exe` into temp dir
3. spawn a small helper/updater process or script with:
   - current executable path
   - temp extracted exe path
   - current PID
4. current `embed-log.exe` exits
5. helper waits for PID to exit
6. helper moves existing exe to `embed-log.old.exe`
7. helper replaces with new exe
8. helper optionally restarts `embed-log.exe` or just reports completion

Recommended first version: replace and exit. Do not auto-restart.

The helper can be:

- a tiny Rust `embed-log-updater` helper binary, or
- a generated PowerShell script

Recommendation: start with a generated PowerShell helper to avoid shipping a second binary immediately.

---

## Safety and correctness checks

Before applying an update:

- current binary path must be known
- target triple must map to a known release asset
- release asset must exist
- checksum entry must exist in `SHA256SUMS`
- checksum must match exactly
- target version must be newer unless `--allow-downgrade`
- install location must be writable

Optional but recommended:

- create a local backup (`embed-log.old` / `embed-log.old.exe`)
- remove stale temp dirs on next successful run

Never:

- update config files automatically
- migrate user files implicitly without a versioned migration path
- replace the binary if verification fails

---

## UX details

### `embed-log update --check`

Human output example:

```text
current: 1.1.5
latest:  1.1.6
status:  update available
```

JSON output example:

```json
{
  "current": "1.1.5",
  "target": "1.1.6",
  "available": true,
  "prerelease": false
}
```

Exit code suggestion:

- `0` when command completed successfully, regardless of availability
- reserve non-zero for lookup/verification/update errors

### `embed-log update`

Human output example:

```text
current: 1.1.5
target:  1.1.6
asset:   embed-log-x86_64-unknown-linux-gnu.tar.gz
checking checksum...
installing update...
updated successfully
```

### Interactive confirmation

If stdout/stderr are terminals and `--yes` is not supplied:

```text
Install update 1.1.5 -> 1.1.6? [y/N]
```

In CI/non-interactive mode, either:

- require `--yes`, or
- auto-proceed when stdin is not a TTY

Recommendation: auto-proceed when non-interactive, prompt only in interactive terminals.

---

## Recommended implementation phases

## Phase 1 — stable self-update MVP

Scope:

- release-based install remains unchanged
- add `embed-log update`
- add `embed-log update --check`
- add `embed-log update --version <tag>`
- GitHub Releases only
- stable releases only by default
- Linux/macOS in-place replace
- Windows helper-based replace
- `embed-log version` expanded with build metadata and executable path

Acceptance:

- installed binary can update itself to a newer tagged release on all supported OSes
- checksum verification is mandatory
- wrong-platform artifact is rejected
- manifest/config/log data are untouched

## Phase 2 — polish and operator ergonomics

Scope:

- `--json`
- `--repo`
- `--prerelease`
- `--allow-downgrade`
- backup retention / cleanup policy
- richer version diagnostics

Acceptance:

- machine-readable status for CI/device labs
- fork/testing workflows supported without patching the binary

## Phase 3 — optional ecosystem expansion

Possible later work:

- uninstall scripts restored/documented cleanly
- package manager distribution (`brew`, `winget`, etc.)
- signed/notarized artifacts
- dedicated updater helper binary if PowerShell helper becomes limiting

---

## README / docs changes required when the rewrite lands

### Keep

- one-line install commands from release assets
- direct download section
- statement that release binaries embed frontend assets

### Remove or rewrite

- raw `main` branch installer URLs as the recommended path
- `embed-log update --sha <sha>`
- any source-based update expectations for normal users
- uninstall docs if uninstall support is not shipped

### New recommended top-level install story

1. Install latest release with one command.
2. Run `embed-log update` for upgrades.
3. Use direct archive downloads if preferred.
4. Use Cargo/source build only for development or unreleased changes.

---

## Suggested command examples for docs

### Install

```bash
curl -fsSL https://github.com/embedlab-tech/embed-log/releases/latest/download/install.sh | sh
```

```powershell
irm https://github.com/embedlab-tech/embed-log/releases/latest/download/install.ps1 | iex
```

### Update

```bash
embed-log update
embed-log update --check
embed-log update --version v1.1.6
```

### Version diagnostics

```bash
embed-log version
embed-log version --json
```

---

## Suggested implementation notes for the codebase

### CLI crate

Add:

- `Command::Update { ... }` in `crates/embed-log-cli/src/main.rs`
- `commands::update` module

Likely responsibilities:

- resolve executable path
- resolve runtime target triple
- fetch release metadata
- download asset/checksums
- verify hashes
- perform platform-specific replacement
- print human/JSON results

### Dependency choices

Reasonable additions if needed:

- HTTP client already in use, if present; otherwise a small blocking client is fine for CLI updater work
- `tempfile` for staging
- archive handling already available or minimal additions for `.tar.gz` / `.zip`
- avoid overbuilding async complexity for a leaf CLI command

Recommendation: keep updater implementation boring and mostly synchronous.

---

## Final recommendation

For the Rust rewrite, the clean install/update contract should be:

- **Install once** with release-hosted installer scripts.
- **Update later** with `embed-log update`.
- **Distribute only prebuilt release artifacts** to normal users.
- **Keep Tauri release/update separate** from the CLI/browser-mode distribution.

That preserves the convenience of the current product while moving the system onto safer, versioned, reproducible Rust release artifacts.
