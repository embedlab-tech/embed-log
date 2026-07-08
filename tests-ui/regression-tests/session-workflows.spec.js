import { expect, test } from '@playwright/test';
import { collectPageErrors, waitForLineContaining, waitForSourceTestLine } from './helpers.js';

async function currentSessionId(request) {
  const res = await request.get('/api/session/current');
  expect(res.ok()).toBeTruthy();
  const json = await res.json();
  return json.id;
}

async function openSettingsPanel(page) {
  const panel = page.locator('#settings-panel');
  if (!(await panel.evaluate(el => el.classList.contains('open')))) {
    await page.locator('#btn-settings').click();
  }
  await expect(panel).toHaveClass(/open/);
}

function currentHtmlButton(page) {
  return page.locator('#settings-panel button').filter({ hasText: /Open HTML|No HTML yet|HTML error/ }).first();
}

// Feature: session workflows — End-to-end tests for session management including HTML export, clean session rotation, and sessions popup
//
test.describe('session workflows', () => {
  let errors;

  test.beforeEach(async ({ page }) => {
    errors = collectPageErrors(page);
  });

  test.afterEach(async () => {
    expect(errors).toEqual([]);
  });

// Scenario: Current HTML button opens backend session export with correct pane data
//   Given an active session with a saved HTML export
//   When  the user clicks the "Open HTML" button in settings
//   Then  a new browser page opens the session export showing correct pane tabs and log content
//
  test('Current HTML opens backend session export with panes and logs', async ({ page, browser }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
    await waitForSourceTestLine(page, 'SENSOR_A');
    await openSettingsPanel(page);

    const exportResponse = page.waitForResponse(response =>
      response.url().includes('/api/session/export') && response.request().method() === 'POST',
      { timeout: 60_000 }
    );
    await page.locator('#btn-save-to-server').click();
    const response = await exportResponse;
    expect(response.ok()).toBeTruthy();

    const currentBtn = currentHtmlButton(page);
    await expect(currentBtn).toBeEnabled({ timeout: 60_000 });
    await expect(currentBtn).toHaveText('Open HTML', { timeout: 60_000 });

    await page.evaluate(() => {
      window.__openedUrls = [];
      const prev = window.open;
      window.open = function patched(url, target, features) {
        window.__openedUrls.push(String(url));
        return prev.call(this, url, target, features);
      };
    });

    await currentBtn.click();
    const openedUrl = await page.evaluate(() => window.__openedUrls?.at(-1) || null);
    expect(openedUrl).toMatch(/^\/sessions\/.+\/session\.html$/);

    const exported = await browser.newPage();
    await exported.goto(`http://127.0.0.1:8080${openedUrl}`);
    await expect(exported.getByRole('button', { name: 'DevA', exact: true })).toBeVisible();
    await expect(exported.getByRole('button', { name: 'DevB', exact: true })).toBeVisible();
    await exported.getByRole('button', { name: 'DevA', exact: true }).click();
    await expect(exported.locator('.pane-name', { hasText: 'DEVICE_A' }).first()).toBeVisible();
    await expect(exported.locator('.pane-name', { hasText: 'HOST' })).toBeVisible();
    await expect(exported.locator('.log-area', { hasText: 'TEST src=SENSOR_A' }).first()).toBeVisible();
    await exported.close();
  });

// Scenario: Clean session rotates session id and receives new logs
//   Given an active session with existing log lines
//   When  the user clicks "Clean session" and confirms the dialog
//   Then  the session id changes, old log lines disappear, and new test lines arrive
//
  test('Clean session rotates session id and receives new logs', async ({ page, request }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const oldId = await currentSessionId(request);
    const oldLine = await waitForLineContaining(page, 'SENSOR_A', 'kind=filter-alpha');
    const oldText = (await oldLine.textContent())?.trim();
    expect(oldText).toBeTruthy();

    await openSettingsPanel(page);
    page.once('dialog', dialog => dialog.accept());
    await page.locator('#btn-clean-session').click();

    await expect.poll(async () => currentSessionId(request), { timeout: 20_000 }).not.toBe(oldId);
    await expect(page.locator('#log-SENSOR_A .log-line', { hasText: oldText })).toHaveCount(0, { timeout: 20_000 });
    await waitForSourceTestLine(page, 'SENSOR_A');
  });

// Scenario: Sessions popup marks current session and exposes manifest/open-html links
//   Given a saved session
//   When  the user opens the sessions popup
//   Then  the current session row is tagged as current and contains manifest.json and session.html links
//
  test('Sessions popup marks current session and exposes manifest/open-html links', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
    await openSettingsPanel(page);

    await page.locator('#btn-save-to-server').click();
    const currentBtn = currentHtmlButton(page);
    await expect(currentBtn).toBeEnabled({ timeout: 20_000 });
    await expect(currentBtn).toHaveText('Open HTML', { timeout: 20_000 });

    await openSettingsPanel(page);
    await page.locator('#btn-sessions').click({ force: true });
    await expect(page.locator('#sessions-menu')).toHaveClass(/open/);

    const currentRow = page.locator('#sessions-menu .session-row', {
      has: page.locator('.session-tag.current'),
    }).first();

    await expect(currentRow).toBeVisible();
    await expect(currentRow.getByRole('link', { name: 'manifest', exact: true })).toHaveAttribute('href', /\/sessions\/.*\/manifest\.json$/);
    await expect(currentRow.getByRole('link', { name: 'open html', exact: true })).toHaveAttribute('href', /\/sessions\/.*\/session\.html$/);
  });
});
