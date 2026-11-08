import { expect, test, type Page } from '@playwright/test';
import { backToCatalog, card, cardMenu, openCatalog, openSettings } from './nav';

// End-to-end flows of the catalog screen with the simulated backend (src/lib/mock.ts): five
// sample disks, of which "Backup Foto" is offline (in "cassetto 2") and "KINGSTON" is new.

const newDisks = (page: Page) => page.getByRole('dialog', { name: 'New disks' });

async function openNewDisks(page: Page) {
  await page.getByRole('button', { name: '1 new disk' }).click();
  await expect(newDisks(page)).toBeVisible();
}

test.beforeEach(async ({ page }) => {
  await openCatalog(page);
});

test('F01-AC1: offline disks stay visible with their last indexed data', async ({ page }) => {
  const filters = page.getByRole('group', { name: 'Show' });
  await expect(filters.getByRole('button', { name: 'All 5' })).toBeVisible();
  await expect(filters.getByRole('button', { name: 'In the drawer 1' })).toBeVisible();
  await expect(page.locator('footer')).toContainText('4 connected');
  const backup = card(page, 'Backup Foto');
  await expect(backup).toContainText('In the drawer · seen 12 days ago');
  await expect(backup).toContainText('182,340');
  await expect(backup).toContainText('cassetto 2');
  // An offline disk cannot be indexed or opened.
  await expect(backup.getByRole('button', { name: /^Index/ })).toHaveCount(0);
  const menu = await cardMenu(page, 'Backup Foto');
  await expect(menu.getByRole('menuitem', { name: 'Open in Explorer' })).toHaveCount(0);
  await expect(menu.getByRole('menuitem', { name: /Update now|Index/ })).toHaveCount(0);
  // Only the disks in the drawer.
  await filters.getByRole('button', { name: 'In the drawer 1' }).click();
  await expect(page.getByRole('article')).toHaveCount(1);
  await page.screenshot({ path: 'test-results/catalog.png', fullPage: true });
});

test('a connected disk opens in Explorer without errors', async ({ page }) => {
  const menu = await cardMenu(page, 'Data');
  await menu.getByRole('menuitem', { name: 'Open in Explorer' }).click();
  await expect(page.getByRole('alert')).toHaveCount(0);
});

test('F02-AC1, F01-AC3: a new disk is proposed, indexed with progress, then listed', async ({ page }) => {
  await openNewDisks(page);
  await newDisks(page).getByRole('listitem', { name: 'KINGSTON' }).getByRole('button', { name: 'Index', exact: true }).click();
  await expect(page.getByRole('button', { name: '1 new disk' })).toHaveCount(0);

  const stick = card(page, 'KINGSTON');
  await expect(stick).toContainText('Indexing');
  await expect(stick.getByRole('button', { name: 'Cancel' })).toBeVisible();
  await expect(stick).toContainText('120,000', { timeout: 10_000 });
  await expect(stick).toContainText('Updated');
  const menu = await cardMenu(page, 'KINGSTON');
  await expect(menu.getByRole('menuitem', { name: 'Update now' })).toBeVisible();
});

test('F02-AC1: "Not now" hides the proposal for this session only', async ({ page }) => {
  await openNewDisks(page);
  await newDisks(page).getByRole('button', { name: 'Options for KINGSTON' }).click();
  await page.getByRole('menuitem', { name: 'Not now' }).click();
  await expect(page.getByRole('button', { name: '1 new disk' })).toHaveCount(0);
  await expect(card(page, 'KINGSTON')).toContainText('Not indexed yet');
});

test('F02-AC4: "Always ignore" hides the disk until restored from the settings', async ({ page }) => {
  await openNewDisks(page);
  await newDisks(page).getByRole('button', { name: 'Options for KINGSTON' }).click();
  await page.getByRole('menuitem', { name: 'Always ignore' }).click();
  await expect(card(page, 'KINGSTON')).toHaveCount(0);
  // The ignored disk is connected but no longer counted or shown.
  await expect(page.getByRole('group', { name: 'Show' }).getByRole('button', { name: 'All 4' })).toBeVisible();

  await openSettings(page);
  const ignored = page.getByRole('listitem').filter({ hasText: 'KINGSTON' });
  await expect(ignored).toBeVisible();
  await ignored.getByRole('button', { name: 'Stop ignoring' }).click();
  await expect(page.getByText('No disk is ignored.')).toBeVisible();

  await backToCatalog(page);
  await expect(card(page, 'KINGSTON')).toBeVisible();
});

test('F03-AC1: name, location and colour of a disk', async ({ page }) => {
  const menu = await cardMenu(page, 'Data');
  await menu.getByRole('menuitem', { name: 'Name, location and colour…' }).click();
  const dialog = page.getByRole('dialog', { name: 'Name, location and colour' });
  await expect(dialog).toBeVisible();
  await dialog.getByLabel('Name').fill('Archive');
  // The locations of the other disks are suggested.
  await expect(dialog).toContainText('cassetto 2');
  await dialog.getByLabel('Where it is').fill('drawer 3');
  await dialog.getByRole('button', { name: 'Green' }).click();
  await expect(dialog.getByRole('button', { name: 'Green' })).toHaveAttribute('aria-pressed', 'true');
  await dialog.getByRole('button', { name: 'Save' }).click();
  await expect(dialog).toBeHidden();

  const archive = card(page, 'Archive');
  await expect(archive).toContainText('drawer 3');
  await expect(card(page, 'Data')).toHaveCount(0);
});

test('F01-AC5: removing a disk asks for confirmation', async ({ page }) => {
  let menu = await cardMenu(page, 'NAS Big-Storage');
  await menu.getByRole('menuitem', { name: 'Remove from catalog…' }).click();
  const dialog = page.getByRole('dialog', { name: 'Remove “NAS Big-Storage” from the catalog?' });
  await expect(dialog).toContainText('The files on the disk are not touched.');
  await dialog.getByRole('button', { name: 'Cancel' }).click();
  await expect(card(page, 'NAS Big-Storage')).toBeVisible();

  menu = await cardMenu(page, 'NAS Big-Storage');
  await menu.getByRole('menuitem', { name: 'Remove from catalog…' }).click();
  await page.getByRole('dialog').getByRole('button', { name: 'Remove from catalog' }).click();
  await expect(card(page, 'NAS Big-Storage')).toHaveCount(0);
});

test('F21-AC1: language and theme apply immediately', async ({ page }) => {
  await openSettings(page);
  await page.getByLabel('Theme').selectOption('dark');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await page.getByLabel('Language').selectOption('it');
  await expect(page.getByRole('heading', { name: 'Impostazioni', level: 1 })).toBeVisible();
  await backToCatalog(page, 'Catalogo');
  await expect(page.locator('footer')).toContainText('4 collegati');
  await page.screenshot({ path: 'test-results/catalog-dark.png', fullPage: true });
});

test('F02-AC3: a disk that looks like two catalog disks waits for the user to say which', async ({ page }) => {
  const panel = page.getByRole('region', { name: 'Which disk is Backup Foto?' });
  await expect(panel).toContainText('for example a cloned disk');
  await expect(panel).toContainText('K:\\ · NTFS · 2.0 TB');
  await expect(panel.getByRole('button', { name: 'It is Backup Foto', exact: true })).toBeVisible();
  await expect(panel.getByRole('button', { name: 'It is Backup Foto (copia)' })).toBeVisible();
  await expect(panel).toContainText('Location: cassetto 2');
  await panel.getByRole('button', { name: 'It is Backup Foto', exact: true }).click();
  await expect(panel).toHaveCount(0);
  // The chosen disk is now connected, on its new letter.
  const backup = card(page, 'Backup Foto');
  await expect(backup).toContainText('K:');
  await expect(backup).not.toContainText('In the drawer');
});

test('F17-AC1/AC2: the health of a disk with its values and history', async ({ page }) => {
  await expect(card(page, 'Windows').getByRole('button', { name: 'Good 42 °C' })).toBeVisible();
  await card(page, 'Data').getByRole('button', { name: /Needs attention/ }).click();
  await expect(page.getByRole('heading', { name: 'Data', level: 1 })).toBeVisible();
  const health = page.getByRole('region', { name: 'Health of WDC WD10EZEX-08WN4A0' });
  await expect(health).toContainText('Needs attention');
  await expect(health).toContainText('Some damaged sectors have been replaced with spare ones.');
  await expect(page.getByText('Replaced sectors').first()).toBeVisible();
  await expect(page.getByText('31,204')).toBeVisible();
  await expect(page.getByRole('table').getByRole('row')).toHaveCount(3);
  await page.screenshot({ path: 'test-results/health.png', fullPage: true });
});
