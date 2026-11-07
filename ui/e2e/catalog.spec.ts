import { expect, test, type Page } from '@playwright/test';

// End-to-end flows of the catalog screen with the simulated backend (src/lib/mock.ts): five
// sample disks, of which "Backup Foto" is offline and "KINGSTON" is new.

const card = (page: Page, name: string) => page.getByRole('article', { name });

test.beforeEach(async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Disks', level: 1 })).toBeVisible();
});

test('F01-AC1: offline disks stay visible with their last indexed data', async ({ page }) => {
  await expect(page.getByText('4 connected · 5 in the catalog')).toBeVisible();
  const backup = card(page, 'Backup Foto');
  await expect(backup).toContainText('Not connected');
  await expect(backup).toContainText('182,340 files');
  await expect(backup).toContainText('Last seen 12 days ago');
  await expect(backup).toContainText('Location: cassetto 2');
  // An offline disk cannot be indexed or opened.
  await expect(backup.getByRole('button', { name: /Index|Update/ })).toHaveCount(0);
  await expect(backup.getByRole('button', { name: 'Open in Explorer' })).toHaveCount(0);
});

test('a connected disk opens in Explorer without errors', async ({ page }) => {
  await card(page, 'Data').getByRole('button', { name: 'Open in Explorer' }).click();
  await expect(page.getByRole('alert')).toHaveCount(0);
});

test('F02-AC1, F01-AC3: a new disk is proposed, indexed with progress, then listed', async ({ page }) => {
  const prompt = page.getByRole('region', { name: 'New disk: KINGSTON' });
  await expect(prompt).toBeVisible();
  await prompt.getByRole('button', { name: 'Index', exact: true }).click();
  await expect(prompt).toBeHidden();

  const stick = card(page, 'KINGSTON');
  await expect(stick).toContainText('Indexing');
  await expect(stick.getByRole('button', { name: 'Cancel' })).toBeVisible();
  await expect(stick).toContainText('120,000 files', { timeout: 10_000 });
  await expect(stick.getByRole('button', { name: 'Update' })).toBeVisible();
});

test('F02-AC1: "Not now" hides the prompt for this session only', async ({ page }) => {
  const prompt = page.getByRole('region', { name: 'New disk: KINGSTON' });
  await prompt.getByRole('button', { name: 'Not now' }).click();
  await expect(prompt).toBeHidden();
  await expect(card(page, 'KINGSTON')).toContainText('Not indexed yet');
});

test('F02-AC4: "Always ignore" hides the disk until restored from the settings', async ({ page }) => {
  await page.getByRole('region', { name: 'New disk: KINGSTON' }).getByRole('button', { name: 'Always ignore' }).click();
  await expect(card(page, 'KINGSTON')).toHaveCount(0);
  // The ignored disk is connected but no longer counted or shown.
  await expect(page.getByText('3 connected · 4 in the catalog')).toBeVisible();

  await page.getByRole('button', { name: 'Settings' }).click();
  const ignored = page.getByRole('listitem').filter({ hasText: 'KINGSTON' });
  await expect(ignored).toBeVisible();
  await ignored.getByRole('button', { name: 'Stop ignoring' }).click();
  await expect(page.getByText('No disk is ignored.')).toBeVisible();

  await page.getByRole('button', { name: 'Disks' }).click();
  await expect(card(page, 'KINGSTON')).toBeVisible();
});

test('F03-AC1: name, location and colour of a disk', async ({ page }) => {
  await card(page, 'Data').getByRole('button', { name: 'Name and location' }).click();
  const dialog = page.getByRole('dialog', { name: 'Name and location' });
  await expect(dialog).toBeVisible();
  await dialog.getByLabel('Name').fill('Archive');
  await dialog.getByLabel('Physical location').fill('drawer 3');
  await dialog.getByRole('button', { name: '#10b981' }).click();
  await dialog.getByRole('button', { name: 'Save' }).click();
  await expect(dialog).toBeHidden();

  const archive = card(page, 'Archive');
  await expect(archive).toContainText('Location: drawer 3');
  await expect(card(page, 'Data')).toHaveCount(0);
});

test('F01-AC5: removing a disk asks for confirmation', async ({ page }) => {
  await card(page, 'NAS Big-Storage').getByRole('button', { name: 'Remove from catalog' }).click();
  const dialog = page.getByRole('dialog', { name: 'Remove “NAS Big-Storage” from the catalog?' });
  await expect(dialog).toContainText('The files on the disk are not touched.');
  await dialog.getByRole('button', { name: 'Cancel' }).click();
  await expect(card(page, 'NAS Big-Storage')).toBeVisible();

  await card(page, 'NAS Big-Storage').getByRole('button', { name: 'Remove from catalog' }).click();
  await page.getByRole('dialog').getByRole('button', { name: 'Remove from catalog' }).click();
  await expect(card(page, 'NAS Big-Storage')).toHaveCount(0);
});

test('F21-AC1: language and theme apply immediately', async ({ page }) => {
  await page.getByRole('button', { name: 'Settings' }).click();
  await page.getByLabel('Theme').selectOption('dark');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await page.getByLabel('Language').selectOption('it');
  await expect(page.getByRole('heading', { name: 'Impostazioni', level: 1 })).toBeVisible();
  await page.getByRole('button', { name: 'Dischi' }).click();
  await expect(page.getByText('4 collegati · 5 nel catalogo')).toBeVisible();
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
  const backup = page.getByRole('article', { name: 'Backup Foto' });
  await expect(backup).toContainText('Connected');
  await expect(backup).toContainText('K:\\');
});

test('F17-AC1/AC2: the health of a disk with its values and history', async ({ page }) => {
  await expect(page.getByRole('article', { name: 'Windows' }).getByRole('button', { name: 'Health: good · 42 °C' })).toBeVisible();
  await page.getByRole('article', { name: 'Data' }).getByRole('button', { name: /Health: needs attention/ }).click();
  const dialog = page.getByRole('dialog', { name: 'Health of WDC WD10EZEX-08WN4A0' });
  await expect(dialog).toContainText('Some damaged sectors have been replaced with spare ones.');
  await expect(dialog).toContainText('Replaced sectors');
  await expect(dialog).toContainText('31,204');
  await expect(dialog.getByRole('row')).toHaveCount(3);
  await dialog.getByRole('button', { name: 'Close' }).click();
  await expect(dialog).toBeHidden();
});
