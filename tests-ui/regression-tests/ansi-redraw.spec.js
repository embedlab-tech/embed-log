import { test, expect } from '@playwright/test';

test('erased redraws stay in raw history but not in virtual rows or filtered view', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#log-SENSOR_A')).toBeVisible();
    await page.evaluate(async () => {
        const { appendLineBatch, clearPane } = await import('/lines.js');
        clearPane('SENSOR_A');
        const values = [
            ['device> \x1b[8D\x1b[J', false],
            ['redraw-test: log', false],
            ['device> help', false],
            ['Available commands:', false],
            ['  help', false],
            ['device> \x1b[8D\x1b[J', true],
        ];
        appendLineBatch(values.map(([rawText, isTx]) => ({ paneId: 'SENSOR_A', ts: 'T+00:00:00.000', rawText, isTx })));
    });
    await expect(page.locator('#log-SENSOR_A .log-line')).toHaveCount(5);
    await expect(page.locator('#log-SENSOR_A .tx-line')).toHaveCount(1);
    const rawRows = await page.evaluate(async () => {
        const { state } = await import('/state.js');
        return state.rawLines.SENSOR_A.map(entry => Array.isArray(entry) ? entry[1] : entry.rawText);
    });
    expect(rawRows).toHaveLength(6);
    expect(rawRows[0]).toBe('device> \x1b[8D\x1b[J');
    await expect(page.locator('#log-SENSOR_A')).toContainText('Available commands:');

    // Exercise incremental projection, virtualization, regex changes and tail follow.
    await page.evaluate(async () => {
        const { appendLineBatch } = await import('/lines.js');
        const rows = [];
        for (let n = 0; n < 300; n++) {
            for (const rawText of ['device> \x1b[8D\x1b[J', `redraw-log-${n}`]) {
                rows.push({ paneId: 'SENSOR_A', ts: 'T+00:00:01.000', rawText, isTx: false });
            }
        }
        appendLineBatch(rows);
    });
    await expect(page.locator('#log-SENSOR_A')).toContainText('redraw-log-299');
    await expect(page.locator('#log-SENSOR_A .log-line', { hasText: 'device>' })).toHaveCount(0);
    // Previously persisted line objects do not carry the new classification.
    await page.evaluate(async () => {
        const { state } = await import('/state.js');
        const { buildStoredLine, rerenderPane } = await import('/lines.js');
        const legacy = buildStoredLine('SENSOR_A', '', 'device> \x1b[8D\x1b[J', false);
        delete legacy.transientConsoleRedraw;
        state.rawLines.SENSOR_A.push(legacy);
        rerenderPane('SENSOR_A');
    });
    const filter = page.locator('.filter-input[data-pane="SENSOR_A"]');
    await filter.fill('device>');
    await expect(page.locator('#log-SENSOR_A .log-line:visible')).toHaveCount(2);
    await filter.fill('redraw-log-299');
    await expect(page.locator('#log-SENSOR_A .log-line:visible')).toHaveCount(1);
    await filter.fill('');
    await expect(page.locator('#log-SENSOR_A')).toContainText('redraw-log-299');
    await expect(page.locator('#log-SENSOR_A .log-line', { hasText: 'device>' })).toHaveCount(0);
});

test('a pane containing only redraws has no blank spacer rows and can resume', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#log-SENSOR_A')).toBeVisible();
    await page.evaluate(async () => {
        const { appendLineBatch, clearPane } = await import('/lines.js');
        clearPane('SENSOR_A');
        appendLineBatch([{ paneId: 'SENSOR_A', ts: '', rawText: 'device> \x1b[8D\x1b[J', isTx: false }]);
    });
    await expect(page.locator('#log-SENSOR_A .log-line')).toHaveCount(0);
    await page.evaluate(async () => {
        const { appendLineBatch } = await import('/lines.js');
        appendLineBatch([{ paneId: 'SENSOR_A', ts: '', rawText: 'response preserved', isTx: false }]);
    });
    await expect(page.locator('#log-SENSOR_A .log-line')).toHaveCount(1);
    await expect(page.locator('#log-SENSOR_A')).toContainText('response preserved');
});
