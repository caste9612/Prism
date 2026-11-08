import { expect, test } from '@playwright/test';
import { openCatalog } from './nav';

// Requests of the Explorer menu (F18-AC1), sent the way the app receives them from the pipe
// (the simulated backend exposes a hook for the tests).

declare global {
  interface Window {
    __prismShellRequest?: (action: string, path: string) => void;
  }
}

test.beforeEach(async ({ page }) => {
  await openCatalog(page);
  await page.waitForFunction(() => typeof window.__prismShellRequest === 'function');
});

test('F18-AC1: "Analyse with Prism" opens the space of the folder', async ({ page }) => {
  await page.evaluate(() => window.__prismShellRequest?.('analyze', 'D:\\Progetti'));
  await expect(page.getByRole('heading', { name: 'Data', level: 1 })).toBeVisible();
  const tabs = page.getByRole('navigation', { name: 'Functions of the disk' });
  await expect(tabs.getByRole('button', { name: 'Space' })).toHaveAttribute('aria-current', 'page');
});

test('F18-AC1: "Find duplicates here" opens the duplicates filtered on the folder', async ({ page }) => {
  await page.evaluate(() => window.__prismShellRequest?.('duplicates', 'D:\\Foto'));
  await expect(page.getByRole('heading', { name: 'Data', level: 1 })).toBeVisible();
  const tabs = page.getByRole('navigation', { name: 'Functions of the disk' });
  await expect(tabs.getByRole('button', { name: 'Duplicates' })).toHaveAttribute('aria-current', 'page');
  await expect(page.getByRole('textbox', { name: 'Path contains' })).toHaveValue('Foto');
});

test('F18: a folder outside the catalog is explained', async ({ page }) => {
  await page.evaluate(() => window.__prismShellRequest?.('analyze', 'Q:\\Altro'));
  await expect(page.getByRole('alert')).toContainText('not on a disk of the catalog');
});
