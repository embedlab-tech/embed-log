import { defineConfig, devices } from '@playwright/test';

const defaultBaseURL = 'http://127.0.0.1:8080';
const baseURL = process.env.E2E_BASE_URL || defaultBaseURL;
const shouldStartDemo = process.env.E2E_START_DEMO !== '0' && baseURL === defaultBaseURL;

export default defineConfig({
  testDir: './tests',
  timeout: 45_000,
  expect: { timeout: 10_000 },
  fullyParallel: false,
  workers: process.env.CI ? 1 : undefined,
  retries: process.env.CI ? 1 : 0,
  reporter: [['list'], ['html', { open: 'never' }]],
  globalTeardown: './global-teardown.js',
  use: {
    baseURL,
    headless: true,
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure',
    acceptDownloads: true,
  },
  webServer: shouldStartDemo ? {
    command: 'node rust-demo-server.mjs',
    url: baseURL,
    timeout: 60_000,
    reuseExistingServer: false,
  } : undefined,
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    // Keep the broad suite Chromium-only; CI enables Firefox for the focused
    // refresh regression with E2E_FIREFOX=1.
    ...(process.env.E2E_FIREFOX === '1'
      ? [{ name: 'firefox', use: { ...devices['Desktop Firefox'] } }]
      : []),
    ...(process.env.E2E_EDGE === '1'
      ? [{ name: 'edge', use: { ...devices['Desktop Chrome'], channel: 'msedge' } }]
      : []),
  ],
});
