import { expect, test, type Page } from '@playwright/test';
import { card, openCatalog, openDisk, openWhole } from './nav';

// The backup screen with the simulated backend (src/lib/mock-backup.ts): a check finds 5 files
// without a copy (in Foto\2024, Foto\2023 and Documenti), the copy of 5 files ends with one
// failure, a comparison has one file of each kind plus a renamed one.

async function openBackup(page: Page) {
  await openCatalog(page);
  await openWhole(page, 'Backup');
  await expect(page.getByRole('tab', { name: 'Check a backup' })).toHaveAttribute('aria-selected', 'true');
}

const side = (page: Page, name: string) => page.getByRole('group', { name, exact: true });

async function choose(page: Page, group: string, disk: string, folder = '') {
  const g = side(page, group);
  await g.getByRole('combobox', { name: 'Disk' }).selectOption({ label: disk });
  if (folder) await g.getByRole('textbox', { name: 'Folder' }).fill(folder);
}

test('F13-AC1/AC2/AC3: files without a copy, against a disconnected backup', async ({ page }) => {
  await openBackup(page);
  await choose(page, 'Files to protect', 'Data', 'Foto');
  await choose(page, 'Backup 1', 'Backup Foto (not connected)');
  await page.getByRole('button', { name: 'Check', exact: true }).click();
  const result = page.getByRole('status', { name: 'Result for Data \\ Foto' });
  await expect(result).toContainText('2,410 files to protect, 18.4 GB');
  await expect(result).toContainText('With a copy verified by content: 2,105 (87%)');
  await expect(result).toContainText('With a copy found by name, size and date: 300 (12%)');
  await expect(result).toContainText('Without a copy: 5 files, 16.3 MB');
  await page.screenshot({ path: 'test-results/backup-verify.png', fullPage: true });
});

test('F13-AC4: missing files by folder, with search', async ({ page }) => {
  await openBackup(page);
  await choose(page, 'Backup 1', 'Backup Foto (not connected)');
  await page.getByRole('button', { name: 'Check', exact: true }).click();
  await expect(page.getByRole('button', { name: /^Foto$/ })).toBeVisible();
  await expect(page.getByRole('navigation', { name: 'Folder' }).getByText('5 files · 16.3 MB')).toBeVisible();
  await page.getByRole('button', { name: /^Foto$/ }).click();
  await expect(page.getByRole('button', { name: /^2024$/ })).toBeVisible();
  await page.getByRole('button', { name: /^2024$/ }).click();
  await expect(page.getByText('IMG_0001.jpg')).toBeVisible();
  await expect(page.getByText('3 files · 13.1 MB')).toBeVisible();
  await page.getByRole('textbox', { name: 'Search in the paths' }).fill('vacanza');
  await page.getByRole('textbox', { name: 'Search in the paths' }).press('Enter');
  await expect(page.getByText('1 file · 5.2 MB')).toBeVisible();
});

test('F13-AC5, F15-AC2/AC5: copy of the missing files with preview and outcome', async ({ page }) => {
  await openBackup(page);
  await choose(page, 'Files to protect', 'Data');
  await choose(page, 'Backup 1', 'Backup Foto (not connected)');
  await page.getByRole('button', { name: 'Check', exact: true }).click();
  await page.getByRole('button', { name: 'Copy the missing files' }).click();
  const dialog = page.getByRole('dialog', { name: 'Copy the missing files' });
  await expect(dialog).toContainText('never overwritten');
  // The backup disk is not connected: another connected disk is proposed.
  await choose(page, 'Copy to', 'NAS Media', 'Copia');
  await expect(dialog).toContainText('5 files, 16.3 MB, to Y:\\Copia');
  await dialog.getByRole('radio', { name: /Copy with another name/ }).check();
  await dialog.getByRole('button', { name: 'Copy', exact: true }).click();
  await expect(page.getByText(/Copying: /)).toBeVisible();
  const outcome = page.getByRole('status', { name: 'Copy result' });
  await expect(outcome).toContainText('Copied and checked: 4 (16.3 MB).', { timeout: 10_000 });
  await expect(outcome).toContainText('Not copied: 1');
  await expect(outcome).toContainText('Access denied');
  await expect(outcome.getByRole('button', { name: 'Resume the copy' })).toBeVisible();
  // The check runs again by itself: only the file that failed is still missing.
  await expect(page.getByRole('status', { name: 'Result for Data' })).toContainText('Without a copy: 1 files, 1.2 MB');
});

test('F15-AC1: no copy from a disconnected source', async ({ page }) => {
  await openBackup(page);
  await choose(page, 'Files to protect', 'Backup Foto (not connected)');
  await choose(page, 'Backup 1', 'Data');
  await page.getByRole('button', { name: 'Check', exact: true }).click();
  await page.getByRole('button', { name: 'Copy the missing files' }).click();
  const dialog = page.getByRole('dialog', { name: 'Copy the missing files' });
  await expect(dialog.getByRole('alert')).toContainText('The disk is not connected.');
  await expect(dialog.getByRole('button', { name: 'Copy', exact: true })).toBeDisabled();
});

test('F14-AC1/AC2/AC3: comparison by content finds the renamed file; tree by status', async ({ page }) => {
  await openBackup(page);
  await page.getByRole('tab', { name: 'Compare' }).click();
  await choose(page, 'Folder A', 'Data');
  await choose(page, 'Folder B', 'NAS Media');
  await page.getByRole('button', { name: 'Compare', exact: true }).click();
  const filters = page.getByRole('group', { name: 'Show' });
  await expect(filters.getByText('Only in A').locator('..')).toContainText('1');
  await expect(filters.getByText('Identical').locator('..')).toContainText('2');
  // Identical files are hidden at first; showing them reveals the renamed copy.
  await filters.getByRole('checkbox', { name: /Identical/ }).check();
  await page.getByRole('button', { name: /^Foto$/ }).click();
  await page.getByRole('button', { name: /^2023$/ }).click();
  await expect(page.getByText('in B: Archivio\\monti-2023.jpg')).toBeVisible();
  await expect(page.getByText('by content').first()).toBeVisible();
  await page.screenshot({ path: 'test-results/backup-compare.png', fullPage: true });

  // By path the renamed file counts as only in A and only in B.
  await page.getByRole('radio', { name: 'by path' }).check();
  await page.getByRole('button', { name: 'Compare', exact: true }).click();
  await expect(filters.getByText('Only in B').locator('..')).toContainText('2');
});

test('the backup tab of a disk proposes it as the folder to protect', async ({ page }) => {
  await openCatalog(page);
  await expect(card(page, 'Data')).toBeVisible();
  await openDisk(page, 'Data', 'Backup');
  await expect(side(page, 'Files to protect').getByRole('combobox', { name: 'Disk' })).toHaveValue('2');
});
