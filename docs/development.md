# Development

## Prerequisites

- Rust toolchain compatible with workspace `rust-version = 1.77`
- `just` for convenience recipes
- Node/npm only for UI tests under `tests-ui/`
- Platform dependencies for Tauri when working on the desktop app

## Common commands

```bash
just --list
just build
just check
just test
just verify
```

Run browser mode:

```bash
just run embed-log.yml
just run-headless embed-log.yml
```

Run demos:

```bash
just demo
just demo-headless
just demo-desktop
```

Generate config:

```bash
just init embed-log.yml
```

Diagnostics:

```bash
just doctor embed-log.yml
just ports
```

## Release helper commands

Local CLI packaging:

```bash
just package-cli-current
```

Explicit target name packaging:

```bash
just package-cli x86_64-unknown-linux-gnu
```

Platform-specific helpers:

```bash
just package-cli-linux
just package-cli-macos
just package-cli-windows
```

Validate release helper scripts:

```bash
just release-check
```

Create/push a release tag:

```bash
just release-tag v0.1.0
```

See [releasing.md](releasing.md).

## Workspace layout

```text
Cargo.toml
├── crates/embed-log-core
│   └── src
│       ├── clock.rs
│       ├── config/
│       ├── demo.rs
│       ├── frontend_assets.rs
│       ├── models.rs
│       ├── naming.rs
│       ├── net/
│       ├── parsers/
│       ├── runtime/
│       ├── session/
│       └── sources/
├── crates/embed-log-cli
│   └── src/main.rs
├── crates/embed-log-tauri
│   ├── src/
│   └── tauri.conf.json
├── frontend/
├── tests-ui/
├── scripts/
└── docs/
```

## Adding a source type

1. Add a struct in `crates/embed-log-core/src/sources/` implementing `LogSource`.
2. Export it from `sources/mod.rs`.
3. Add config validation in `config/loader.rs`.
4. Add fields to `config/models.rs` if needed.
5. Instantiate it in `runtime/server.rs::resolve_sources`.
6. Add tests for config validation and source behavior.
7. Update [configuration.md](configuration.md) and [architecture.md](architecture.md).

Data flow expected from a source:

```text
I/O source ─▶ parser ─▶ LogEntry ─▶ mpsc::Sender<LogEntry>
```

The runtime owns writing, broadcasting, replay, stats, and session metadata.

## Adding a parser

1. Implement `StreamParser` in `crates/embed-log-core/src/parsers/`.
2. Export it from `parsers/mod.rs`.
3. Add it to `create_parser`.
4. Validate allowed use in `config/loader.rs`.
5. Document the `parser.type` in [configuration.md](configuration.md).

## Adding HTTP/WS functionality

HTTP routes and WebSocket commands live in `crates/embed-log-core/src/net/ws_server.rs`.

Checklist:

- add route or command handler
- add/extend `ServerState` only if needed
- broadcast state-changing events when the frontend must react
- add tests where possible
- update [architecture.md](architecture.md)

## Frontend development

The frontend is plain browser JS modules in `frontend/`.

- `main.js` controls live-mode import order.
- `ws.js` consumes the server config message and live events.
- `state.js`, `lines.js`, `tabcreate.js`, `tabs.js` own most viewer state/rendering.
- `renderPane.js` and `renderToolbar.js` are shared by live/static paths.
- `pluginRuntime.js` is the plugin integration point.

The Rust binary embeds `frontend/` through `rust-embed`, but during development the server prefers a real filesystem `frontend_dir` when `index.html` exists.

## UI tests

Install test dependencies:

```bash
just ui-install
just ui-install-browsers
```

Run tests:

```bash
just ui-unit
just ui-e2e
just ui-parity
just ui-all
```

## Tauri development

Run desktop demo:

```bash
just demo-desktop
```

On first run without a config, the Tauri app shows onboarding and writes an `embed-log.yml` to the app config directory. With a valid config, Tauri starts `LogServer` and navigates the webview to the local HTTP server.

## Generated files and ignored outputs

Ignored outputs include:

- `target/`
- `dist/`
- `logs/`
- `.tmp/`
- Playwright/test reports
- generated static exports such as `session.html`, `merged.html`, `parsed/`
