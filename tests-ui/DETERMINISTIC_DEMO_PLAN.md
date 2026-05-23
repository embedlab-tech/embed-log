# Deterministic demo profile plan

Goal: provide a predictable demo traffic mode for Playwright/UI regression tests. Tests should not depend on random messages or long random delays.

## Recommendation about time

Do **not** mock the OS/system clock for E2E tests.

Reasons:

- the backend currently timestamps lines at ingestion time (`datetime.now().astimezone()`), not from the payload
- mocking time across subprocesses/threads/WebSocket/frontend would be fragile and non-portable
- tools like `libfaketime` are not cross-platform, and freezing time would also break range/sync assumptions unless we implement a custom clock

Instead, make the **traffic deterministic in relative time and content**:

- deterministic line sequence numbers
- deterministic source names
- deterministic tick/cycle IDs
- deterministic relative delays between messages
- dynamic absolute timestamps are accepted and tested with tolerance or ignored where not important

Tests should assert on stable payload fields like:

```text
TEST src=SENSOR_A tick=001 seq=0001 kind=sync message="..."
```

not on the absolute wall-clock timestamp.

## Why dynamic absolute time is OK

For UI tests we usually care about:

- panes contain expected sources
- logs arrive
- clicking one pane syncs to the nearest timestamp in another pane
- selected time range includes related lines from other panes
- snippet lines are sorted by displayed timestamp
- exported HTML replays the same data

All of these can be validated with deterministic payload IDs and timestamp ordering/tolerance. The initial absolute time can vary.

## Proposed implementation

Add a deterministic profile to `run_demo.sh`:

```bash
DEMO_PROFILE=test ./run_demo.sh --no-browser
```

Profiles:

```text
DEMO_PROFILE=random   current behavior, default
DEMO_PROFILE=test     deterministic high-frequency traffic for UI tests
```

## New traffic generator

Create:

```text
utils/deterministic_demo_traffic.py
```

Responsibilities:

- send deterministic UDP messages to the configured demo sources
- optionally send deterministic inject markers to inject ports
- run forever by default, or for fixed `--cycles N`
- expose predictable cadence through `--tick-ms`

Example CLI:

```bash
python utils/deterministic_demo_traffic.py \
  --udp SENSOR_A=127.0.0.1:6000 \
  --udp SENSOR_B=127.0.0.1:6001 \
  --udp SENSOR_C=127.0.0.1:6002 \
  --inject SENSOR_A=127.0.0.1:5001 \
  --inject SENSOR_B=127.0.0.1:5002 \
  --inject SENSOR_C=127.0.0.1:5003 \
  --tick-ms 100 \
  --cycles 0
```

## Message schedule

For each tick `N`, send related events to all sources with small deterministic offsets:

```text
T + 00ms  SENSOR_A  TEST src=SENSOR_A tick=N seq=... kind=sync msg="controller step N"
T + 10ms  SENSOR_B  TEST src=SENSOR_B tick=N seq=... kind=sync msg="sensor step N"
T + 20ms  SENSOR_C  TEST src=SENSOR_C tick=N seq=... kind=sync msg="network step N"
```

Every few ticks send special messages for specific UI assertions:

```text
tick % 5 == 0   kind=warning
tick % 7 == 0   kind=error
tick % 9 == 0   kind=prefix-cleanup with duplicated [SENSOR_X]
tick % 11 == 0  kind=embedded-timestamp with [2026-...]
```

Example payloads:

```text
TEST src=SENSOR_A tick=012 seq=0036 kind=sync msg="controller state stable"
[SENSOR_A] TEST src=SENSOR_A tick=018 seq=0054 kind=prefix-cleanup msg="duplicated source prefix"
[2026-01-01T00:00:18.000+00:00] TEST src=SENSOR_C tick=018 seq=0056 kind=embedded-timestamp msg="duplicated timestamp prefix"
```

This gives stable fixtures for:

- raw snippet cleanup
- time range selection
- sync tests
- filter tests
- HTML export tests

## How tests should use deterministic data

### Wait for enough data

Instead of sleeping blindly, wait for specific tick text:

```js
await expect(page.locator('#log-SENSOR_A')).toContainText('tick=020');
```

### Select known range

Use line text selectors:

```js
const start = page.locator('#log-SENSOR_A .log-line', { hasText: 'tick=010' }).first();
const end = page.locator('#log-SENSOR_A .log-line', { hasText: 'tick=015' }).first();
await start.click();
await end.click({ modifiers: ['Shift'] });
```

### Assert related panes are included

After raw snippet download, assert:

```text
SENSOR_A tick=010
SENSOR_B tick=010
SENSOR_C tick=010
SENSOR_A tick=015
```

No need to assert exact wall-clock timestamps.

## Time sync testing strategy

For live UI sync:

1. wait for `tick=020` in all panes
2. click `SENSOR_A tick=020`
3. assert `SENSOR_B` and/or `SENSOR_C` scrolls/highlights near `tick=020`

Because backend timestamps are ingestion-time based, exact millisecond equality is not required. Related tick messages should be close in time due to deterministic offsets.

Better assertion options:

- sibling pane highlighted line contains same `tick=N`, if the nearest timestamp maps there
- or highlighted line's tick is within `N ± 1`
- or compare numeric displayed timestamps with tolerance, e.g. `< 500ms`

## Optional exact timestamp mode later

If exact timestamps become necessary, add a test-only backend feature instead of mocking system time globally.

Possible approaches:

1. **Frontend fixture mode**
   - load static/fake log data directly into frontend with fixed timestamps
   - best for pure UI range/sync tests

2. **Backend test timestamp override**
   - hidden/test config option allowing sources to parse timestamp from payload
   - e.g. `logs.use_payload_timestamp: true` only in test configs
   - more invasive, but keeps full backend flow

3. **Synthetic source type**
   - `type: synthetic`
   - emits deterministic events with fixed timestamps
   - useful but adds backend surface area

Initial recommendation: do not implement exact timestamp override yet.

## run_demo.sh integration

Pseudo-flow:

```bash
DEMO_PROFILE="${DEMO_PROFILE:-random}"

if [ "$DEMO_PROFILE" = "test" ]; then
  python utils/deterministic_demo_traffic.py \
    --udp SENSOR_A=127.0.0.1:6000 \
    --udp SENSOR_B=127.0.0.1:6001 \
    --udp SENSOR_C=127.0.0.1:6002 \
    --inject SENSOR_A=127.0.0.1:5001 \
    --inject SENSOR_B=127.0.0.1:5002 \
    --inject SENSOR_C=127.0.0.1:5003 \
    --tick-ms "${DEMO_TEST_TICK_MS:-100}" \
    --cycles 0 &
else
  # existing random UDP simulator + marker injector
fi
```

## Playwright integration

Update `tests-ui/playwright.config.js` to start:

```bash
DEMO_PROFILE=test DEMO_TEST_TICK_MS=50 DEMO_LOG_DIR=tests-ui/.tmp/logs ./run_demo.sh --no-browser
```

## Implementation checklist

- [x] Add `utils/deterministic_demo_traffic.py`
- [x] Add `DEMO_PROFILE=random|test` support in `run_demo.sh`
- [x] Add `DEMO_TEST_TICK_MS` env var
- [x] Update Playwright config to use `DEMO_PROFILE=test`
- [x] Rewrite existing UI tests to wait for `tick=N` instead of random log count
- [ ] Add time sync test using known tick values
- [ ] Add raw snippet cleanup test using deterministic duplicated-prefix messages
- [ ] Add exported HTML replay test using deterministic selected range

## Acceptance criteria

- UI tests do not depend on random message content
- UI tests do not use long fixed sleeps
- test runs produce enough logs within a few seconds
- same tests work on macOS/Linux/CI
- absolute wall-clock timestamp can vary without breaking tests
