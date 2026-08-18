# CLI update feature plan

## 1. Define update policy

- Official GitHub Releases are the update source.
- Only installer-managed binaries may self-update.
- Normal CLI behavior remains fully offline-capable.
- Background checks are best-effort and silent on every network failure.
- `EMBED_LOG_NO_UPDATE_CHECK=1` disables background checks.

## 2. Publish release metadata

- Extend `.github/workflows/release-cli.yml` to generate and publish `release.json`.
- Include the release version, target-to-archive mapping, and SHA-256 hashes.
- Keep publishing `SHA256SUMS` for direct installer compatibility.

## 3. Add an update domain module

Create a module responsible for:

- target detection;
- semantic-version comparison;
- cache and state locations per operating system;
- release metadata retrieval and validation;
- cache read/write;
- install-management marker read/write.

## 4. Mark installer-managed installations

- Update `install.sh` and `install.ps1`.
- After a successful installation, save a small marker/state file recording:
  - executable path and installation directory;
  - repository/channel;
  - target;
  - installer-managed status.
- Existing unmarked installations remain usable but cannot self-update automatically.

## 5. Implement `embed-log update --check`

- Fetch current release metadata with bounded network timeouts.
- Compare the installed and available semantic versions.
- Report whether the installation is up to date or an update is available.
- Do not download or modify files.
- An explicit check reports a readable error when offline.

## 6. Implement `embed-log update`

- Verify the installation marker and executable location.
- Download the archive for the current target and its checksum metadata.
- Verify SHA-256 before extracting the archive.
- Replace safely:
  - Unix: write a temporary sibling file, then atomically rename it into place.
  - Windows: launch a temporary updater process that replaces the executable after the CLI exits.
- Never modify the installed binary until download, extraction, and verification all succeed.

## 7. Add unobtrusive update hints

- On suitable interactive, human-facing commands, schedule a best-effort check.
- Cache successful results for a limited period, initially 24 hours.
- Print a short stderr hint only when a newer version is freshly confirmed.
- Never check or print a hint for JSON output, help/version, `update`, daemon children, or when disabled.
- Print nothing when offline, on timeout, when cache access fails, or when metadata is invalid.

## 8. Test the feature

- Unit tests for semantic versions, target mapping, cache expiry, marker validation, and release metadata validation.
- Local HTTP fixture tests for update availability, checksum failures, download failures, malformed metadata, and offline behavior.
- Installer tests for marker creation and custom installation directories.
- Platform tests for Unix replacement and deferred Windows replacement.
- Regression tests proving normal commands work without network access and JSON output contains no update noise.

## 9. Document it

- Update `docs/releasing.md` with metadata/signing and release requirements.
- Update `docs/cli.md` with `update`, `--check`, offline behavior, and the opt-out environment variable.
- Update README installation and update instructions.

## 10. Future hardening

Possible follow-up work:

- Sign `release.json` or checksums with minisign or Sigstore.
- Add package-manager-specific guidance/detection for `.deb`, Homebrew, and similar installations.
- Consider explicit update channels such as stable and prerelease.
