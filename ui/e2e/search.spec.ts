import { expect, test, type Page } from '@playwright/test';
import { openCatalog } from './nav';

// Search with the simulated backend (src/lib/mock-search.ts) over the simulated trees of the
// sample disks: "Backup Foto" is indexed but not connected, in "cassetto 2".

/** Typing in the box of the top bar opens the search screen, which keeps what was typed. */
async function openSearch(page: Page) {
  await openCatalog(page);
  await page.getByRole('searchbox', { name: 'Search every disk' }).fill('f');
  const box = page.getByRole('textbox', { name: 'Search all disks' });
  await expect(box).toBeFocused();
  await expect(box).toHaveValue('f');
  return box;
}

const results = (page: Page) => page.getByRole('listbox', { name: 'Results' }).getByRole('option');

test('F07-AC2/AC4/AC7: results from every disk, offline ones marked and not openable', async ({ page }) => {
  const box = await openSearch(page);
  await box.fill('FOTO-0');
  await expect(results(page).first()).toBeVisible();
  await expect(page.getByRole('status')).toContainText('Results:');
  const offline = results(page).filter({ hasText: 'not connected · cassetto 2' }).first();
  await expect(offline).toBeVisible();
  await expect(offline.getByRole('button', { name: /^Open: / })).toBeDisabled();
  await expect(offline.getByRole('button', { name: /^Copy path: / })).toBeEnabled();
  const online = results(page).filter({ hasNotText: 'not connected' }).first();
  await expect(online.getByRole('button', { name: /^Open: / })).toBeEnabled();
  await page.screenshot({ path: 'test-results/search.png' });
});

test('F07-AC5: the controls write the filters into the search text', async ({ page }) => {
  const box = await openSearch(page);
  await box.fill('foto');
  await page.getByRole('combobox', { name: 'Type' }).selectOption('image');
  await expect(box).toHaveValue('foto type:image');
  await page.getByRole('combobox', { name: 'Connection' }).selectOption('no');
  await expect(box).toHaveValue('foto type:image online:no');
  await page.getByRole('combobox', { name: 'Type' }).selectOption('');
  await expect(box).toHaveValue('foto online:no');
  await expect(results(page).first()).toContainText('not connected');
});

test('F07-AC6: a mistake is shown where it is, not as zero results', async ({ page }) => {
  const box = await openSearch(page);
  await box.fill('report colour:red');
  const alert = page.getByRole('alert');
  await expect(alert).toContainText('There is no filter called “colour”.');
  await expect(alert.locator('mark')).toHaveText('colour');
  await expect(page.getByRole('listbox')).toHaveCount(0);
  await box.fill('foto disk:Archivio');
  await expect(page.getByRole('alert')).toContainText('No disk is called');
});

test('F07: keyboard selection', async ({ page }) => {
  const box = await openSearch(page);
  await box.fill('video');
  await expect(results(page).nth(1)).toBeVisible();
  await expect(results(page).first()).toHaveAttribute('aria-selected', 'true');
  await box.press('ArrowDown');
  await expect(results(page).nth(1)).toHaveAttribute('aria-selected', 'true');
  await box.press('ArrowUp');
  await expect(results(page).first()).toHaveAttribute('aria-selected', 'true');
});

test('F07-AC1: the separate search window shows the search alone', async ({ page }) => {
  await page.goto('/?view=search');
  const box = page.getByRole('textbox', { name: 'Search all disks' });
  await expect(box).toBeVisible();
  await expect(page.getByRole('navigation')).toHaveCount(0);
  await expect(page.getByRole('searchbox')).toHaveCount(0);
  await box.fill('foto-0');
  await expect(results(page).first()).toBeVisible();
  await box.press('Escape');
});
