import { spawnSync } from 'node:child_process';

const categories = {
  smoke: [
    'regression-tests/demo-smoke.spec.js',
    'regression-tests/deterministic-demo-coap.spec.js',
    'regression-tests/stats-display.spec.js',
    'regression-tests/timestamp-toggle.spec.js',
  ],
  data: [
    'regression-tests/cbor-decoder.spec.js',
    'regression-tests/network-capture.spec.js',
    'regression-tests/pane-plugin-coap.spec.js',
    'regression-tests/plugin-failure-isolation.spec.js',
  ],
  interaction: [
    'regression-tests/clipboard.spec.js',
    'regression-tests/drag-selection.spec.js',
    'regression-tests/filter-keyboard.spec.js',
    'regression-tests/layout-sync.spec.js',
    'regression-tests/scope-selection.spec.js',
  ],
  events: [
    'regression-tests/events.spec.js',
  ],
  sessions: [
    'regression-tests/export-replay.spec.js',
    'regression-tests/relative-time-replay.spec.js',
    'regression-tests/session-workflows.spec.js',
  ],
};

const categoryOrder = ['smoke', 'data', 'interaction', 'events', 'sessions'];
const usage = `usage: node regression-categories.mjs <category|all|list> [-- extra playwright args]\n\ncategories:\n${categoryOrder.map(name => `  ${name}`).join('\n')}`;

const rawArgs = process.argv.slice(2);
const separator = rawArgs.indexOf('--');
const args = separator >= 0 ? rawArgs.slice(0, separator) : rawArgs;
const extra = separator >= 0 ? rawArgs.slice(separator + 1) : [];
const command = args[0] || 'list';

function runCategory(name) {
  const files = categories[name];
  if (!files) {
    console.error(`unknown regression category: ${name}\n\n${usage}`);
    return 2;
  }

  console.log(`\n==> Running regression category: ${name}`);
  const npx = process.platform === 'win32' ? 'npx.cmd' : 'npx';
  const result = spawnSync(npx, [
    'playwright',
    'test',
    '--config=playwright.regression.config.js',
    ...files,
    ...extra,
  ], {
    stdio: 'inherit',
    shell: false,
  });

  if (result.error) {
    console.error(result.error.message);
    return 1;
  }
  return result.status ?? 1;
}

if (command === 'list' || command === '--list' || command === '-l') {
  console.log(usage);
  process.exit(0);
}

if (command === 'all') {
  for (const name of categoryOrder) {
    const code = runCategory(name);
    if (code !== 0) process.exit(code);
  }
  process.exit(0);
}

process.exit(runCategory(command));
