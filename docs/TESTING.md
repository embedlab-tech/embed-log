# TESTING

## Test layers

`embed-log` currently has two practical test layers:

1. **Backend/unit tests** in `tests/`
2. **Browser E2E tests** in `tests-ui/` using Playwright

## Backend tests

Run from repository root:

```bash
python3 -m unittest discover -s tests -v
```

Backend tests should cover:
- config parsing/validation,
- source parsing behavior,
- session/manifest/export helpers,
- protocol-safe backend behavior that does not require a browser.

## UI / E2E tests

Install once:

```bash
cd tests-ui
npm install
npm run install-browsers
```

Run full suite:

```bash
cd tests-ui
npm test
```

Run one spec:

```bash
cd tests-ui
npx playwright test tests/session-workflows.spec.js
```

## Deterministic demo strategy

The Playwright suite relies on deterministic demo traffic rather than random log timing.

Key idea:
- tests wait for known markers such as `tick=011` or `kind=filter-alpha`
- tests should not rely on arbitrary sleeps or rough line counts

Important deterministic helpers live in:
- `tests-ui/tests/helpers.js`

Common helper patterns:
- `waitForLineContaining(...)`
- `waitForRangePair(...)`
- `waitForSourceTestLine(...)`
- `collectPageErrors(page)`

## What UI tests should protect

- backend-driven tab/pane layout
- live WS connection and rendering
- live rendering keeps the full pane history visible while logs stream
- selection and clipboard flows
- snippet export and replay
- full export replay
- session HTML generation/opening
- session rotation and stale-line clearing
- cache/persistence-sensitive UI behavior

## Test-writing rules for this repo

- Prefer deterministic text/tick assertions over sleeps.
- Prefer behavioral assertions over style-only assertions.
- For exported HTML, be careful with live-vs-static DOM differences.
- When touching frontend behavior, add or update a Playwright test if the behavior is user-visible and regression-prone.

## Related docs

- `FRONTEND_BACKLOG.md` — UI testing backlog and frontend gaps
- `BENCHMARKING.md` — throughput/stress benchmark notes
- `tests-ui/README.md` — Playwright-specific operational details
