import { expect, test } from '@playwright/test';

// Filtering a pane without plugins tests compact rows directly. The result
// must equal matchesFilter() on fully built lines, and rows must stay compact.

const rows = [
  ['11-16 12:00:00.000', '\x1b[1;31m<err> boom & <bang>\x1b[0m', false, 0],
  ['11-16 12:00:01.000', '<inf> net: link "up" rssi=-60', false, 1000],
  ['11-16 12:00:02.000', 'help', true, 2000],
  ['11-16 12:00:03.000', 'uart:~$ \x1b[8D\x1b[J', false, 3000],
  ['11-16 12:00:04.000', 'plain text 4', false, 4000],
];

async function setup(page) {
  await page.routeWebSocket('**/ws', ws => ws.send(JSON.stringify({
    type: 'config', session: { id: 'compact' }, tabs: [{ label: 'Logs', panes: ['A'] }], pane_kinds: { A: 'uart' },
    source_levels: { A: null },
  })));
  await page.goto('/');
  await expect(page.locator('#ws-status')).toContainText(/connected/i);
}

test('compact-row filtering matches full-line filtering in absolute and relative time', async ({ page }) => {
  await setup(page);
  const results = await page.evaluate(async rows => {
    const { appendLineBatch, buildStoredLine, matchesFilter, visibleRowIndices, setTimestampMode, rerenderPane } = await import('/lines.js');
    const { state } = await import('/state.js');
    // Repeat the sample rows so most of the pane stays compact (only the
    // rendered window is built into full lines).
    const all = [];
    for (let r = 0; r < 400; r++) for (const [ts, rawText, isTx, rel] of rows) all.push([ts, `${rawText} #${r}`, isTx, rel]);
    const meta = (i, ts, rel) => ({ sequence: i + 1, absTs: ts, absNum: 1e12 + rel, relTs: `T+00:00:0${rel / 1000}.000`, relNum: rel });
    appendLineBatch(all.map(([ts, rawText, isTx, rel], i) => ({ paneId: 'A', ts, rawText, isTx, meta: meta(i, ts, rel) })));
    const out = [];
    for (const mode of ['absolute', 'relative']) {
      setTimestampMode(mode);
      for (const source of ['&lt;err', '<inf>', '&amp;', 'T\\+00:00:01', '12:00:0[34]', '\\x1b\\[1', 'bang', 'help', 'uart', '"up"', '#39\\b']) {
        const rx = new RegExp(source, 'i');
        state.filters.A = rx;
        rerenderPane('A');
        const got = visibleRowIndices('A');
        const expected = [];
        all.forEach(([ts, rawText, isTx, rel], i) => {
          if (matchesFilter(buildStoredLine('A', ts, rawText, isTx, meta(i, ts, rel)), rx, 'A')) expected.push(i);
        });
        out.push({ mode, source, got, expected });
      }
    }
    const compact = state.rawLines.A.filter(Array.isArray).length;
    return { out, compact, total: all.length };
  }, rows);
  for (const { mode, source, got, expected } of results.out) expect(got, `${mode} ${source}`).toEqual(expected);
  // Only the timestamp of the other mode matches nothing.
  expect(results.out.filter(r => r.got.length === 0).map(r => `${r.mode} ${r.source}`))
    .toEqual(['absolute T\\+00:00:01', 'relative 12:00:0[34]']);
  expect(results.compact).toBeGreaterThan(results.total / 2);
});

test('filtering a large pane keeps rows compact and is fast', async ({ page }) => {
  test.setTimeout(120000);
  await setup(page);
  const result = await page.evaluate(async () => {
    const { appendLineBatch, visibleRowIndices } = await import('/lines.js');
    const { state } = await import('/state.js');
    const N = 300000;
    for (let b = 0; b < N; b += 5000) {
      const batch = [];
      for (let i = b; i < b + 5000; i++) batch.push({ paneId: 'A', ts: '11-16 12:00:00.000', rawText: `\x1b[0m<inf> net: packet ${i} len=${i % 1500}`, isTx: false,
        meta: { sequence: i + 1, absTs: '11-16 12:00:00.000', absNum: 1e12 + i, relTs: 'T+00:00:00.000', relNum: i } });
      appendLineBatch(batch);
    }
    const builtBefore = state.rawLines.A.filter(entry => !Array.isArray(entry)).length;
    const input = document.querySelector('.filter-input[data-pane="A"]');
    input.value = 'len=7(?!\\d)';
    const started = performance.now();
    input.dispatchEvent(new Event('input'));
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    const ms = performance.now() - started;
    const materialized = state.rawLines.A.filter(entry => !Array.isArray(entry)).length - builtBefore;
    return { ms, materialized, visible: visibleRowIndices('A').length };
  });
  expect(result.visible).toBe(200);
  // Only the rendered window is built into full line objects (all 300k before).
  expect(result.materialized).toBeLessThan(500);
  // Building every row took ~570 ms locally for 300k rows (~185 ms now).
  expect(result.ms).toBeLessThan(450);
});
