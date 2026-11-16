import { expect, test } from '@playwright/test';

const config = (session, extra = {}) => ({
  type: 'config', session: { id: session },
  tabs: [{ label: 'Sources', panes: ['A', 'B'] }, { label: 'Merged', panes: ['ALL'] }],
  pane_labels: { A: 'ALPHA', B: 'BETA' }, pane_kinds: { A: 'uart', B: 'uart' },
  merges: [{ name: 'ALL', of: ['A', 'B'] }],
  ...extra,
});
const live = (session, sequence, message, extra = {}) => ({
  sequence, message, data: message, source_id: 'A', source_kind: 'uart', type: 'rx',
  timestamp: '12:00:00', timestamp_num: sequence, line_idx: sequence, session_id: session, ...extra,
});
const zephyrSources = {
  A: { levels: [
    { id: 'err', label: 'Error', color: null, default_visible: true },
    { id: 'wrn', label: 'Warning', color: null, default_visible: true },
    { id: 'inf', label: 'Info', color: null, default_visible: true },
    { id: 'dbg', label: 'Debug', color: null, default_visible: false },
  ] },
  B: { levels: [
    { id: 'fatal', label: 'Fatal', color: '#ff00ff', default_visible: true },
    { id: 'trace', label: 'Trace', color: null, default_visible: false },
  ] },
};

async function serve(page, session, rows, extra = {}) {
  await page.route(`**/sessions/${session}/combined.jsonl`, route => route.fulfill({
    contentType: 'application/x-ndjson', body: rows.map(JSON.stringify).join('\n') + '\n',
  }));
  await page.routeWebSocket('**/ws', ws => {
    ws.send(JSON.stringify(config(session, extra)));
    for (const row of rows) ws.send(JSON.stringify(row));
  });
}

const paneText = (page, pane) => page.locator(`#log-${pane} .log-line`).allInnerTexts();

test('default hides debug, toggles per pane, counts levels and survives reload', async ({ page }) => {
  const s = 'levels-default';
  const rows = [
    live(s, 1, '<err> boom', { level: 'err' }),
    live(s, 2, '<dbg> chatty', { level: 'dbg' }),
    live(s, 3, 'uart:~$ prompt'),
    live(s, 4, 'help', { type: 'tx' }),
    live(s, 5, '<inf> ready', { level: 'inf' }),
  ];
  await serve(page, s, rows, { source_levels: zephyrSources });
  await page.goto('/');
  await expect.poll(() => page.evaluate(async () => (await import('/state.js')).state.rawLines.A.length)).toBe(5);
  const button = page.locator('#pane-A .level-mode');
  await expect(button).toHaveText('Levels 4/5');
  await expect.poll(async () => (await paneText(page, 'A')).join('\n')).not.toContain('chatty');
  const visible = (await paneText(page, 'A')).join('\n');
  for (const text of ['boom', 'prompt', 'help', 'ready']) expect(visible).toContain(text);

  await button.click();
  const menu = page.locator('#pane-A .level-menu');
  await expect(menu).toBeVisible();
  await expect(menu.locator('label')).toHaveText([/Error\s*1/, /Warning\s*0/, /Info\s*1/, /Debug\s*1/, /No level\s*1/]);
  await menu.locator('input[data-level="dbg"]').check();
  await expect(button).toHaveText('Levels');
  await expect.poll(async () => (await paneText(page, 'A')).join('\n')).toContain('chatty');
  await menu.locator('input[data-level=""]').uncheck();
  await expect.poll(async () => (await paneText(page, 'A')).join('\n')).not.toContain('prompt');
  await page.keyboard.press('Escape');
  await expect(menu).toBeHidden();

  // Highlight only counts rows that pass the level filter.
  await page.locator('#pane-A .search-mode').click();
  await page.locator('.filter-input[data-pane="A"]').fill('prompt');
  await expect(page.locator('#pane-A .search-count')).toHaveText('0 / 0');
  await page.locator('.filter-input[data-pane="A"]').fill('chatty');
  await expect(page.locator('#pane-A .search-count')).toHaveText('0 / 1');
  await page.locator('#pane-A .search-mode').click();
  await page.locator('.filter-input[data-pane="A"]').fill('');

  await page.reload();
  await expect.poll(() => page.evaluate(async () => (await import('/state.js')).state.rawLines.A.length)).toBeGreaterThan(0);
  await expect(page.locator('#pane-A .level-mode')).toHaveText('Levels 4/5');
  await page.locator('#pane-A .level-mode').click();
  await expect(page.locator('#pane-A input[data-level="dbg"]')).toBeChecked();
  await expect(page.locator('#pane-A input[data-level=""]')).not.toBeChecked();
  await page.locator('#pane-A .level-show-all').click();
  await expect(page.locator('#pane-A .level-mode')).toHaveText('Levels');
});

test('regex filter and level filter combine; merged panes offer the union of their sources', async ({ page }) => {
  const s = 'levels-merge';
  const rows = [
    live(s, 1, '<err> net down', { level: 'err' }),
    live(s, 2, '<dbg> net retry', { level: 'dbg' }),
    live(s, 3, 'FATAL core', { source_id: 'B', level: 'fatal' }),
    live(s, 4, 'TRACE core', { source_id: 'B', level: 'trace' }),
  ];
  await serve(page, s, rows, { source_levels: zephyrSources });
  await page.goto('/');
  await expect.poll(() => page.evaluate(async () => (await import('/state.js')).state.rawLines.A.length)).toBe(2);
  await page.locator('.filter-input[data-pane="A"]').fill('net');
  await expect.poll(() => paneText(page, 'A')).toHaveLength(1);
  await expect(page.locator('#log-B .log-line').filter({ hasText: 'FATAL' })).toHaveCSS('color', 'rgb(255, 0, 255)');
  await expect.poll(async () => (await paneText(page, 'B')).join('\n')).not.toContain('TRACE');

  await page.getByRole('button', { name: 'Merged' }).click();
  await page.locator('#pane-ALL .level-mode').click();
  await expect(page.locator('#pane-ALL .level-menu label .level-name'))
    .toHaveText(['Error', 'Warning', 'Info', 'Debug', 'Fatal', 'Trace', 'No level']);
  const merged = (await paneText(page, 'ALL')).join('\n');
  expect(merged).toContain('net down');
  expect(merged).toContain('FATAL core');
  expect(merged).not.toContain('net retry');
  expect(merged).not.toContain('TRACE core');
});

test('without server level definitions the UI falls back to Zephyr detection; null disables the control', async ({ page }) => {
  const s = 'levels-fallback';
  await serve(page, s, [live(s, 1, '<dbg> hidden by default'), live(s, 2, '<wrn> shown'),
    live(s, 3, '<dbg> off source', { source_id: 'B' }),
    live(s, 4, 'help: use <dbg> to enable diagnostics'),
    live(s, 5, '[2024-01-01T00:00:00Z]: <dbg> quoted [ERROR]')]);
  await page.goto('/');
  await expect.poll(() => page.evaluate(async () => (await import('/state.js')).state.rawLines.A.length)).toBe(4);
  await expect(page.locator('#pane-A .level-mode')).toHaveText('Levels 4/5');
  await expect.poll(async () => (await paneText(page, 'A')).join('\n')).not.toContain('hidden by default');
  await expect.poll(async () => (await paneText(page, 'A')).join('\n')).toContain('help: use <dbg>');
  await expect.poll(async () => (await paneText(page, 'A')).join('\n')).not.toContain('quoted [ERROR]');

  await page.evaluate(async () => {
    window.__embedLogSourceLevels = { A: null, B: null };
    (await import('/levels.js')).refreshLevelDefinitions();
    window.__embedLogRefreshLevelControls();
  });
  await expect(page.locator('#pane-A .level-filter')).toBeHidden();
});
