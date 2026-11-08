import { expect, test, type Page } from '@playwright/test';
import { backToCatalog, diskTab, openCatalog, openDisk, openWhole } from './nav';

// The Space tab of a disk with the simulated backend (src/lib/mock-space.ts): a deterministic
// tree per sample disk. "Windows" is connected; "Backup Foto" is indexed but not connected.

async function openSpace(page: Page, disk: string) {
  await openCatalog(page);
  await openDisk(page, disk);
  await expect(page.getByRole('navigation', { name: 'Functions of the disk' }).getByRole('button', { name: 'Space' })).toHaveAttribute('aria-current', 'page');
}

const list = (page: Page) => page.getByRole('table', { name: 'Folder content, largest first' });
const rows = (page: Page) => list(page).locator('tbody tr');

test('F08-AC1/AC2: treemap with the folder content, drill-down and breadcrumb', async ({ page }) => {
  await openSpace(page, 'Windows');
  await expect(page.getByRole('figure', { name: /Space map of Windows/ })).toBeVisible();
  await expect(rows(page).first()).toBeVisible();
  // Largest first, with size, share and files.
  expect(await rows(page).count()).toBeGreaterThan(3);
  await expect(list(page).getByRole('columnheader', { name: '%' })).toBeVisible();

  // Open a folder from the list, then go back with the path.
  await list(page).getByRole('button', { name: 'Foto', exact: true }).click();
  const crumbs = page.getByRole('navigation', { name: 'Folder path' });
  await expect(crumbs.getByText('Foto', { exact: true })).toHaveAttribute('aria-current', 'location');
  await expect(list(page).getByRole('button', { name: /^20\d\d$/ }).first()).toBeVisible();
  await page.screenshot({ path: 'test-results/space-folder.png' });
  await crumbs.getByRole('button', { name: 'Windows' }).click();
  await expect(list(page).getByRole('button', { name: 'Foto', exact: true })).toBeVisible();
  // The parent folder button goes up one level.
  await list(page).getByRole('button', { name: 'Foto', exact: true }).click();
  await page.getByRole('button', { name: 'Parent folder' }).click();
  await expect(list(page).getByRole('button', { name: 'Foto', exact: true })).toBeVisible();

  // Hovering the map shows what is under the pointer.
  const box = (await page.getByRole('figure', { name: /Space map/ }).boundingBox())!;
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await expect(page.getByRole('tooltip')).toBeVisible();
  await page.screenshot({ path: 'test-results/space-map.png' });
});

test('F08-AC2: largest files of the disk', async ({ page }) => {
  await openSpace(page, 'Windows');
  await page.getByRole('button', { name: 'Largest files' }).click();
  const table = page.getByRole('table', { name: 'Largest files' });
  await expect(table.getByRole('row').nth(1)).toBeVisible();
  // pagefile.sys (4 GB) is not the largest: disk images are generated larger.
  const size = await table.getByRole('row').nth(1).getByRole('cell').nth(1).textContent();
  expect(size).toMatch(/GB/);
});

test('F08-AC3: statistics by type, for the disk and for the whole catalog', async ({ page }) => {
  await openSpace(page, 'Windows');
  await diskTab(page, 'File types');
  await expect(page.getByRole('heading', { name: 'By category' })).toBeVisible();
  await expect(page.getByText('Videos', { exact: true }).first()).toBeVisible();
  await expect(page.getByRole('heading', { name: 'By size' })).toBeVisible();
  const before = await page.getByText(/files, .* (GB|TB)$/).first().textContent();
  await backToCatalog(page);
  await openWhole(page, 'Catalogued');
  await expect(page.getByRole('heading', { name: 'By category' })).toBeVisible();
  await expect(page.getByText(/files, .* (GB|TB)$/).first()).not.toHaveText(before!);
  await page.screenshot({ path: 'test-results/space-types.png', fullPage: true });
});

test('F08-AC4: a disk that is not connected can be analysed, not opened', async ({ page }) => {
  await openSpace(page, 'Backup Foto');
  await expect(page.getByText(/disk not connected/)).toBeVisible();
  await expect(rows(page).first()).toBeVisible();
  await expect(page.getByRole('button', { name: /^Open: / })).toHaveCount(0);
  await backToCatalog(page);
  await expect(page.getByRole('heading', { name: 'Disks', level: 1 })).toBeVisible();
});

test('a connected disk offers open and show in Explorer for each item', async ({ page }) => {
  await openSpace(page, 'Windows');
  await list(page).getByRole('button', { name: /^Show in Explorer: / }).first().click();
  await expect(page.getByRole('alert')).toHaveCount(0);
});
