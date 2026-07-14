# Non-session roadmap

This roadmap intentionally excludes session import, export, retention, and browsing work. Complete the session backlog before starting these items.

## 1. Distribution and release trust

### 1.1 Homebrew tap

- Create and publish `krezolekcoder/homebrew-tap`.
- Add an `embed-log` formula that downloads versioned macOS release archives and pins SHA-256 values.
- Document `brew install krezolekcoder/tap/embed-log`.
- Automate formula bumps only after release CI is available.

### 1.2 Installer and artifact hardening

- Add release-artifact smoke tests for every published target.
- Support Linux ARM64 if it is a supported user platform.
- Add release provenance/signing when CI runners and secrets are available.
- Keep package-manager installs distinct from self-updated standalone installs.

### 1.3 Self-update completion

- Add a local mock HTTP end-to-end updater test.
- Detect package-managed/read-only executable locations with clear guidance.
- Implement a Windows replacement helper or explicitly keep Windows self-update unsupported.
- Consider signed checksum/provenance verification in addition to SHA256SUMS.

## 2. First-run developer experience

- Improve `doctor` serial diagnostics with platform-specific permission/udev guidance.
- Keep quick `embed-log run` focused on UART/file sources; move advanced source options to saved YAML.
- Publish polished one-UART, multi-UART, Zephyr, pytest/watcher, and UDP/CoAP recipes.
- Keep browser onboarding and quick-run behavior consistent with TUI behavior.

## 3. TUI/browser workflow parity

- Surface source reconnect/failure state clearly in the TUI.
- Show browser-only plugin capability notices in the TUI.
- Keep core workflows equivalent: view, filter, TX, markers, events, export, and session opening.
- Do not attempt to execute browser JavaScript plugins in the TUI.

## 4. Zephyr dictionary logging reliability

- Separate standard raw Zephyr dictionary-binary transport from the custom framed-HEX transport.
- Correct timestamp interpretation: do not infer Unix epoch seconds from a raw magnitude without an explicit timestamp source/unit.
- Add tracked real capture fixtures and transport-specific regression tests.
- Document supported Zephyr database versions and transport modes precisely.

## 5. Deferred product work

- Production Tauri packaging, signing, and notarization.
- IDE integrations.
- Cloud sync, accounts, and collaboration.
- Public APT repository; ship `.deb` artifacts and installer flow first.
