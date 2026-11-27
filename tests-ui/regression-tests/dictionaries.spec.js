import { expect, test } from '@playwright/test';

// These tests use real page widgets and control sockets, with a deterministic
// control peer so delayed acknowledgements and rejection can be exercised.
const option = (page, source, id) => page.locator(`#pane-${source} .dd-option[data-value="${id}"]`);
const chooseDictionary = async (page, source, id) => {
  await page.getByLabel(`Dictionary for ${source}`, { exact: true }).click();
  await option(page, source, id).click();
};

async function setup(page, firstCount = 3, secondCount = 3) {
  const states = {};
  for (const source of ['SENSOR_A', 'SENSOR_B']) {
    const configured = [
      { path: '/images/A/database.json', dictionary: { dictionary_id: 'a', build_id: 'A' }, error: null },
      { path: '/images/B/database.json', dictionary: { dictionary_id: 'b', build_id: 'B' }, error: null },
      { path: '/images/missing.json', dictionary: null, error: 'File not found' },
    ];
    configured.splice(source === 'SENSOR_A' ? firstCount : secondCount);
    states[source] = { generation: 0, parser_type: 'zephyr-dict', dictionary: configured[0].dictionary, active_index: 0, configured };
  }
  let rejectNext = false;
  let reloadHandler = null;
  const requests = [];
  await page.route('**/api/v1/status', route => route.fulfill({ json: {
    ok: true, control_api: true, session_id: 'test-session',
    sources: Object.fromEntries(Object.entries(states).map(([source, dictionary]) => [source, { dictionary }])),
  } }));
  await page.routeWebSocket('**/api/v1/control', ws => {
    ws.onMessage(async message => {
      const request = JSON.parse(message);
      requests.push(request);
      await new Promise(resolve => setTimeout(resolve, 350));
      const status = states[request.source_id];
      if (request.type === 'dictionary.reload') {
        const reply = reloadHandler ? reloadHandler(status, request) : { reloaded: false };
        ws.send(JSON.stringify(reply.error
          ? { id: request.id, ok: false, error: reply.error }
          : { id: request.id, ok: true, reloaded: reply.reloaded, dictionary: status.dictionary, status }));
        return;
      }
      if (rejectNext) {
        rejectNext = false;
        ws.send(JSON.stringify({ id: request.id, ok: false, error: 'generation conflict' }));
      } else {
        status.generation++;
        status.dictionary = status.configured[request.configured_index].dictionary;
        status.active_index = request.configured_index;
        ws.send(JSON.stringify({ id: request.id, ok: true, status }));
      }
    });
  });
  await page.goto('/');
  return {
    states, requests,
    reject: () => { rejectNext = true; },
    onReload: handler => { reloadHandler = handler; },
  };
}

test('configured choices wait for ACK, remain per-source, and survive reload/rebuild', async ({ page }) => {
  const { requests } = await setup(page);
  const first = page.getByLabel('Dictionary for SENSOR_A', { exact: true });
  const other = page.getByLabel('Dictionary for SENSOR_B', { exact: true });
  await expect(first).toHaveAttribute('data-value', '0');
  await first.click();
  await expect(page.locator('#pane-SENSOR_A .dd-option', { hasText: 'missing' })).toHaveAttribute('aria-disabled', 'true');
  await option(page, 'SENSOR_A', '1').click();
  await expect(first).toBeDisabled();
  await expect(first).toHaveAttribute('data-value', '0');
  await expect(first).toBeEnabled();
  await expect(first).toHaveAttribute('data-value', '1');
  await expect(other).toHaveAttribute('data-value', '0');
  expect(requests).toHaveLength(1);
  expect(requests[0]).toMatchObject({ type: 'dictionary.activate', source_id: 'SENSOR_A', expected_generation: 0 });
  await page.reload();
  await expect(page.getByLabel('Dictionary for SENSOR_A', { exact: true })).toHaveAttribute('data-value', '1');
  await page.evaluate(async () => (await import('/tabcreate.js')).rebuildLayout());
  await expect(page.getByLabel('Dictionary for SENSOR_A', { exact: true })).toHaveCount(1);
  await expect(page.getByLabel('Dictionary for SENSOR_A', { exact: true })).toHaveAttribute('data-value', '1');
});

test('single configured dictionary has no selector but can be reloaded, including after a status update', async ({ page }) => {
  const { states } = await setup(page, 1);
  const single = page.getByLabel('Dictionary for SENSOR_A', { exact: true });
  const multiple = page.getByLabel('Dictionary for SENSOR_B', { exact: true });
  await expect(multiple).toHaveAttribute('data-value', '0');
  await expect(single).toHaveCount(0);
  await page.evaluate(async () => (await import('/tabcreate.js')).rebuildLayout());
  await expect(multiple).toHaveCount(1);
  await expect(single).toHaveCount(0);
  await expect(page.locator('#pane-SENSOR_A .dictionary-reload')).toBeEnabled();
  states.SENSOR_B.configured.splice(1);
  await expect(multiple).toHaveCount(0, { timeout: 7000 });
});

test('rejected change preserves the active choice and displays the server error', async ({ page }) => {
  const peer = await setup(page);
  peer.reject();
  const select = page.getByLabel('Dictionary for SENSOR_A', { exact: true });
  await expect(select).toHaveAttribute('data-value', '0');
  await chooseDictionary(page, 'SENSOR_A', '1');
  await expect(page.getByRole('alert').filter({ hasText: 'generation conflict' })).toBeVisible();
  await expect(select).toBeEnabled();
  await expect(select).toHaveAttribute('data-value', '0');
  expect(peer.states.SENSOR_A.generation).toBe(0);
});

test('Reload re-reads the active file, reports the result and keeps the choice on failure', async ({ page }) => {
  const peer = await setup(page, 1);
  const button = page.locator('#pane-SENSOR_A .dictionary-reload');
  const note = page.locator('#pane-SENSOR_A .dictionary-pill');
  await expect(button).toBeEnabled();

  // Unchanged file: no new generation.
  await button.click();
  await expect(button).toBeDisabled();
  await expect(note).toHaveText('Already up to date');
  expect(peer.requests.at(-1)).toMatchObject({ type: 'dictionary.reload', source_id: 'SENSOR_A', expected_generation: 0 });
  expect(peer.states.SENSOR_A.generation).toBe(0);

  // Rebuilt file: the backend applies it and the pane follows the new binding.
  peer.onReload(status => {
    status.generation++;
    status.dictionary = { dictionary_id: 'a2', build_id: 'A2' };
    status.configured[0].dictionary = status.dictionary;
    return { reloaded: true };
  });
  await button.click();
  await expect(note).toHaveText('Reloaded · build A2');
  await expect(button).toBeEnabled();
  await button.click();
  expect(peer.requests.at(-1)).toMatchObject({ expected_generation: 1 });

  // Invalid file: error shown, active dictionary unchanged.
  peer.onReload(() => ({ error: 'cannot read /images/A/database.json: No such file or directory' }));
  await button.click();
  await expect(page.getByRole('alert').filter({ hasText: 'cannot read' })).toBeVisible();
  await expect(note).toBeHidden();
  await expect(page.locator('#pane-SENSOR_A .dictionary-name')).toHaveAttribute('title', '/images/A/database.json');
  expect(peer.states.SENSOR_A.generation).toBe(1);
});

test('Reload is disabled when the active dictionary was uploaded, not read from a file', async ({ page }) => {
  const { states } = await setup(page, 1);
  states.SENSOR_A.dictionary = { dictionary_id: 'uploaded', build_id: null };
  states.SENSOR_A.active_index = null; // not backed by a configured file
  await expect(page.locator('#pane-SENSOR_A .dictionary-reload')).toBeDisabled({ timeout: 7000 });
});

test('a rebuilt file is hinted, auto-reload is labelled, and its failure is shown', async ({ page }) => {
  const { states } = await setup(page, 1);
  const pane = page.locator('#pane-SENSOR_A');
  await expect(pane.locator('.dictionary-pill')).toHaveText('');
  await expect(pane.locator('.dictionary-reload')).not.toHaveClass(/attention/);

  states.SENSOR_A.configured[0].changed_on_disk = true;
  await expect(pane.locator('.dictionary-pill')).toHaveText('File changed — reload to use it', { timeout: 7000 });
  await expect(pane.locator('.dictionary-reload')).toHaveClass(/attention/);

  // With parser.watch the backend reloads by itself: no call to action.
  states.SENSOR_A.watch = true;
  await expect(pane.locator('.dictionary-pill')).toHaveText('File changed · reloading…', { timeout: 7000 });
  await expect(pane.locator('.dictionary-reload')).not.toHaveClass(/attention/);
  states.SENSOR_A.configured[0].changed_on_disk = false;
  await expect(pane.locator('.dictionary-pill')).toHaveText('Auto-reload on', { timeout: 7000 });

  states.SENSOR_A.watch_error = 'images/A/database.json: invalid dictionary';
  await expect(pane.getByRole('alert')).toHaveText('Auto-reload failed: images/A/database.json: invalid dictionary', { timeout: 7000 });
});

test('a stale "Already up to date" does not hide that the file changed afterwards', async ({ page }) => {
  const { states } = await setup(page, 1);
  const pane = page.locator('#pane-SENSOR_A');
  const pill = pane.locator('.dictionary-pill');
  await pane.locator('.dictionary-reload').click();
  await expect(pill).toHaveText('Already up to date');
  // A rebuild lands right after: the hint replaces the old result at once.
  states.SENSOR_A.configured[0].changed_on_disk = true;
  await expect(pill).toHaveText('File changed — reload to use it', { timeout: 7000 });
  await expect(pane.locator('.dictionary-reload')).toHaveClass(/attention/);
});

test('the dictionary row shows the file name and build, with the full path on hover', async ({ page }) => {
  await setup(page, 1);
  const pane = page.locator('#pane-SENSOR_A');
  await expect(pane.locator('.dictionary-name')).toHaveText('database.json');
  await expect(pane.locator('.dictionary-name')).toHaveAttribute('title', '/images/A/database.json');
  await expect(pane.locator('.dictionary-meta')).toHaveText('build A · #a');
  // Same file name in two folders: the folder is shown, so the choices differ.
  await page.getByLabel('Dictionary for SENSOR_B', { exact: true }).click();
  const names = page.locator('#pane-SENSOR_B .dd-option-name');
  await expect(names.nth(0)).toHaveText('A/database.json');
  await expect(names.nth(1)).toHaveText('B/database.json');
  await expect(page.locator('#pane-SENSOR_B .dd-option-build').nth(1)).toHaveText('build B');
  await expect(option(page, 'SENSOR_B', '0')).toHaveAttribute('aria-selected', 'true');
});

test('the dictionary menu works from the keyboard and closes on Escape or an outside click', async ({ page }) => {
  const { requests } = await setup(page);
  const trigger = page.getByLabel('Dictionary for SENSOR_A', { exact: true });
  const menu = page.locator('#pane-SENSOR_A .dd-menu');
  await trigger.focus();
  await page.keyboard.press('ArrowDown');
  await expect(menu).toBeVisible();
  await expect(option(page, 'SENSOR_A', '0')).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(menu).toBeHidden();
  await expect(trigger).toBeFocused();
  await trigger.click();
  await page.mouse.click(5, 300);
  await expect(menu).toBeHidden();
  await trigger.click();
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Enter');
  await expect(menu).toBeHidden();
  await expect(trigger).toHaveAttribute('data-value', '1');
  expect(requests).toHaveLength(1);
  expect(requests[0]).toMatchObject({ type: 'dictionary.activate', dictionary_id: 'b', configured_index: 1 });
});

test('messages appear to the right of Reload and never move it or the selector', async ({ page }) => {
  const { states } = await setup(page);
  const reload = page.locator('#pane-SENSOR_A .dictionary-reload');
  const trigger = page.getByLabel('Dictionary for SENSOR_A', { exact: true });
  const pill = page.locator('#pane-SENSOR_A .dictionary-pill');
  const x = async locator => (await locator.boundingBox()).x;
  const before = [await x(trigger), await x(reload)];

  await reload.click();
  await expect(pill).toHaveText('Already up to date');
  expect(await x(pill)).toBeGreaterThan(await x(reload));
  expect([await x(trigger), await x(reload)]).toEqual(before);

  states.SENSOR_A.configured[0].changed_on_disk = true;
  await expect(pill).toHaveText('File changed — reload to use it', { timeout: 7000 });
  expect([await x(trigger), await x(reload)]).toEqual(before);

  // Switching to a dictionary with a different name does not resize the selector either.
  await chooseDictionary(page, 'SENSOR_A', '1');
  await expect(trigger).toHaveAttribute('data-value', '1');
  expect([await x(trigger), await x(reload)]).toEqual(before);
});

test('two files with identical bytes are chosen by entry, and the choice is reported', async ({ page }) => {
  const peer = await setup(page);
  // debug.json and release.json start with the same bytes, so they share one dictionary_id.
  for (const status of [peer.states.SENSOR_A]) {
    status.configured[1].dictionary = { ...status.configured[0].dictionary };
  }
  await page.reload();
  const trigger = page.getByLabel('Dictionary for SENSOR_A', { exact: true });
  await expect(trigger).toHaveAttribute('data-value', '0');
  await chooseDictionary(page, 'SENSOR_A', '1');
  // Not a no-op: the second entry is chosen, even though the id is the same.
  await expect(trigger).toHaveAttribute('data-value', '1');
  expect(peer.requests).toHaveLength(1);
  expect(peer.requests[0]).toMatchObject({ type: 'dictionary.activate', dictionary_id: 'a', configured_index: 1 });
  await expect(page.locator('#pane-SENSOR_A .dictionary-name, #pane-SENSOR_A .dd-trigger-name').first()).toContainText('B/database.json');
});

test('an open dictionary list survives status refreshes of another UART', async ({ page }) => {
  // SENSOR_A has a list, SENSOR_B only one dictionary and so no list.
  await setup(page, 3, 1);
  const trigger = page.getByLabel('Dictionary for SENSOR_A', { exact: true });
  await trigger.click();
  const menu = page.locator('#pane-SENSOR_A .dd-menu');
  await expect(menu).toBeVisible();
  await page.waitForTimeout(7000); // two status refreshes (every 3 s)
  await expect(menu).toBeVisible();
  await expect(trigger).toHaveAttribute('aria-expanded', 'true');
});
