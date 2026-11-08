import { expect, test } from '@playwright/test';
import { openCatalog, openDisk, openSettings } from './nav';

// The smallest window Prism allows (tauri.conf.json: 960 × 640): every screen fits its width,
// the top bar, the status bar and the action bar stay on one row.

test.use({ viewport: { width: 960, height: 640 } });

const rowHeight = async (selector: string, page: import('@playwright/test').Page) =>
  (await page.locator(selector).first().boundingBox())!.height;

test('the screens fit the smallest window', async ({ page }) => {
  await openCatalog(page);
  const overflow = () => page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(await overflow()).toBe(0);
  expect(await rowHeight('header.topbar', page)).toBeLessThan(70);
  expect(await rowHeight('footer.statusbar', page)).toBeLessThan(32);

  await openDisk(page, 'Data', 'Duplicates');
  expect(await overflow()).toBe(0);
  expect(await rowHeight('header.topbar', page)).toBeLessThan(70);
  const bar = page.getByRole('region', { name: /copies selected/ });
  await expect(bar).toBeVisible();
  expect((await bar.boundingBox())!.height).toBeLessThan(70);
  await expect(bar.getByRole('button', { name: /Delete selected copies/ })).toBeInViewport();

  for (const tab of ['Space', 'Similar', 'Backup', 'Cleanup', 'Health', 'File types']) {
    await page.getByRole('navigation', { name: 'Functions of the disk' }).getByRole('button', { name: tab, exact: true }).click();
    expect(await overflow()).toBe(0);
  }
  await openSettings(page);
  expect(await overflow()).toBe(0);
  await page.screenshot({ path: 'test-results/layout-960.png' });
});
