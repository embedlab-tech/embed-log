import { expect, test } from '@playwright/test';

// These tests use real page widgets and control sockets, with a deterministic
// control peer so delayed acknowledgements and rejection can be exercised.
async function setup(page) {
  const states = {};
  for (const source of ['SENSOR_A', 'SENSOR_B']) {
    const configured = [
      { path: '/images/A/database.json', dictionary: { dictionary_id: 'a', build_id: 'A' }, error: null },
      { path: '/images/B/database.json', dictionary: { dictionary_id: 'b', build_id: 'B' }, error: null },
      { path: '/images/missing.json', dictionary: null, error: 'File not found' },
    ];
    states[source] = { generation: 0, parser_type: 'zephyr-dict', dictionary: configured[0].dictionary, configured };
  }
  let rejectNext = false;
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
      if (rejectNext) {
        rejectNext = false;
        ws.send(JSON.stringify({ id: request.id, ok: false, error: 'generation conflict' }));
      } else {
        status.generation++;
        status.dictionary = status.configured.find(item => item.dictionary?.dictionary_id === request.dictionary_id).dictionary;
        ws.send(JSON.stringify({ id: request.id, ok: true, status }));
      }
    });
  });
  await page.goto('/');
  return { states, requests, reject: () => { rejectNext = true; } };
}

test('configured choices wait for ACK, remain per-source, and survive reload/rebuild', async ({ page }) => {
  const { requests } = await setup(page);
  const first = page.getByLabel('Dictionary for SENSOR_A', { exact: true });
  const other = page.getByLabel('Dictionary for SENSOR_B', { exact: true });
  await expect(first).toHaveValue('a');
  await expect(first.locator('option', { hasText: 'missing' })).toHaveJSProperty('disabled', true);
  await first.selectOption('b');
  await expect(first).toBeDisabled();
  await expect(first).toHaveValue('a');
  await expect(first).toBeEnabled();
  await expect(first).toHaveValue('b');
  await expect(other).toHaveValue('a');
  expect(requests).toHaveLength(1);
  expect(requests[0]).toMatchObject({ type: 'dictionary.activate', source_id: 'SENSOR_A', expected_generation: 0 });
  await page.reload();
  await expect(page.getByLabel('Dictionary for SENSOR_A', { exact: true })).toHaveValue('b');
  await page.evaluate(async () => (await import('/tabcreate.js')).rebuildLayout());
  await expect(page.getByLabel('Dictionary for SENSOR_A', { exact: true })).toHaveCount(1);
  await expect(page.getByLabel('Dictionary for SENSOR_A', { exact: true })).toHaveValue('b');
});

test('rejected change preserves the active choice and displays the server error', async ({ page }) => {
  const peer = await setup(page);
  peer.reject();
  const select = page.getByLabel('Dictionary for SENSOR_A', { exact: true });
  await expect(select).toHaveValue('a');
  await select.selectOption('b');
  await expect(page.getByRole('alert').filter({ hasText: 'generation conflict' })).toBeVisible();
  await expect(select).toBeEnabled();
  await expect(select).toHaveValue('a');
  expect(peer.states.SENSOR_A.generation).toBe(0);
});
