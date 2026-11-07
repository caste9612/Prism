import { expect, test, type Page } from '@playwright/test';

// Diagnostics (F20-AC2) and reset of the catalog (F21-AC4) in the settings, with the simulated
// backend.

async function openSettings(page: Page) {
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Settings', level: 1 })).toBeVisible();
}

test('F20-AC2: the diagnostic package shows its content before it is saved', async ({ page }) => {
  await openSettings(page);
  await expect(page.getByRole('button', { name: 'Open the log folder' })).toBeVisible();
  await page.getByRole('button', { name: 'Export a diagnostic package…' }).click();
  const dialog = page.getByRole('dialog', { name: 'Diagnostic package' });
  await expect(dialog).toContainText('look at them before sending the package');
  await expect(dialog).toContainText('logs/prism.2026-11-07.log');
  await expect(dialog).toContainText('system.txt');
  await expect(dialog.getByLabel('system.txt')).toContainText('Windows 11 Pro');
  await expect(dialog).toContainText('It will be saved in C:\\Users\\me\\Downloads.');
  await dialog.getByRole('button', { name: 'Export' }).click();
  await expect(page.getByRole('status')).toContainText('Saved: C:\\Users\\me\\Downloads\\prism-diagnostics-');
});

test('F21-AC4: the catalog is reset only after an explicit confirmation', async ({ page }) => {
  await openSettings(page);
  await page.getByRole('button', { name: 'Reset the catalog…' }).click();
  const dialog = page.getByRole('dialog', { name: 'Reset the catalog?' });
  await expect(dialog).toContainText('No file on your disks is touched.');
  const reset = dialog.getByRole('button', { name: 'Reset', exact: true });
  await expect(reset).toBeDisabled();
  await dialog.getByRole('checkbox').check();
  await reset.click();
  await expect(page.getByRole('alert')).toContainText('Catalog reset: 5 disks removed.');
  await page.getByRole('button', { name: 'Disks', exact: true }).click();
  await expect(page.getByText('No disks yet.')).toBeVisible();
});

test('F22-AC3: a downloaded update is installed only after the user confirms', async ({ page }) => {
  await openSettings(page);
  await expect(page.getByText('Prism 1.0.1 is ready to install.')).toBeVisible();
  await expect(page.getByText('Fixes for the backup check.')).toBeVisible();
  await page.getByRole('button', { name: 'Install and restart…' }).click();
  const dialog = page.getByRole('dialog', { name: 'Install Prism 1.0.1?' });
  await expect(dialog).toContainText('Prism closes, installs the new version and starts again.');
  await dialog.getByRole('button', { name: 'Cancel' }).click();
  await expect(page.getByText('Prism 1.0.1 is ready to install.')).toBeVisible();
  await page.getByRole('button', { name: 'Install and restart…' }).click();
  await page.getByRole('dialog', { name: 'Install Prism 1.0.1?' }).getByRole('button', { name: 'Install and restart' }).click();
  await expect(page.getByText('Prism is up to date.')).toBeVisible();
});
