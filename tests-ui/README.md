# embed-log UI tests

This folder contains headless browser tests for the embed-log UI using Playwright.

## What is Playwright?

Playwright is a browser automation/test framework. It can run real Chromium, Firefox, and WebKit browsers in headless mode and interact with the page like a user: click, drag, type, wait for WebSocket-driven UI updates, and verify downloaded files.

For this project it is useful because the frontend is plain browser JavaScript and many important features are UI/browser behaviors:

- WebSocket connection state
- dynamic pane creation
- log rendering
- click/shift-click/drag selection
- snippet downloads
- exported HTML files

## Install

From this folder:

```bash
cd tests-ui
npm install
npm run install-browsers
```

## Run tests

Default run:

```bash
npm test
```

By default the Playwright config starts the bundled demo automatically with deterministic test traffic and a temporary logs directory:

```bash
DEMO_LOG_DIR=tests-ui/.tmp/logs DEMO_PROFILE=test DEMO_TEST_TICK_MS=100 ../run_demo.sh --no-browser
```

and then opens:

```text
http://127.0.0.1:8080/
```

## Run against an already running backend

If you already started the demo/backend manually:

```bash
cd ..
./run_demo.sh --no-browser
```

For deterministic logs, start it with the test profile:

```bash
DEMO_PROFILE=test DEMO_TEST_TICK_MS=100 ./run_demo.sh --no-browser
```

then in another terminal:

```bash
cd tests-ui
E2E_START_DEMO=0 npm test
```

Use a custom URL:

```bash
E2E_START_DEMO=0 E2E_BASE_URL=http://127.0.0.1:8081 npm test
```

## Test logs and artifacts

When Playwright starts the demo itself, backend logs are written under:

```text
tests-ui/.tmp/logs
```

`global-teardown.js` removes `tests-ui/.tmp` after the test run. Keep those logs for debugging with:

```bash
E2E_KEEP_LOGS=1 npm test
```

If you run the backend/demo manually with `E2E_START_DEMO=0`, cleanup is your responsibility.

## Useful modes

Run with a visible browser:

```bash
npm run test:headed
```

Interactive Playwright UI:

```bash
npm run test:ui
```

Debug one test:

```bash
npm run test:debug
```

## Current coverage

The initial tests cover:

- UI connects to backend and receives demo logs
- Shift+Click range selection
- raw merged snippet download and duplicate-prefix cleanup
- HTML snippet download using the regular embed-log exported UI
