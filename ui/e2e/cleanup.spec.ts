import { expect, test, type Page } from '@playwright/test';
import { openCatalog, openDisk } from './nav';

// The cleanup tab of a disk with the simulated backend (src/lib/mock-cleanup.ts): on "Windows"
// old temporary files, empty folders, large old files and the caches of Chrome (closed) and
// Firefox (running); on the network drive a large old file (no Recycle Bin there).

async function openCleanup(page: Page, disk = 'Windows') {
  await openCatalog(page);
  await openDisk(page, disk, 'Cleanup');
}

const group = (page: Page, name: RegExp) => page.getByRole('article', { name });

test('F09-AC1: categories with explanation and total', async ({ page }) => {
  await openCleanup(page);
  const temp = group(page, /^Temporary files · Windows/);
  await expect(temp).toContainText('older than one day');
  await expect(temp).toContainText('452 MB');
  await expect(group(page, /^Empty folders · Windows/)).toContainText('empty');
  await expect(group(page, /^Chrome cache · Windows/)).toContainText('cookies, passwords and history are never touched');
  await expect(page.getByText('Recycle Bin of Windows: 12 items, 1.3 GB.')).toBeVisible();
  // Only the proposals of this disk.
  await expect(group(page, /NAS Big-Storage/)).toHaveCount(0);
  await page.screenshot({ path: 'test-results/cleanup.png', fullPage: true });
});

test('F09-AC3: everything can be deselected; large old files start unselected', async ({ page }) => {
  await openCleanup(page);
  const temp = group(page, /^Temporary files · Windows/);
  await expect(temp.getByRole('button', { name: 'Move to the Recycle Bin (452 MB)' })).toBeEnabled();
  await temp.getByRole('checkbox').nth(1).uncheck();
  await expect(temp.getByRole('button', { name: 'Move to the Recycle Bin (62.0 MB)' })).toBeEnabled();
  await temp.getByRole('button', { name: 'Select all' }).click();
  await expect(temp.getByRole('button', { name: 'Move to the Recycle Bin (452 MB)' })).toBeEnabled();

  const large = group(page, /^Large and old files · Windows/);
  await expect(large.getByRole('button', { name: /Move to the Recycle Bin/ })).toBeDisabled();
  for (const box of await large.getByRole('checkbox').all()) await expect(box).not.toBeChecked();
});

test('F09-AC4: the cache of a running browser cannot be cleaned', async ({ page }) => {
  await openCleanup(page);
  const firefox = group(page, /^Firefox cache · Windows/);
  await expect(firefox).toContainText('Close Firefox to clean its cache.');
  await expect(firefox.getByRole('checkbox')).toHaveCount(0);
  await expect(firefox.getByRole('button', { name: /Recycle Bin|Delete/ })).toHaveCount(0);
});

test('F16-AC1: moving to the Recycle Bin asks for confirmation, then reports', async ({ page }) => {
  await openCleanup(page);
  const temp = group(page, /^Temporary files · Windows/);
  await temp.getByRole('button', { name: /Move to the Recycle Bin/ }).click();
  const dialog = page.getByRole('dialog', { name: 'Move the selected items (4, 452 MB) to the Recycle Bin?' });
  await expect(dialog).toContainText('Prism checks every item first');
  await dialog.getByRole('button', { name: 'Cancel' }).click();
  await expect(temp).toBeVisible();

  await temp.getByRole('button', { name: /Move to the Recycle Bin/ }).click();
  await page.getByRole('dialog').getByRole('button', { name: 'Move to the Recycle Bin' }).click();
  await expect(page.getByRole('status')).toContainText('Moved to the Recycle Bin: 4. Freed: 452 MB.');
  await expect(group(page, /^Temporary files · Windows/)).toHaveCount(0);
});

test('F16-AC2: on a disk without Recycle Bin the deletion is permanent and says so', async ({ page }) => {
  await openCleanup(page, 'NAS Big-Storage');
  const nas = group(page, /^Large and old files · NAS Big-Storage/);
  await nas.getByRole('checkbox').first().check();
  await nas.getByRole('button', { name: /Delete permanently/ }).click();
  const dialog = page.getByRole('dialog', { name: /permanently\?$/ });
  await expect(dialog).toContainText('cannot be restored');
  await dialog.getByRole('button', { name: 'Delete permanently' }).click();
  await expect(page.getByRole('status')).toContainText('Deleted: 1. Freed: 7.8 GB.');
});
