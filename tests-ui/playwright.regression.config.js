import { defineConfig, devices } from '@playwright/test';

const defaultBaseURL = 'http://127.0.0.1:8080';
const baseURL = process.env.E2E_BASE_URL || defaultBaseURL;
const shouldStartServer = process.env.E2E_START_SERVER !== '0' && baseURL === defaultBaseURL;

export default defineConfig({
  testDir: './regression-tests',
  timeout: 45_000,
  expect: { timeout: 10_000 },
  fullyParallel: false,
  workers: process.env.CI ? 1 : undefined,
  retries: process.env.CI ? 1 : 0,
  reporter: [['list'], ['html', { open: 'never' }]],
  use: {
    baseURL,
    headless: true,
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure',
    acceptDownloads: true,
  },
  webServer: shouldStartServer ? {
    command: 'node rust-test-server.mjs --regression',
    url: baseURL,
    timeout: 60_000,
    reuseExistingServer: false,
  } : undefined,
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    { name: 'edge', use: { ...devices['Desktop Edge'], channel: 'msedge' } },
  ],
});
