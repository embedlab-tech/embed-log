# UI/E2E tests backlog

Current deterministic Playwright suite covers the main smoke/regression path and passes locally (`10 passed`).

## Implemented

- Live UI connects and receives deterministic logs.
- Shift+Click selects a range.
- Raw snippet download cleans duplicated prefixes/timestamps.
- HTML snippet uses the regular exported embed-log UI.
- Downloaded HTML snippet reopens as static replay.
- Live tab/pane layout is verified.
- Time synchronization highlights sibling pane lines.
- Full toolbar `Export` downloads and reopens as static replay.
- Filter by deterministic `kind=filter-alpha` works.
- Escape clears selection.

## Remaining backlog

### 1. Backend session HTML flow

- Test backend-generated `session.html` artifact.
- Verify toolbar `Current HTML` opens the current session export.
- Assert panes/logs are present in the opened artifact.

### 2. Clipboard UX

- `Copy range` clipboard content matches downloaded raw file content.
- Direct clipboard copy button copies selected range.
- Platform shortcut copy works:
  - macOS: `Meta+C`
  - Linux/Windows: `Control+C`
- Clipboard buffer workflow:
  - `Clipboard add` for one pane,
  - add another range from a second pane,
  - open clipboard peek,
  - verify both selections exist,
  - `Copy all` works,
  - `Clear` empties buffer.

### 3. Drag selection

- Drag-select a range in a pane.
- Assert selected lines are contiguous.
- Assert sibling panes are not selected.
- Assert copy actions appear after drag selection.

### 4. Session workflows

- `Clean session` / session rotation:
  - trigger clean session,
  - confirm panes clear,
  - wait for new session id,
  - assert new deterministic logs arrive,
  - assert old stale lines do not remain.
- Sessions popup:
  - open sessions UI,
  - assert current session is marked,
  - assert manifest link exists,
  - assert open-html link exists after export is ready.

### 5. Page error guard

Add shared Playwright helper that fails tests on unexpected frontend errors:

```js
export function collectPageErrors(page) {
  const errors = [];
  page.on('pageerror', err => errors.push(String(err)));
  page.on('console', msg => {
    if (msg.type() === 'error') errors.push(msg.text());
  });
  return errors;
}
```

Use in tests or `afterEach`:

```js
expect(errors).toEqual([]);
```

Allowlist only if absolutely necessary.

### 6. Optional helper/unit tests

- Timestamp parser helper tests.
- Snippet cleanup helper tests.
- Range merge/sort helper tests.

## Suggested next implementation order

1. Add page error guard helper.
2. Add `Copy range` vs raw file consistency test.
3. Add platform shortcut copy test.
4. Add clipboard buffer test.
5. Add backend `Current HTML` / session export test.
6. Add `Clean session` rotation test.
7. Add sessions popup test.
8. Add drag selection test.
9. Add optional frontend helper/unit tests.
