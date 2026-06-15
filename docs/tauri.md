# Tauri desktop app behavior

The Tauri app is a desktop shell around the same `embed-log-core::LogServer` used by the CLI.

## Config path resolution

At startup the Tauri app resolves the config path in this order:

```text
1. CLI flag:             --config <path> or -c <path>
2. Environment variable: EMBED_LOG_CONFIG_YML_PATH
3. Local file:           ./embed-log.yml, if it exists in the app's current working directory
4. App config default:   <tauri app_config_dir>/embed-log.yml
```

The implementation is in `crates/embed-log-tauri/src/lib.rs`:

```text
resolve_config_path()
  └─ resolve_config_path_from(args, EMBED_LOG_CONFIG_YML_PATH, app_default_config_path)
```

The app config default is:

```text
app.path().app_config_dir()/embed-log.yml
```

That directory is platform-specific and is chosen by Tauri. Typical examples look like:

```text
macOS:   ~/Library/Application Support/<app-id>/embed-log.yml
Linux:   ~/.config/<app-id>/embed-log.yml
Windows: %APPDATA%\<app-id>\embed-log.yml
```

The exact `<app-id>` comes from the Tauri app identifier/config.

## What happens on first run

If the resolved config path does not exist or cannot be loaded:

- if the path exists but is invalid, the app shows a config error page
- if the path does not exist, the app shows the onboarding page

Onboarding lets the user create a quick config and choose the session logs directory. The default logs directory shown by onboarding is `logs/`. When the user clicks **Start logging**:

```text
onboarding.js
  └─ POST /api/save_config  (or save_quick_config Tauri command in eval fallback)
      └─ embed_log_core::onboarding::save_quick_config
          ├─ writes YAML to the already-resolved config path
          ├─ loads/validates the generated config
          └─ returns the local viewer URL
```

> **Shared with the CLI.** The Tauri app and the CLI run the exact same onboarding page (`frontend/onboarding.js`) and the same `embed_log_core::onboarding::OnboardingServer`. The Tauri app additionally starts its `LogServer` inside the save handler; the CLI writes the config then starts its server after `wait_for_save()`. See [cli.md → Onboarding](cli.md#onboarding).

So the Tauri app knows the config location because it resolves it before showing onboarding and stores it in process state (`CONFIG_PATH` static / `GET /api/server_status`). Onboarding saves to that same path.

## Default session log location after onboarding

The onboarding draft defaults to:

```yaml
logs:
  dir: logs/
```

The runtime resolves relative `logs.dir` values relative to the config file directory.

Therefore, if onboarding saves config to:

```text
<tauri app_config_dir>/embed-log.yml
```

then sessions are saved under:

```text
<tauri app_config_dir>/logs/
```

A session directory looks like:

```text
<tauri app_config_dir>/logs/
└── 2026-06-14_09-30-00/
    ├── manifest.json
    ├── session.html
    ├── markers.json
    ├── snippets/
    └── <tab>__<source>__<session>.log
```

If `logs.dir` is absolute, that absolute path is used as-is.

## Using a separate config

Users can run the Tauri app with a separate config in three ways. If the selected config path does not exist, onboarding writes the new config to that selected path instead of the app config default.

### 1. CLI flag

```bash
embed-log-tauri --config /path/to/lab-a.yml
```

or via the CLI launcher:

```bash
embed-log --ui --config /path/to/lab-a.yml
```

### 2. Environment variable

```bash
EMBED_LOG_CONFIG_YML_PATH=/path/to/lab-a.yml embed-log-tauri
```

### 3. Local `embed-log.yml`

If the app starts with a current working directory that contains `embed-log.yml`, that local file wins over the app config default.

## Important path rule

For both CLI and Tauri:

```text
relative logs.dir is resolved relative to the config file directory
```

Examples:

```text
config: /Users/me/lab-a/embed-log.yml
logs.dir: logs/
result: /Users/me/lab-a/logs/
```

```text
config: /Users/me/lab-b.yml
logs.dir: /tmp/embed-log-runs
result: /tmp/embed-log-runs
```

This makes separate configs naturally keep separate logs when each config lives in its own directory.
