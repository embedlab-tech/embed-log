import { test, expect } from '@playwright/test';

// UART_DUT receives no traffic from the test server, so exact row counts are
// not racing with live UDP data (unlike SENSOR_A..D).
const PANE = 'UART_DUT';

async function openSilentPane(page) {
    await page.goto('/');
    await page.getByRole('button', { name: 'UART', exact: true }).click();
    await expect(page.locator(`#log-${PANE}`)).toBeVisible();
}

test('erased redraws stay in raw history but not in virtual rows or filtered view', async ({ page }) => {
    await openSilentPane(page);
    await page.evaluate(async (paneId) => {
        const { appendLineBatch, clearPane } = await import('/lines.js');
        clearPane(paneId);
        const values = [
            ['device> \x1b[8D\x1b[J', false],
            ['redraw-test: log', false],
            ['device> help', false],
            ['Available commands:', false],
            ['  help', false],
            ['device> \x1b[8D\x1b[J', true],
        ];
        appendLineBatch(values.map(([rawText, isTx]) => ({ paneId, ts: 'T+00:00:00.000', rawText, isTx })));
    }, PANE);
    await expect(page.locator(`#log-${PANE} .log-line`)).toHaveCount(5);
    await expect(page.locator(`#log-${PANE} .tx-line`)).toHaveCount(1);
    const rawRows = await page.evaluate(async (paneId) => {
        const { state } = await import('/state.js');
        return state.rawLines[paneId].map(entry => Array.isArray(entry) ? entry[1] : entry.rawText);
    }, PANE);
    expect(rawRows).toHaveLength(6);
    expect(rawRows[0]).toBe('device> \x1b[8D\x1b[J');
    await expect(page.locator(`#log-${PANE}`)).toContainText('Available commands:');

    // Exercise incremental projection, virtualization, regex changes and tail follow.
    await page.evaluate(async (paneId) => {
        const { appendLineBatch } = await import('/lines.js');
        const rows = [];
        for (let n = 0; n < 300; n++) {
            for (const rawText of ['device> \x1b[8D\x1b[J', `redraw-log-${n}`]) {
                rows.push({ paneId, ts: 'T+00:00:01.000', rawText, isTx: false });
            }
        }
        appendLineBatch(rows);
    }, PANE);
    await expect(page.locator(`#log-${PANE}`)).toContainText('redraw-log-299');
    await expect(page.locator(`#log-${PANE} .log-line`, { hasText: 'device>' })).toHaveCount(0);
    // Previously persisted line objects do not carry the new classification.
    await page.evaluate(async (paneId) => {
        const { state } = await import('/state.js');
        const { buildStoredLine, rerenderPane } = await import('/lines.js');
        const legacy = buildStoredLine(paneId, '', 'device> \x1b[8D\x1b[J', false);
        delete legacy.transientConsoleRedraw;
        state.rawLines[paneId].push(legacy);
        rerenderPane(paneId);
    }, PANE);
    const filter = page.locator(`.filter-input[data-pane="${PANE}"]`);
    await filter.fill('device>');
    await expect(page.locator(`#log-${PANE} .log-line:visible`)).toHaveCount(2);
    await filter.fill('redraw-log-299');
    await expect(page.locator(`#log-${PANE} .log-line:visible`)).toHaveCount(1);
    await filter.fill('');
    await expect(page.locator(`#log-${PANE}`)).toContainText('redraw-log-299');
    await expect(page.locator(`#log-${PANE} .log-line`, { hasText: 'device>' })).toHaveCount(0);
});

test('a pane containing only redraws has no blank spacer rows and can resume', async ({ page }) => {
    await openSilentPane(page);
    await page.evaluate(async (paneId) => {
        const { appendLineBatch, clearPane } = await import('/lines.js');
        clearPane(paneId);
        appendLineBatch([{ paneId, ts: '', rawText: 'device> \x1b[8D\x1b[J', isTx: false }]);
    }, PANE);
    await expect(page.locator(`#log-${PANE} .log-line`)).toHaveCount(0);
    await page.evaluate(async (paneId) => {
        const { appendLineBatch } = await import('/lines.js');
        appendLineBatch([{ paneId, ts: '', rawText: 'response preserved', isTx: false }]);
    }, PANE);
    await expect(page.locator(`#log-${PANE} .log-line`)).toHaveCount(1);
    await expect(page.locator(`#log-${PANE}`)).toContainText('response preserved');
});

test('jumping to a hidden redraw row scrolls to the next visible row', async ({ page }) => {
    await openSilentPane(page);
    await page.evaluate(async (paneId) => {
        const { appendLineBatch, clearPane, scrollPaneToLineIdx } = await import('/lines.js');
        clearPane(paneId);
        const rows = [];
        for (let n = 0; n < 200; n++) {
            rows.push({ paneId, ts: '', rawText: `jump-before-${n}`, isTx: false, meta: { lineIdx: n } });
        }
        rows.push({ paneId, ts: '', rawText: 'device> \x1b[8D\x1b[J', isTx: false, meta: { lineIdx: 200 } });
        rows.push({ paneId, ts: '', rawText: 'jump-target', isTx: false, meta: { lineIdx: 201 } });
        for (let n = 0; n < 200; n++) {
            rows.push({ paneId, ts: '', rawText: `jump-after-${n}`, isTx: false, meta: { lineIdx: 202 + n } });
        }
        appendLineBatch(rows);
        scrollPaneToLineIdx(paneId, 200);
    }, PANE);
    await expect(page.locator(`#log-${PANE} .sync-highlight`)).toHaveText(/jump-target/);
});
