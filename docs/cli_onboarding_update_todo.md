# CLI onboarding and update TODO

## P0 — CLI onboarding for humans and agents

- [x] Add a quick onboarding command, for example:

  ```bash
  embed-log onboard
  ```

  It should print a short, practical orientation:
  - active config, or clearly state that no config is active
  - where example configs are available
  - how to generate a starter config
  - how to start the UI
  - where sessions/logs are saved
  - most important commands: `ports`, `doctor`, `init`, `onboard`, `run`, `sessions list`

- [x] Add a machine-readable mode for agents:

  ```bash
  embed-log onboard --json
  ```

  The output should be stable and include:
  - `version`
  - `install_source`
  - `active_config`
  - `available_samples`
  - `commands`
  - `docs`
  - `next_steps`

- [x] Extend or reorganize `embed-log doctor` so it clearly reports:
  - which config is active
  - config source: explicit `--config`, `EMBED_LOG_CONFIG_YML_PATH`, or local `embed-log.yml`
  - whether the config exists
  - sources, tabs, and panes derived from the config
  - where logs will be written

## P0 — quick config generation

- [x] Replace `embed-log sample-config` as the visible path with a simpler onboarding command:

  ```bash
  embed-log init
  ```

- [x] `embed-log init` should cover the common desk setups:
  - 1 UART
  - 2 UARTs
  - UART + UDP, for example `PYTEST`
  - UDP only
  - file tail
  - network capture

- [x] Add a non-interactive variant:

  ```bash
  embed-log init --sample double_uart_udp_two_tabs --output embed-log.yml
  ```

- [x] After generating a config, CLI should print the next commands:

  ```bash
  embed-log doctor --config embed-log.yml
  embed-log run --config embed-log.yml
  ```

  And the environment-variable variant:

  ```bash
  export EMBED_LOG_CONFIG_YML_PATH="$PWD/embed-log.yml"
  embed-log run
  ```

## P0 — `embed-log update`

- [x] Add:

  ```bash
  embed-log update
  ```

  It installs the latest release.

- [x] Add installing a specific commit:

  ```bash
  embed-log update --sha <commit_sha>
  ```

- [x] Implement `embed-log update` as a thin Python orchestration layer over the existing platform installer. Do not duplicate installer logic in Python.

  Responsibilities:
  - Python CLI: parse flags, resolve target release/commit, perform anti-rollback checks, print clear UX/errors.
  - `install.sh` / `install.ps1`: perform the actual install/update steps.

- [x] Keep installer selection platform-aware:
  - macOS / Linux: call `install.sh`
  - Windows: call `install.ps1`

- [x] Preserve a stable installer environment contract:

  ```bash
  EMBED_LOG_REF_TYPE=release|branch|tag|commit
  EMBED_LOG_REF=latest|vX.Y.Z|<sha>
  EMBED_LOG_REPO=krezolekcoder/embed-log
  EMBED_LOG_REPO_URL=https://github.com/krezolekcoder/embed-log.git
  ```

- [x] Add an explicit update mode env var for installer UX and future edge cases:

  ```bash
  EMBED_LOG_INSTALL_MODE=update
  ```

- [x] Proposed implementation:
  - `embed-log update` runs the platform installer with:

    ```bash
    EMBED_LOG_INSTALL_MODE=update
    EMBED_LOG_REF_TYPE=release
    EMBED_LOG_REF=latest
    ```

  - `embed-log update --sha <sha>` runs the platform installer with:

    ```bash
    EMBED_LOG_INSTALL_MODE=update
    EMBED_LOG_REF_TYPE=commit
    EMBED_LOG_REF=<sha>
    ```

- [x] Treat `install.sh` / `install.ps1` as the initial-install path. After first install, users should update through `embed-log update`.

## P0 — anti-rollback for `update --sha`

- [x] Before installing `--sha`, check:
  - latest release
  - when that release was published, or which commit/tag it represents
  - commit date for the requested `--sha`

- [x] If the commit is older than the latest release, abort:

  ```text
  Refusing to install <sha>: commit is older than latest release <version>.
  Use --allow-rollback if you really want this.
  ```

- [x] Add an explicit escape hatch:

  ```bash
  embed-log update --sha <sha> --allow-rollback
  ```

- [x] Rollback must be blocked by default.

## P1 — documentation updates

- [x] README: add a short “For agents / quick repo orientation” section:

  ```bash
  embed-log doctor
  embed-log onboard
  embed-log init --list
  ```

- [x] README: document update commands:

  ```bash
  embed-log update
  embed-log update --sha <sha>
  ```

- [x] `embed-log --help`: add `update` to the main command list.

- [x] `embed-log update --help`: explain the intended split:
  - `install.sh` is for first install
  - `embed-log update` is for later updates of an existing install
