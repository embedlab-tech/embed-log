# Backend Refactor Plan

Refactoring the Python backend against the project's coding guidelines (Python 3.10+).
Every phase MUST leave all existing tests green. Run `python3 -m unittest discover -s tests -v` after each task.

---

## Ground Rules

- **No functionality changes.** Each task is a pure refactor or a type-safety improvement.
- **Existing tests must pass after every task.** If a task changes an interface, update the tests that exercise it in the same task.
- **Write new tests when adding data models or extracting logic.** Untested refactor = shipped bug.
- **One task per commit** when practical. Makes bisection trivial.
- **Update `features.md`** when a task adds or changes observable behavior.

---

## Phase 1 — Typed Data Models

Replace raw `dict`/`list` passing with `dataclass` models at system boundaries.
This is the foundation: every later phase benefits from typed contracts.

### 1.1 Config model

- [ ] Create `backend/config/models.py` with dataclasses:
  - `SourceConfig(name, type, port, baudrate?, parser, inject_port?, forward_ports?, label?)`
  - `TabConfig(label, panes: list[str], pane_labels: dict[str, str])`
  - `ServerConfig(host, ws_port, ws_ui?, app_name, open_browser, verbosity, job_id?, timestamp_mode, ...)`
  - `AppConfig(sources: list[SourceConfig], tabs: list[TabConfig], server: ServerConfig, injects, forwards, source_labels, log_dir, baudrate)`
- [ ] Refactor `load_config()` to return `AppConfig` instead of `dict`
- [ ] Update all callers of `load_config()` (cli.py `_run_run`, `_run_validate`, `_run_version`)
- [ ] Write unit tests for the new models (construction, defaults, validation)
- [ ] **Verify:** `python3 -m unittest discover -s tests -v`

### 1.2 Runtime data models

- [ ] Create `backend/core/models.py` with dataclasses:
  - `LogEntry(timestamp: datetime, source: str, message: str, color: str | None = None, no_ws: bool = False)` — use `@dataclass(slots=True)`
  - `SourceSpec(name, source: LogSource, log_file: str, inject_port: int | None, label: str, forward_ports: list[int])`
  - `TabSpec(label: str, panes: list[str])`
  - `SessionInfo(id, job_id, app_name, dir, started_at, timestamp_mode, ...)` — typed mirror of the current `_session_info` dict
  - `QueueStats(maxsize, depth, utilization_pct, enqueued, dequeued, peak_depth, near_full_events)`
- [ ] Replace `LogEntry` class in `runtime.py` with the dataclass from `models.py`
- [ ] Replace `sources: list[dict]` param in `LogServer.__init__` with `list[SourceSpec]`
- [ ] Replace `tabs: list` params everywhere with `list[TabSpec]`
- [ ] Update tests that construct `LogEntry` or pass source/tab dicts
- [ ] **Verify:** `python3 -m unittest discover -s tests -v`

### 1.3 Session data models

- [ ] Create `backend/session/models.py` with dataclasses:
  - `SessionStats(alias, lines, size_kb, time_start, time_end, duration_secs, markers)`
  - `SessionSummary(manifest: dict, stats: SessionStats, dir: str)` — replaces the underscore-key mutation pattern in `_iter_sessions`
  - `SnippetEntry(file, label, scope, panes, line_count, saved_at)`
- [ ] Refactor `_iter_sessions()` and `_session_stats()` in `cli.py` to return `list[SessionSummary]`
- [ ] Refactor `_format_session_row()` to accept `SessionSummary`
- [ ] Update session-related tests
- [ ] **Verify:** `python3 -m unittest discover -s tests -v`

---

## Phase 2 — Split `cli.py`

The goal is a package `backend/cli/` with one submodule per embed-log CLI feature area.
The public API (`main()` → `int`) stays identical. Each submodule gets its own test file.

### Existing CLI test coverage

| Test file | Covers | Gaps |
|-----------|--------|------|
| `test_cli_create_config.py` | `_run_create_config`, `_detected_serial_ports` | — |
| `test_cli_version.py` | `_run_version` | — |
| `test_cli_update.py` | `_run_update` | — |
| `test_cli_markers.py` | `_run_sessions_marker` (list, show) | — |
| `test_cli_snippet.py` | `_run_sessions_snippet` (list, show, delete) | — |
| `test_cli_sessions_export.py` | `_run_sessions_export`, `_handle_first_log_at` | raw format, --missing batch, --first/--last time filters |
| `test_cli_run_timestamp_mode.py` | CLI flag → `run_app` precedence | other flags (host, ws-port, etc.) |
| `test_sessions.py` | `_iter_sessions`, `_resolve_session_id` | `_session_stats`, `_format_session_row` |
| `test_config_loader.py` | `load_config` validation | — |
| `test_merge_logs.py` | merge script invocation | — |
| (none) | `_run_sessions_list`, `_run_sessions_info`, `_run_sessions_logs`, `_run_sessions_delete`, `_run_sessions_open` | **all untested** |
| (none) | `_parse_duration`, `_format_duration`, `_parse_log_timestamp`, `_ms3` | **all untested** |
| (none) | `_run_validate` standalone | only via `test_config_loader` |
| (none) | `_run_run` end-to-end | only timestamp override tested |
| (none) | `_build_parser` structure | **untested** |

### 2.1 Migration strategy

**Problem:** Python can't have `backend/cli.py` (module) and `backend/cli/` (package) at the same time.
**Solution:** Extract into sibling modules (`backend/cli_*.py`) during incremental migration. At the final cutover (2.15), rename `backend/cli.py` → `backend/cli/__init__.py` and move all `backend/cli_*.py` into the package.

- [x] Create `backend/cli_util.py` — shared pure helpers (done)
- [x] Write `tests/test_cli_util.py` — 49 tests (done)
- [x] Verify all existing tests pass (182 total) (done)

### 2.2 Extract `backend/cli_dispatch.py` — main dispatcher

- [ ] Move `main()` and the top-level `if`/`elif` dispatch chain → `backend/cli_dispatch.py`
- [ ] In original `backend/cli.py`, replace `main()` body with: `from backend.cli_dispatch import main`
- [ ] **Verify:** `python3 -m unittest discover -s tests -v`


### 2.3 Extract `backend/cli/sessions/list.py` — `sessions list`

- [ ] Move `_run_sessions_list`, `_iter_sessions`, `_session_stats`, `_format_session_row`
- [ ] Move `sessions list` subparser registration to `parser.py` (or keep inline, decide during extraction)
- [ ] Write `tests/test_cli_sessions_list.py`:
  - [ ] Empty log directory → "No sessions found"
  - [ ] Multiple sessions → correct row count
  - [ ] `--json` output is valid JSON
  - [ ] `--sort name` reorders correctly
  - [ ] `--limit N` truncates
- [ ] **Verify:** `python3 -m unittest discover -s tests -v`

### 2.4 Extract `backend/cli/sessions/info.py` — `sessions info`

- [ ] Move `_run_sessions_info`
- [ ] Write `tests/test_cli_sessions_info.py`:
  - [ ] Known session → prints expected fields (session, alias, app, started, sources, tabs)
  - [ ] `--json` output is valid JSON matching manifest
  - [ ] Unknown session → exit 1, stderr message
  - [ ] Missing manifest → exit 1, stderr message
- [ ] **Verify:** `python3 -m unittest discover -s tests -v`

### 2.5 Extract `backend/cli/sessions/logs.py` — `sessions logs`

- [ ] Move `_run_sessions_logs`
- [ ] Write `tests/test_cli_sessions_logs.py`:
  - [ ] Prints log file contents to stdout
  - [ ] `--pane` filters to matching source
  - [ ] Unknown pane → exit 1
  - [ ] Unknown session → exit 1
- [ ] **Verify:** `python3 -m unittest discover -s tests -v`

### 2.6 Extract `backend/cli/sessions/export.py` — `sessions export`

- [ ] Move `_run_sessions_export` (already partially tested)
- [ ] Add missing tests to `tests/test_cli_sessions_export.py`:
  - [ ] Raw format: multi-pane merge with pane prefix
  - [ ] Raw format: `--after` / `--before` with ISO and duration
  - [ ] Raw format: `--first` / `--last` time windowing
  - [ ] Raw format: `--pane` filter
  - [ ] Raw format: `--first` + `--last` → error
  - [ ] `--missing` batch mode: exports only sessions without HTML
  - [ ] HTML format: manifest updated with `html_status=ready`
  - [ ] Unknown session → exit 1
  - [ ] Missing manifest → exit 1
- [ ] **Verify:** `python3 -m unittest discover -s tests -v`

### 2.7 Extract `backend/cli/sessions/delete.py` — `sessions delete`

- [ ] Move `_run_sessions_delete`
- [ ] Write `tests/test_cli_sessions_delete.py`:
  - [ ] Delete by session ID with `--yes`
  - [ ] Delete by `--older-than 7d` with `--yes`
  - [ ] Delete `--all` with `--yes`
  - [ ] Unknown session → exit 1
  - [ ] Conflicting modes (ID + `--all`) → exit 1
  - [ ] No matching sessions → "No sessions match"
  - [ ] Invalid duration → exit 1
- [ ] **Verify:** `python3 -m unittest discover -s tests -v`

### 2.8 Extract `backend/cli/sessions/open.py` — `sessions open`

- [ ] Move `_run_sessions_open`
- [ ] Write `tests/test_cli_sessions_open.py`:
  - [ ] Known session with HTML → calls `webbrowser.open` with file URI
  - [ ] `marker N` argument → URI includes `#marker-N`
  - [ ] No HTML file → exit 1, suggest export
  - [ ] Invalid marker index → exit 1
  - [ ] Unknown session → exit 1
- [ ] **Verify:** `python3 -m unittest discover -s tests -v`

### 2.9 Extract `backend/cli/sessions/marker.py` — `sessions marker`

- [ ] Move `_run_sessions_marker` (already tested in `test_cli_markers.py`)
- [ ] Update import in `test_cli_markers.py` from `backend.cli` → `backend.cli.sessions.marker`
- [ ] **Verify:** `python3 -m unittest discover -s tests -v`

### 2.10 Extract `backend/cli/sessions/snippet.py` — `sessions snippet`

- [ ] Move `_run_sessions_snippet` (already tested in `test_cli_snippet.py`)
- [ ] Update import in `test_cli_snippet.py` from `backend.cli` → `backend.cli.sessions.snippet`
- [ ] **Verify:** `python3 -m unittest discover -s tests -v`

### 2.11 Extract `backend/cli/wizard.py` — `create-config`

- [ ] Move `_run_create_config`, `_default_init_yaml`, `_slug_name`, `_prompt`, `_prompt_yes_no`, `_prompt_int`, `_choose_uart_port`, `_detected_serial_ports`, `_build_wizard_yaml`
- [ ] Update import in `test_cli_create_config.py`
- [ ] **Verify:** `python3 -m unittest discover -s tests -v`

### 2.12 Extract `backend/cli/diagnostics.py` — `version` / `ports`

- [ ] Move `_run_version`, `_run_ports`, `_display_version_line`, `_display_source_label`, `_display_source_status`, `_load_install_identity`
- [ ] Update import in `test_cli_version.py`
- [ ] **Verify:** `python3 -m unittest discover -s tests -v`

### 2.13 Extract `backend/cli/update.py` — `update`

- [ ] Move `_run_update` and all its nested helpers
- [ ] Update import in `test_cli_update.py`
- [ ] **Verify:** `python3 -m unittest discover -s tests -v`

### 2.14 Extract `backend/cli/run.py` — `run` / `validate` / `merge`

- [ ] Move `_run_run`, `_run_validate`, `_run_merge`
- [ ] Write `tests/test_cli_run.py`:
  - [ ] `run` with `--config` → sources constructed from config
  - [ ] `run` with `--source` inline → sources constructed from CLI
  - [ ] `run` with duplicate source names → exit 1
  - [ ] `run` with no sources → exit 1
  - [ ] `validate` valid config → exit 0
  - [ ] `validate` invalid config → exit 2
  - [ ] `validate --json` → valid JSON
- [ ] **Verify:** `python3 -m unittest discover -s tests -v`

### 2.15 Finalize: remove old `backend/cli.py`

- [ ] Ensure `backend/cli/__init__.py` exports `main` correctly
- [ ] Ensure `backend/server.py` entrypoint works with new package
- [ ] Remove old monolithic `backend/cli.py`
- [ ] Update all remaining test imports to new module paths
- [ ] **Verify:** `python3 -m unittest discover -s tests -v` **and** `cd tests-ui && npm test`

### Target file layout after Phase 2

```
backend/cli/
  __init__.py              # re-exports main()
  dispatch.py              # main() dispatcher
  parser.py                # _build_parser()
  util.py                  # shared pure helpers (timestamps, durations, file stats)
  sessions/
    __init__.py            # _run_sessions() dispatcher
    list.py                # sessions list
    info.py                # sessions info
    logs.py                # sessions logs
    export.py              # sessions export
    delete.py              # sessions delete
    open.py                # sessions open
    marker.py              # sessions marker list/show
    snippet.py             # sessions snippet list/show/delete
  wizard.py                # create-config interactive wizard
  diagnostics.py           # version / doctor / ports
  update.py                # self-update
  run.py                   # run / validate / merge

tests/
  test_cli_parser.py             # NEW — parser structure + defaults
  test_cli_util.py               # NEW — pure helpers
  test_cli_sessions_list.py      # NEW — sessions list
  test_cli_sessions_info.py      # NEW — sessions info
  test_cli_sessions_logs.py      # NEW — sessions logs
  test_cli_sessions_export.py    # EXTEND — add raw format, time filters, --missing
  test_cli_sessions_delete.py    # NEW — sessions delete
  test_cli_sessions_open.py      # NEW — sessions open
  test_cli_sessions_marker.py    # RENAME from test_cli_markers.py, update imports
  test_cli_sessions_snippet.py   # RENAME from test_cli_snippet.py, update imports
  test_cli_create_config.py      # UPDATE imports only
  test_cli_version.py            # UPDATE imports only
  test_cli_update.py             # UPDATE imports only
  test_cli_run.py                # NEW — run / validate / merge
```

---

## Phase 3 — Split `runtime.py`

### 3.1 Extract data structures

- [ ] Move `TrackedQueue` → `backend/core/queue.py`
- [ ] Move `SessionClock` → `backend/core/clock.py`
- [ ] Move `ANSI` dict → `backend/core/ansi.py` (or a constants module)
- [ ] Keep `SourceManager` and `LogServer` in `backend/core/runtime.py`
- [ ] Update imports in `runtime.py` and tests
- [ ] **Verify:** `python3 -m unittest discover -s tests -v`

### 3.2 Remove dead wrapper

- [ ] Remove `_slug()` wrapper in `runtime.py` — call `slugify()` directly
- [ ] **Verify:** `python3 -m unittest discover -s tests -v`

---

## Phase 4 — Error Handling

### 4.1 Eliminate silent `except Exception: pass`

- [ ] `ws_server.py:370-371` — log the exception at `debug` level: `logging.debug("WS command error: %s", exc)`
- [ ] `ws_server.py:260` — log manifest parse failures: `logging.debug("manifest parse error for %s: %s", session_id, exc)`
- [ ] `runtime.py:846` — narrow to `except OSError` or log at `warning`
- [ ] Audit all other `except Exception` blocks; add logging or narrow the catch
- [ ] **Verify:** `python3 -m unittest discover -s tests -v`

### 4.2 Remove inline imports that shadow module-level

- [ ] `cli.py:765` — remove `import re as _re` (use module-level `re`)
- [ ] `cli.py:1083` and `cli.py:832` — remove `import datetime as _dt` (use module-level `datetime`)
- [ ] `cli.py:1480` — remove inline `import shutil` (already at module level)
- [ ] `cli.py:1427` — remove inline `import json as _json` (already at module level)
- [ ] `cli.py:1281` — remove inline `from datetime import datetime as _dt2`
- [ ] Search for any remaining inline imports that shadow top-level
- [ ] **Verify:** `python3 -m unittest discover -s tests -v`

---

## Phase 5 — Thread Safety

### 5.1 Protect `_session_info`

- [ ] Add a `threading.Lock` to `LogServer` for `_session_info` access
- [ ] Wrap all `_session_info` reads/writes in the lock
- [ ] Or: convert `_session_info` to a `SessionInfo` dataclass with a lock-guarded update method
- [ ] **Verify:** `python3 -m unittest discover -s tests -v`

### 5.2 Fix O(n²) dead client cleanup

- [ ] In `SourceManager._stream_to_clients` and `_forward_to_clients`, replace `list.remove()` loop with list comprehension rebuild:
  ```python
  self._stream_clients = [c for c in self._stream_clients if c not in dead]
  ```
- [ ] **Verify:** `python3 -m unittest discover -s tests -v`

---

## Phase 6 — Testability

### 6.1 Inject clock into `SourceManager`

- [ ] Replace all `datetime.now().astimezone()` calls in `SourceManager` with `self._clock.now()` where `_clock` is a callable (default: `datetime.now().astimezone`)
- [ ] Update `SessionClock` to provide a `now()` method
- [ ] Write tests that use a fixed clock for deterministic assertions
- [ ] **Verify:** `python3 -m unittest discover -s tests -v`

### 6.2 Unify manifest reading

- [ ] Create a single `read_manifest(path: Path) -> dict | None` function in `backend/session/`
- [ ] Replace `cli.py:_read_manifest` and `SessionManager._read_manifest` with this shared function
- [ ] **Verify:** `python3 -m unittest discover -s tests -v`

### 6.3 Extract `resolve_run_args` helper

- [ ] Extract the 12-line CLI-vs-config precedence chain in `_run_run` into `resolve_run_args(args, cfg) -> AppConfig`
- [ ] This function becomes unit-testable without argparse
- [ ] Write tests for precedence: CLI overrides config, config overrides defaults
- [ ] **Verify:** `python3 -m unittest discover -s tests -v`

---

## Phase 7 — Performance (Low Risk)

### 7.1 Cache line counts in manifest

- [ ] When `SessionManager.write_manifest()` runs, compute and store line counts per source file
- [ ] `_session_stats` reads from manifest first, only recomputes if missing
- [ ] **Verify:** `python3 -m unittest discover -s tests -v`

### 7.2 Throttle queue stats check

- [ ] In `SourceManager._writer_loop`, check queue stats every 50th iteration instead of every iteration when congested
- [ ] **Verify:** `python3 -m unittest discover -s tests -v`

---

## Phase 8 — Modernize Python Idioms

### 8.1 `Optional[X]` → `X | None`

- [ ] Search-and-replace across all backend files (safe because `from __future__ import annotations` is universal)
- [ ] Remove `from typing import Optional` where no longer needed
- [ ] **Verify:** `python3 -m unittest discover -s tests -v`

### 8.2 `match/case` for dispatch

- [ ] Replace `if args.command == "X"` chains in `main()` and `_run_sessions()` with `match args.command`
- [ ] Only do this after Phase 2 (cli split) so you're editing the right file
- [ ] **Verify:** `python3 -m unittest discover -s tests -v`

---

## Progress / Changelog

| Date | Phase | Task | Status | Notes |
|------|-------|------|--------|-------|
| 2026-05-30 | 2.1 | Migration strategy + cli_util.py | DONE | `backend/cli_util.py` (152 lines) + 49 tests |
| 2026-05-30 | 2.2 | cli_dispatch.py | DONE | Deferred import pattern for circular dep |
| 2026-05-30 | 2.3 | cli_parser.py | DONE | 40 parser tests |
| 2026-05-30 | 2.4–2.11 | cli_sessions.py | DONE | All sessions subcommands extracted |
| 2026-05-30 | 2.12 | cli_wizard.py | DONE | create-config wizard + helpers |
| 2026-05-30 | 2.13 | cli_diagnostics.py | DONE | version/ports/display helpers |
| 2026-05-30 | 2.14 | cli_update.py | DONE | self-update logic |
| 2026-05-30 | 2.15 | cli_run.py | DONE | run/validate/merge |
| 2026-05-30 | 2.16 | Final cutover to cli/ package | DONE | `backend/cli/` with `sessions/` subdir, 222 tests pass |
| 2026-05-30 | 1.1 | Config models | DONE | `backend/config/models.py` — AppConfig, SourceConfig, TabConfig, ServerConfig |
| 2026-05-30 | 1.2 | Runtime models | DONE | `backend/core/models.py` — LogEntry(dataclass), QueueStats |
| 2026-05-30 | 1.3 | Session models | DONE | `backend/session/models.py` — SessionStats, SnippetEntry |
| 2026-05-30 | 3.1 | Split runtime.py | DONE | queue.py (81), clock.py (75), ansi.py (11). runtime.py 859→702 lines |
| 2026-05-30 | 3.2 | Remove _slug wrapper | DONE | Replaced with direct slugify() calls |
| 2026-05-30 | 4.1 | Narrow broad excepts | DONE | 4 blocks in ws_server.py + log_client.py now log at debug |
| 2026-05-30 | 4.2 | Remove inline imports | DONE | cli/util.py, cli/sessions/export.py cleaned up |
| 2026-05-30 | 5.1 | Protect _session_info | DONE | threading.Lock + _update_session_info() helper in LogServer |
| 2026-05-30 | 5.2 | Fix O(n²) dead client cleanup | DONE | list.remove() loop → list comprehension rebuild |
| 2026-05-30 | 6.1 | Inject clock into SourceManager | DONE | `clock` param in __init__, 7 `datetime.now()` calls replaced, 7 new tests |
| 2026-05-30 | 6.2 | Unify manifest reading | SKIPPED | Two functions serve different contexts; coupling worse than duplication |
| 2026-05-30 | 8.1 | `Optional[X]` → `X \| None` | DONE | 12 backend files updated, `from __future__ import annotations` already present |
| 2026-05-30 | 8.2 | `match/case` dispatch | DONE | dispatch.py + sessions/__init__.py |

_Update this table as tasks are completed. Format: `YYYY-MM-DD | 1.1 | Config model | DONE | PR #123`_

---

## Dependency Graph

```
Phase 1 (models) ──→ Phase 2 (split cli) ──→ Phase 8.2 (match/case)
                ──→ Phase 3 (split runtime)
                ──→ Phase 4 (error handling)  [independent of 2/3]
                ──→ Phase 5 (thread safety)   [independent of 2/3]
                ──→ Phase 6 (testability)     [needs 1.2 for clock injection]
                ──→ Phase 7 (performance)     [independent of 2/3]
                ──→ Phase 8.1 (Optional)      [independent, do anytime]
```

Phase 1 must go first. Phases 4, 5, 7, 8.1 are independent and can be parallelized.
Phase 2 and 3 can be parallelized after Phase 1.
Phase 6.1 depends on 1.2 (needs `LogEntry` dataclass).
Phase 8.2 depends on 2 (needs cli split to know which file to edit).
Phase 2 tasks 2.1→2.15 are sequential (each builds on the previous).
Within Phase 2, tasks 2.9–2.13 (marker, snippet, wizard, diagnostics, update) are independent of each other and can be parallelized once 2.1–2.2 are done.
