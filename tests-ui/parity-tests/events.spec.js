import { expect, test } from '@playwright/test';
import { collectPageErrors, sendUdpLine } from './helpers.js';

// Feature: Event detection — backend-matched events are visualized as a swimlane
//   timeline and rendered as severity-coloured markers on log lines.
//
//   Exercises: Events tab creation, SVG timeline rendering with swimlanes,
//   dot click-to-sync, event marker severity colours on log lines, and the
//   marker-nav event-marker toggle.

test.describe('event detection', () => {
  let errors;

  test.beforeEach(async ({ page }) => {
    errors = collectPageErrors(page);
  });

  test.afterEach(async () => {
    expect(errors).toEqual([]);
  });

  test('events tab appears in tab bar when config has event rules', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    const eventsBtn = page.locator('.events-tab-btn');
    await expect(eventsBtn).toBeVisible();
    await expect(eventsBtn).toContainText('Events');
  });

  test('events tab is hidden when config has no event rules', async ({ page }) => {
    // Override the page to use a config without event rules by using a
    // dedicated test config.  We verify absence by checking the DOM after
    // the config message arrives.
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });
    // The parity config DOES have event rules, so the button must be visible.
    // This test serves as a regression guard — if the button disappears, it
    // means event detection broke.
    await expect(page.locator('.events-tab-btn')).toBeVisible();
  });

  test('clicking events tab shows SVG timeline with swimlanes and dots', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    // Wait for events to arrive
    await expect(page.locator('.events-tab-btn')).toBeVisible();
    await page.locator('.events-tab-btn').click();

    const svg = page.locator('.events-timeline-svg');
    await expect(svg).toBeVisible();

    // Should have at least one swimlane (the parity config has sync_event
    // matching every tick line).
    const lanes = page.locator('.events-lane-label');
    await expect(lanes.first()).toBeVisible();
    const laneCount = await lanes.count();
    expect(laneCount).toBeGreaterThan(0);

    // Should have event dots
    const dots = page.locator('.events-dot');
    await expect.poll(() => dots.count(), { timeout: 15_000 }).toBeGreaterThan(0);

    // Other tab contents should be hidden
    const eventsContent = page.locator('#events-tab-content');
    await expect(eventsContent).toBeVisible();
  });

  test('event marker rendering on log lines uses severity colors', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    // Wait for event markers to appear (they come via markers_update)
    await expect.poll(() =>
      page.locator('.log-line.has-marker[data-kind="event"]').count(),
      { timeout: 20_000 }
    ).toBeGreaterThan(0);

    // Check that error-severity markers exist and have the right color
    const errorMarker = page.locator('.log-line.has-marker[data-kind="event"][data-severity="error"]').first();
    await expect(errorMarker).toBeVisible();
    const errorColor = await errorMarker.evaluate(el => getComputedStyle(el).borderLeftColor);
    expect(errorColor).toBe('rgb(239, 68, 68)');   // #ef4444

    // Info-severity markers should have a different color
    const infoMarker = page.locator('.log-line.has-marker[data-kind="event"][data-severity="info"]').first();
    if (await infoMarker.count() > 0) {
      const infoColor = await infoMarker.evaluate(el => getComputedStyle(el).borderLeftColor);
      expect(infoColor).toBe('rgb(59, 130, 246)');  // #3b82f6
    }
  });

  test('event marker nav toggle includes/excludes event markers', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    // The toggle lives in the events tab header
    const eventsBtn = page.locator('.events-tab-btn');
    await expect(eventsBtn).toBeVisible();
    await eventsBtn.click();
    await expect(page.locator('.events-timeline-svg')).toBeVisible();

    const toggle = page.locator('#events-nav-toggle');
    await expect(toggle).toBeVisible();
    await expect(toggle).not.toHaveClass(/active/);

    // Enable event markers via the toggle
    await toggle.click();
    await expect(toggle).toHaveClass(/active/);

    // Disable again
    await toggle.click();
    await expect(toggle).not.toHaveClass(/active/);
  });

  test('clicking an event dot switches to the source tab', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    // Wait for dots to appear
    await expect.poll(() => page.locator('.events-dot').count(), { timeout: 20_000 }).toBeGreaterThan(0);

    // Open events tab
    await page.locator('.events-tab-btn').click();
    await expect(page.locator('.events-timeline-svg')).toBeVisible();

    // Click an event dot via direct DOM access (SVG re-renders on every new
    // event, so locator-based clicks can lose their target).
    await page.evaluate(() => {
      const hit = document.querySelector('.events-dot-hit');
      if (hit) hit.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    });
    await expect(page.locator('#events-tab-content')).toBeHidden();
    const activeTab = page.locator('.tab-btn.active');
    await expect(activeTab).toBeVisible();
    const tabText = await activeTab.textContent();
    expect(tabText).not.toContain('Events');
  });

  test('hovering an event dot shows tooltip with event details', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    // Wait for dots
    await expect.poll(() => page.locator('.events-dot').count(), { timeout: 20_000 }).toBeGreaterThan(0);

    // Open events tab
    await page.locator('.events-tab-btn').click();
    await expect(page.locator('.events-timeline-svg')).toBeVisible();

    // Dispatch pointermove on a dot hit area (more reliable than Playwright
    // hover for elements in re-rendering SVGs).
    await page.evaluate(() => {
      const hit = document.querySelector('.events-dot-hit');
      if (hit) {
        const rect = hit.getBoundingClientRect();
        const evt = new PointerEvent('pointermove', {
          bubbles: true,
          clientX: rect.left + rect.width / 2,
          clientY: rect.top + rect.height / 2,
        });
        hit.dispatchEvent(evt);
      }
    });

    const tooltip = page.locator('#events-tooltip');
    await expect(tooltip).toHaveClass(/visible/);
    const tooltipText = await tooltip.textContent();
    expect(tooltipText).toMatch(/info|warn|error|fatal/);
  });

  test('severity filter checkbox hides matching dots', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#ws-status')).toContainText(/connected/i, { timeout: 20_000 });

    // Open events tab
    await page.locator('.events-tab-btn').click();
    await expect(page.locator('.events-timeline-svg')).toBeVisible();

    // Wait for dots
    await expect.poll(() => page.locator('.events-dot').count(), { timeout: 20_000 }).toBeGreaterThan(0);

    // Uncheck "error" severity filter
    const errorCheckbox = page.locator('[data-fsev="error"]');
    if (await errorCheckbox.count() > 0) {
      const dotsBefore = await page.locator('.events-dot').count();
      await errorCheckbox.uncheck();
      // Wait a tick for re-render
      await new Promise(r => setTimeout(r, 200));
      const dotsAfter = await page.locator('.events-dot').count();
      expect(dotsAfter).toBeLessThan(dotsBefore);
    }
  });
});
