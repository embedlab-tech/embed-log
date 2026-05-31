# Curated Demo — Implementation Note

## Artifact

**Output file:** `demo-session.html` (repo root)
**Target:** Website embed / iframe demo

## Session metadata

| Field | Value |
|-------|-------|
| Session ID | `2026-05-31_10-49-40` |
| Generated | 2026-05-31 |
| Export path | `logs/2026-05-31_10-49-40/session.html` (auto-export by server) |
| Source files | `logs/2026-05-31_10-49-40/` |

## Config


| Source | Label | Port (UDP) | Inject Port | Tab | Role |
|--------|-------|-------------|-------------|-----|------|
| SENSOR_A | **DEVICE_A** | 6000 | 5001 | DevA | Device Under Test |
| SENSOR_B | **HOST** | 6001 | 5002 | DevA | Test controller / workstation |
| SENSOR_C | **AUX** | 6002 | 5003 | DevB | Auxiliary monitor |
| SENSOR_D | **PYTEST** | 6004 | 5004 | PYTEST | Test execution log |
| SENSOR_CBOR | **CBOR** | 6003 | — | cbor-tab | Structured diagnostics |

## Story: REST API testing of DEVICE_A

A 3-test HTTP API test suite executed against an embedded device:

| Phase | Ticks | Event | Cross-pane sync moment |
|-------|-------|-------|------------------------|
| **Setup** | 1-3 | Network init, connection established | PYTEST `connecting to DEVICE_A` → HOST `connected` → DEVICE_A `accepted` |
| **Test 1: status** | 4-5 | `GET /api/status` → `200 OK` | HOST `>> GET /api/status` → DEVICE_A `<< GET /api/status` → PYTEST `✓ PASSED` |
| **Test 2: config** | 6-8 | `GET /api/config` → `503` | HOST sends request → DEVICE_A errors → PYTEST `✓ PASSED (expected failure)` |
| **Test 3: health** | 10-12 | `GET /api/health` → `200 OK` | HOST `>> GET /api/health` → DEVICE_A responds healthy → PYTEST `✓ PASSED` |
| **Teardown** | 13-16 | Session close, report written | All panes show completion markers |

### Sync demonstration (best moment)

At tick 5 (relative time ~`T+00:00:00.912`), all three active panes show aligned events:

- **DEVICE_A**: `httpd: >> 200 OK  {status:ok, uptime:3600}  (12ms)`
- **HOST**: `resp: << 200 OK  in 12ms`
- **PYTEST**: `✓ test_01_get_status — PASSED (0.342s)`
- **CBOR**: `kind=response status=200 duration_ms=12`

Clicking any of these lines syncs the other panes to the same timestamp.

### Selection demonstration

Lines 4-7 in DEVICE_A (~ticks 4-5) form a compact 4-line window:
```
httpd: << GET /api/status
handler: processing status request
handler: mem_free=18324KB ...
httpd: >> 200 OK  {status:ok, uptime:3600}
```

- **Exact**: shows only those 4 lines
- **All**: also shows HOST and PYTEST lines at same timestamps
- **Sel…**: choose which sibling panes to include

## Layout

- **DevA**: DEVICE_A (DUT) + HOST (test controller) — main investigation surface
- **DevB**: AUX — background ambient readings corroborating the timeline
- **PYTEST**: test execution steps, assertions, and PASS/FAIL results
- **cbor-tab**: CBOR — structured diagnostic records (request/response pairs, test results, summary)

## Generation

### Quick regeneration (one command)

```bash
cd embed-log
python3 utils/curated_demo_logs.py
```

This script:
1. Starts the embed-log server with `embed-log.demo.yml`
2. Connects inject clients for all 4 sources (5001-5004)
3. Sends curated log lines via UDP to ports 6000/6001/6002/6004
4. Sends CBOR datagrams to port 6003
5. Sends coloured inject markers at key moments
6. Stops the server (triggers auto-export of session.html)
7. Copies the exported file to `demo-session.html`

### Step-by-step (for debugging)

```bash
# 1. Start server
python3 backend/server.py run --config embed-log.demo.yml \
    --ws-port 8080 --no-open-browser

# 2. In another terminal, generate traffic
python3 utils/curated_demo_logs.py --no-serve --no-export

# 3. Stop the server (auto-exports session.html to session directory)
# 4. The session.html is in logs/<session-id>/session.html
```

## Prerequisites

- Python 3 with `cbor2` installed (`pip install cbor2`)
- No process on ports 8080 (WS), 5001-5004 (inject), or 6000-6004 (UDP)

## Markers (inject lines)

16 coloured inject markers are written during generation:

| Tick | Source | Message | Colour |
|------|--------|---------|--------|
| 1 | All | Session started — {source} online | Green |
| 3 | PYTEST | test_01_get_status — starting | Cyan |
| 3 | HOST | GET /api/status — request sent | Cyan |
| 5 | DEVICE_A | GET /api/status — 200 OK answered | Green |
| 5 | PYTEST | ✓ test_01_get_status PASSED | Green |
| 6 | PYTEST | test_02_get_config — starting | Cyan |
| 6 | HOST | GET /api/config — request sent | Cyan |
| 8 | DEVICE_A | GET /api/config — 503 answered (expected error) | Yellow |
| 8 | PYTEST | ✓ test_02_get_config PASSED (expected failure) | Green |
| 9 | AUX | Cross-check — ambient nominal during test | Cyan |
| 10 | PYTEST | test_03_get_health — starting | Cyan |
| 10 | HOST | GET /api/health — request sent | Cyan |
| 12 | DEVICE_A | GET /api/health — 200 OK answered | Green |
| 12 | PYTEST | ✓ test_03_get_health PASSED | Green |
| 14 | All | Test suite complete — {source} done | Green |

These are visible as ANSI-coloured entries in the exported HTML. True saved markers (with navigable line ranges from `markers.json`) are runtime-only.

## Helper files

- `utils/curated_demo_logs.py` — self-contained Python script (also usable in `--no-serve` mode)
- `embed-log.demo.yml` — demo config with new pane names and PYTEST source

## Changes to existing code

One edit to `utils/merge_logs.py` (`_render_toolbar`): added the timestamp-mode toggle button (`btn-timestamp-mode`) to the static toolbar. The log data already carried both absolute and relative timestamps; only the UI element was missing.

## What the exported HTML demonstrates

| Feature | How to demo |
|---------|-------------|
| **Multi-pane sync** | In DevA, click a line in DEVICE_A near `T+00:00:00.912` (200 OK); HOST scrolls to same timestamp showing `<< 200 OK` |
| **Range selection** | Select 4 lines in DEVICE_A around the status request (lines ~4-7). Use `Exact` → only DEVICE_A lines; `All` → correlated lines from HOST and PYTEST; `Sel…` → pick which panes |
| **Tab navigation** | Switch between DevA, DevB, PYTEST, cbor-tab. Unwrap to see per-pane tabs |
| **PYTEST tab** | Shows structured test execution: `[STEP]`, `[PASS]`, assertions, timing |
| **CBOR diagnostics** | `kind=response status=503 error=config_unavailable` and `kind=summary passed=3 failed=0` |
| **Timestamp toggle** | Settings → Absolute / Relative toggle |
| **Theme toggle** | Moon button switches light/dark |
| **Download raw** | Download merged or per-pane log files |
| **Static audit** | WS status, Export HTML, Clear, and TX hidden (static mode) |

## Runtime-only capabilities (not in static export)

| Capability | Recommended demo clip |
|------------|----------------------|
| **WebSocket live feed** | Start a live session; show logs arriving across panes |
| **Save HTML / Export** | After investigation, click Save HTML to produce the artifact |
| **Open HTML / Import** | Show opening a previously saved .html from disk |
| **New session rotation** | Click "New session" → session rotated, old one saved |
| **Sessions popup** | Open sessions list, browse history |
| **TX input / send command** | Type and send a command to the device |
| **Marker save/navigation** | Select lines → save marker → navigate via marker list |
| **Filter by regex** | Type a regex in the filter box |
| **Cached state restore** | Close/reopen → layout and state restored |

### Recommended runtime demo sequence (30-45 second clip)

1. Live viewer connected, logs streaming in
2. Click a line in DEVICE_A → HOST syncs to same request/response pair
3. Select a range of interesting lines → save as marker
4. Open markers list → click marker → viewport jumps
5. Click `Save HTML` → browser downloads session.html
6. Open the downloaded HTML → same investigation state in static replay
