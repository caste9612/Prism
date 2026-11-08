import { expect, test } from '@playwright/test';
import { backToCatalog, card, diskTab, openCatalog, openDisk, openSettings } from './nav';

// Diagnostics (F20-AC2), reset of the catalog (F21-AC4) and updates (F22-AC3) in the settings,
// with the simulated backend.

test.beforeEach(async ({ page }) => {
  await openCatalog(page);
  await openSettings(page);
});

test('F20-AC2: the diagnostic package shows its content before it is saved', async ({ page }) => {
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
  await page.getByRole('button', { name: 'Reset the catalog…' }).click();
  const dialog = page.getByRole('dialog', { name: 'Reset the catalog?' });
  await expect(dialog).toContainText('No file on your disks is touched.');
  const reset = dialog.getByRole('button', { name: 'Reset', exact: true });
  await expect(reset).toBeDisabled();
  await dialog.getByRole('checkbox').check();
  await reset.click();
  await expect(page.getByRole('alert')).toContainText('Catalog reset: 5 disks removed.');
  await backToCatalog(page);
  await expect(page.getByText('No disks yet.')).toBeVisible();
});

test('F22-AC3: a downloaded update is installed only after the user confirms', async ({ page }) => {
  await expect(page.getByText('Prism 2.0.1 is ready to install.')).toBeVisible();
  await expect(page.getByText('Fixes for the backup check.')).toBeVisible();
  await page.getByRole('button', { name: 'Install and restart…' }).click();
  const dialog = page.getByRole('dialog', { name: 'Install Prism 2.0.1?' });
  await expect(dialog).toContainText('Prism closes, installs the new version and starts again.');
  await dialog.getByRole('button', { name: 'Cancel' }).click();
  await expect(page.getByText('Prism 2.0.1 is ready to install.')).toBeVisible();
  await page.getByRole('button', { name: 'Install and restart…' }).click();
  await page.getByRole('dialog', { name: 'Install Prism 2.0.1?' }).getByRole('button', { name: 'Install and restart' }).click();
  await expect(page.getByText('Prism is up to date.')).toBeVisible();
});

test('the sections of the settings are listed beside them', async ({ page }) => {
  const sections = page.getByRole('navigation', { name: 'Sections' });
  await expect(sections.getByRole('link', { name: 'Content analysis' })).toBeVisible();
  await sections.getByRole('link', { name: 'Diagnostics' }).click();
  await expect(page.getByRole('button', { name: 'Open the log folder' })).toBeInViewport();
});

test('D66: a NAS is connected with its login, checked first, and disconnected', async ({ page }) => {
  const nas = page.locator('#settings-nas');
  await expect(nas).toContainText('saved encrypted in the Windows Credential Manager');
  const row = nas.getByRole('listitem').filter({ hasText: 'DISKSTATION · 192.168.1.20' });
  await expect(row).toContainText('1 shared folder · Connected as admin');
  await expect(row).toContainText('Healthy · 2 volumes · 4 disks');
  await row.getByRole('button', { name: 'Disconnect' }).click();
  await expect(row).toContainText('Not connected');

  // Without the login the share has no RAID, and its health tab says how to give it.
  await backToCatalog(page);
  await expect(card(page, 'NAS Media').getByRole('button', { name: /RAID/ })).toHaveCount(0);
  await openDisk(page, 'NAS Media', 'Health');
  await expect(page.getByText('shows the RAID of this shared folder')).toBeVisible();
  await page.getByRole('button', { name: 'Open the settings' }).click();

  await page.locator('#settings-nas').getByRole('button', { name: 'Connect…' }).click();
  const dialog = page.getByRole('dialog', { name: 'Connect to DISKSTATION · 192.168.1.20' });
  const connect = dialog.getByRole('button', { name: 'Connect', exact: true });
  await expect(connect).toBeDisabled();
  await dialog.getByLabel('User').fill('admin');
  await dialog.getByLabel('Password').fill('wrong');
  await connect.click();
  await expect(dialog.getByRole('alert')).toHaveText('Wrong user or password.');
  await expect(dialog.getByLabel('Password')).toHaveValue('');
  await dialog.getByLabel('Password').fill('right');
  await connect.click();
  await expect(dialog).toHaveCount(0);
  await expect(page.locator('#settings-nas')).toContainText('Connected as admin');
  await backToCatalog(page);
  await expect(card(page, 'NAS Media').getByRole('button', { name: 'RAID 1 · OK' })).toBeVisible();
  await openDisk(page, 'NAS Media');
  await diskTab(page, 'Health');
  await expect(page.getByRole('region', { name: 'Media' })).toContainText('Healthy');
});