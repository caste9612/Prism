import { expect, test } from '@playwright/test';

// Requests of the Explorer menu (F18-AC1), sent the way the app receives them from the pipe
// (the simulated backend exposes a hook for the tests).

declare global {
  interface Window {
    __prismShellRequest?: (action: string, path: string) => void;
  }
}

test('F18-AC1: "Analyse with Prism" opens the space of the folder', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Disks', level: 1 })).toBeVisible();
  await page.waitForFunction(() => typeof window.__prismShellRequest === 'function');
  await page.evaluate(() => window.__prismShellRequest?.('analyze', 'D:\\Progetti'));
  await expect(page.getByRole('heading', { name: 'Data', level: 1 })).toBeVisible();
});

test('F18-AC1: "Find duplicates here" opens the duplicates filtered on the folder', async ({ page }) => {
  await page.goto('/');
  await page.waitForFunction(() => typeof window.__prismShellRequest === 'function');
  await page.evaluate(() => window.__prismShellRequest?.('duplicates', 'D:\\Foto'));
  await expect(page.getByRole('heading', { name: 'Duplicates', level: 1 })).toBeVisible();
  await expect(page.getByRole('textbox', { name: 'Path contains' })).toHaveValue('Foto');
  await expect(page.getByRole('combobox', { name: 'Disk' })).toHaveValue('2');
});

test('F18: a folder outside the catalog is explained', async ({ page }) => {
  await page.goto('/');
  await page.waitForFunction(() => typeof window.__prismShellRequest === 'function');
  await page.evaluate(() => window.__prismShellRequest?.('analyze', 'Q:\\Altro'));
  await expect(page.getByRole('alert')).toContainText('not on a disk of the catalog');
});
