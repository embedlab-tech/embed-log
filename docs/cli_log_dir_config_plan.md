# CLI log directory and config path simplification plan

## Goal

Make `embed-log` CLI behavior predictable when users:

- store sessions in a directory not named `logs/`,
- want session-management commands to operate on that directory,
- want `embed-log run` to use a config path from an environment variable,
- need plain date-named log directories to remain readable without extra metadata.

## Desired UX

```bash
export EMBED_LOG_CONFIG_YML_PATH=/opt/project/embed-log.yml
embed-log run

embed-log run --config ./local-debug.yml   # explicit CLI config overrides env var

embed-log sessions list --dir /mnt/ci/artifacts/device-logs
embed-log sessions logs 2026-06-04_12-30-00 --dir /mnt/ci/artifacts/device-logs
embed-log sessions export 2026-06-04_12-30-00 --dir /mnt/ci/artifacts/device-logs
```

## Decisions

### 1. Session CLI gets explicit log-root selection

Add `--dir DIR` to every `embed-log sessions ...` command that reads or writes session artifacts.

Keep the existing `--log-dir DIR` as a backward-compatible alias, but prefer `--dir` in help and docs because session commands are selecting an existing session root, not configuring runtime logging.

Commands covered:

- `embed-log sessions list --dir DIR`
- `embed-log sessions info SESSION --dir DIR`
- `embed-log sessions logs SESSION --dir DIR`
- `embed-log sessions export SESSION --dir DIR`
- `embed-log sessions open SESSION --dir DIR`
- `embed-log sessions delete SESSION --dir DIR`
- `embed-log sessions marker list SESSION --dir DIR`
- `embed-log sessions marker show SESSION N --dir DIR`
- `embed-log sessions snippet list SESSION --dir DIR`
- `embed-log sessions snippet show SESSION --dir DIR`
- `embed-log sessions snippet delete SESSION --dir DIR`

### 2. Runtime config path can come from env var

Add `EMBED_LOG_CONFIG_YML_PATH` as a config fallback for config-aware CLI flows.

Precedence for `embed-log run`:

1. `embed-log run --config path.yml`
2. `EMBED_LOG_CONFIG_YML_PATH=/path/config.yml embed-log run`
3. no config; keep existing inline flags/default behavior

If the env var is set but points to a missing or unreadable file, fail loudly with a clear config error. Silent fallback would be dangerous because the user intended a specific config.

Explicit `--config` always wins over the env var.

### 3. No required log-root metadata

A log root must not need a metadata/index file to be usable.

Session commands should work when the selected root contains plain child directories named by date/session id, as long as each child directory contains recognizable session content.

A child directory is a valid session if it contains at least one of:

- `manifest.json`,
- `session.html`,
- `markers.json`,
- `snippets/`,
- any `*.log` or `*.txt` file.

Metadata or an index file may be added later as an optimization, but must never be required to read existing logs.

## Implementation plan

### 1. Centralize config path resolution

Add a helper, likely in `backend/cli/util.py` or a new `backend/cli/config_resolution.py`:

```py
ENV_CONFIG_PATH = "EMBED_LOG_CONFIG_YML_PATH"

def resolve_config_path(cli_path: str | None) -> Path | None:
    if cli_path:
        return Path(cli_path)
    env_path = os.environ.get(ENV_CONFIG_PATH)
    if env_path and env_path.strip():
        return Path(env_path.strip())
    return None
```

Use this helper in:

- `backend/cli/run.py::_run_run`,
- `backend/cli/diagnostics.py::_run_version`, if version/diagnostics should report the same effective config,
- top-level no-args guidance in `backend/cli/dispatch.py`, so it can show that `embed-log run` will use the env config.

`_run_run` should pass the resolved config path to `run_app(config_path=...)` so manifests record the actual config used.

### 2. Update run help and error text

Update `backend/cli/parser.py` examples for `run`:

```text
embed-log run --config embed-log.yml
EMBED_LOG_CONFIG_YML_PATH=/path/embed-log.yml embed-log run
embed-log run --config other.yml   # overrides env var
```

Update no-source error in `backend/cli/run.py` to mention the env var:

```text
no sources configured. Use --config FILE, EMBED_LOG_CONFIG_YML_PATH, embed-log sample-config, or --source ...
```

### 3. Add `--dir` alias to sessions parser

In `backend/cli/sessions/__init__.py`, change the shared argument to:

```py
shared.add_argument(
    "--dir",
    "--log-dir",
    dest="log_dir",
    default=None,
    help="session log root directory (default: logs/)",
)
```

Then resolve once after parsing:

```py
log_dir = Path(args.log_dir or "logs/")
```

Prefer documenting this placement:

```bash
embed-log sessions list --dir /path/to/session-root
```

Supporting `embed-log sessions --dir /path list` would be nice, but is optional. If added, it should be implemented intentionally with argparse parent placement tests.

### 4. Harden session discovery

In `backend/cli/util.py`, add:

```py
def is_session_dir(path: Path) -> bool:
    if (path / "manifest.json").is_file():
        return True
    if (path / "session.html").is_file():
        return True
    if (path / "markers.json").is_file():
        return True
    if (path / "snippets").is_dir():
        return True
    if any(path.glob("*.log")) or any(path.glob("*.txt")):
        return True
    return False
```

Then make `iter_sessions(log_dir)` skip unrelated child directories.

For manifest-less sessions:

- `session_id` defaults to the directory name,
- alias derives from the directory name,
- stats derive from `*.log` / `*.txt`,
- time range derives from line timestamps where available,
- `session_html` should be detected from `session.html` even if no manifest exists.

### 5. Improve empty-state diagnostics

For non-JSON `sessions list`, if no sessions are found:

```text
No sessions found in /resolved/path
Hint: pass --dir PATH, or run embed-log run --log-dir PATH
```

Do not change JSON output shape unless explicitly versioned or gated by a new option; existing machine users may rely on the current list shape.

## Tests

### Config env var

Add tests for:

- `embed-log run` uses `EMBED_LOG_CONFIG_YML_PATH` when `--config` is absent,
- `--config` overrides `EMBED_LOG_CONFIG_YML_PATH`,
- env var pointing to a missing file returns a clear config error,
- manifest/runtime receives the resolved config path where practical.

### Session directory selection

Use temp directories and test:

- `sessions list --dir custom-root` finds manifest-backed sessions,
- `sessions list --dir custom-root` finds manifest-less date/session dirs with `*.log`,
- `sessions info SESSION --dir custom-root` works without manifest,
- `sessions logs SESSION --dir custom-root` prints log contents,
- `sessions export SESSION --dir custom-root` reads from that root,
- existing `--log-dir` still works.

### Session discovery filtering

Test that:

- unrelated child directories are ignored,
- a directory with only `session.html` is listed,
- a directory with only `markers.json` is listed,
- a directory with only `snippets/` is listed,
- a plain date-named directory with `*.log` is listed.

## Ambiguous CLI behavior found

### 1. `--log-dir` means different things

- `embed-log run --log-dir DIR` means “write logs here”.
- `embed-log sessions ... --log-dir DIR` means “read/manage sessions from here”.

Simplification:

- keep `run --log-dir`,
- introduce and document `sessions ... --dir`,
- keep `sessions ... --log-dir` as compatibility alias.

### 2. Docs mention commands not in the current parser

Docs currently mention commands like `embed-log validate` and `embed-log create-config`, while the parser exposes `sample-config` and does not expose those commands.

Simplification options:

1. Remove or replace stale docs with current commands.
2. Add compatibility aliases only if those workflows are intentionally supported.

Do not leave docs advertising missing commands.

### 3. Config path behavior is inconsistent

- `run` currently only uses `--config`.
- `version` currently uses `--config` or implicit `embed-log.yml`.
- no command honors the proposed env var yet.

Simplification:

Use one config resolver with this precedence:

```text
explicit CLI path > EMBED_LOG_CONFIG_YML_PATH > command-specific default
```

### 4. Session flag placement may surprise users

Current sessions parsing is subcommand-local, so the safe documented form is:

```bash
embed-log sessions list --dir DIR
```

Users may try:

```bash
embed-log sessions --dir DIR list
```

Simplification:

Either support both forms deliberately, or document the supported placement clearly in help examples.

### 5. Manifest-less session directories are underdefined

Current code partially tolerates missing manifests but docs imply `logs/<session_id>/manifest.json` as the primary contract.

Simplification:

Document the actual contract:

- manifest improves metadata and export fidelity,
- plain directories with logs are valid sessions,
- no log-root index/metadata is required.

### 6. Top-level no-args guidance should account for env config

If `EMBED_LOG_CONFIG_YML_PATH` is set, running `embed-log` without args should suggest:

```bash
embed-log run
```

and show which config file will be used.
