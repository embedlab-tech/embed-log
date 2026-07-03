# Terminal UI

The terminal UI is a ratatui/crossterm client for the same `embed-log-core` server used by the browser UI. It is useful over SSH, in labs where a browser is inconvenient, or when you want live logs beside other terminal tools.

## Launch modes

Start the server and TUI in one process:

```bash
embed-log run --config embed-log.yml --tui
embed-log demo --tui
```

Connect the standalone TUI to an already-running server:

```bash
embed-log-tui connect ws://127.0.0.1:8080/ws
# equivalent shorthand
embed-log-tui --url ws://127.0.0.1:8080/ws
```

The standalone `embed-log-tui` binary is a client only. It does not load YAML configs or start a server; use `embed-log run --tui` for that.

## Supported features

- live log viewing over `/ws`
- tabs and one/two-pane layouts
- pane focus and synchronized timestamp navigation
- absolute/relative timestamp toggle
- selection and clipboard copy
- user/event markers and marker navigation
- events timeline tab when event rules are configured
- clear active pane
- UART TX input for writable UART sources
- reconnect after server restart/disconnect

## Limitations

- The TUI does not run browser JavaScript plugins. Plugin configuration is visible as metadata, but plugin-rendered browser UI is not reproduced.
- The TUI does not provide onboarding. Create a config with `embed-log onboard`, `embed-log init`, or by editing YAML first.
- Static HTML export/session browsing is still better handled by the browser UI or the `embed-log sessions` CLI commands.

## Keybindings

Press `?` inside the TUI to show the built-in help overlay.

| Key | Action |
| --- | --- |
| `q`, `Ctrl-C` | Quit |
| `Tab`, `Shift-Tab` | Next/previous tab |
| `h`/`l`, `←`/`→` | Focus left/right pane |
| `j`/`k`, `↓`/`↑` | Scroll active pane |
| `PageDown`, `PageUp` | Half-page scroll |
| `g`, `G` | Top/bottom of active pane |
| `Enter` | Sync panes to the current line timestamp |
| `Space` | Toggle selection on current line |
| `v` | Visual range selection |
| `Esc` | Clear selection, or close help |
| `c` | Toggle exact/context selection scope |
| `y` | Copy selected lines to clipboard |
| `m` | Toggle marker on current line |
| `[`, `]` | Previous/next marker |
| `M` | Include/exclude event markers in marker navigation |
| `t` | Toggle absolute/relative timestamps |
| `u` | Toggle unwrap mode |
| `C` | Clear active pane in the UI |
| `:`, `i` | Open TX input for writable UART panes |
| `e` | Open Events tab when event rules are configured |
| `?` | Show/close help overlay |

## Related CLI commands

Inspect recorded sessions from the terminal:

```bash
embed-log sessions list --dir logs
embed-log sessions combined <SESSION_ID> --dir logs --lines 100
embed-log sessions tail-combined <SESSION_ID> --dir logs --follow
embed-log sessions events <SESSION_ID> --dir logs --severity fatal
embed-log sessions search --dir logs --contains panic
```
