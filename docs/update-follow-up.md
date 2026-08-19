# CLI updater follow-up checklist

The `embed-log update` implementation, release metadata generation, installer markers, Unix integration tests, and Windows-native test coverage are in the current branch. Complete the following before publishing the first updater-enabled release.

## 1. Run the release workflow dry run

Run the **Release CLI** GitHub Actions workflow manually from this branch:

- set **tag** to a non-production tag beginning with `v`, for example `v1.3.3-test`;
- leave **publish release** unchecked.

Expected result:

- Linux, macOS Apple Silicon, macOS Intel, and Windows build/test/package jobs succeed;
- the Windows updater integration test and `install.ps1` installer-marker smoke test run on a native Windows runner;
- the publish job is skipped;
- no GitHub Release is created.

If a platform fails, fix the failure and repeat the dry run before making a real release.

## 2. Inspect release metadata

From a successful dry run, inspect the generated `release.json` artifact. It must contain:

- the intended version without the leading `v`;
- an entry for each published target;
- the exact archive filename for each target;
- a 64-character SHA-256 checksum matching the corresponding archive.

## 3. Optional remaining test coverage

Add an integration test for the background update hint:

- an unreachable update endpoint must not delay `embed-log run` or print an update/network error;
- `EMBED_LOG_NO_UPDATE_CHECK=1` must prevent the request completely.

## 4. Prepare the first updater-enabled release

1. Choose the release version and update the workspace version in `Cargo.toml`.
2. Commit the version bump and create/push the matching `vX.Y.Z` tag.
3. Let the Release CLI workflow publish the release.
4. Install that release with the official installer on Linux/macOS and Windows.
5. Confirm `embed-log update --check` succeeds and `embed-log update` can install a later test release.

Existing installations do not contain the installer management marker, so users must install this first updater-enabled release once with the official installer. Later installer-managed installations can use `embed-log update`.

## 5. Future hardening

Consider signing `release.json` or the checksums with minisign or Sigstore, so authenticity is independently verifiable rather than relying only on GitHub HTTPS and release-asset access control.
