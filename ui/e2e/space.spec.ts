import { expect, test, type Page } from '@playwright/test';

// The space screen with the simulated backend (src/lib/mock-space.ts): a deterministic tree per
// sample disk. "Windows" is connected; "Backup Foto" is indexed but not connected.

async function openSpace(page: Page, disk: string) {
  await page.goto('/');
  await page.getByRole('article', { name: disk }).getByRole('button', { name: 'Space' }).click();
  await expect(page.getByRole('heading', { name: disk, level: 1 })).toBeVisible();
}

const list = (page: Page) => page.getByLabel('Folder content, largest first');

test('F08-AC1/AC2: treemap with the folder content, drill-down and breadcrumb', async ({ page }) => {
  await openSpace(page, 'Windows');
  await expect(page.getByRole('figure', { name: /Space map of Windows/ })).toBeVisible();
  const rows = list(page).getByRole('listitem');
  await expect(rows.first()).toBeVisible();
  // Largest first.
  const sizes = await list(page).locator('.num').allTextContents();
  expect(sizes.length).toBeGreaterThan(3);

  // Open a folder from the list, then go back with the breadcrumb.
  await list(page).getByRole('button', { name: 'Foto', exact: true }).click();
  const crumbs = page.getByRole('navigation', { name: 'Folder path' });
  await expect(crumbs.getByText('Foto', { exact: true })).toHaveAttribute('aria-current', 'location');
  await expect(list(page).getByRole('button', { name: /^20\d\d$/ }).first()).toBeVisible();
  await page.screenshot({ path: 'test-results/space-folder.png' });
  await crumbs.getByRole('button', { name: 'Windows' }).click();
  await expect(list(page).getByRole('button', { name: 'Foto', exact: true })).toBeVisible();

  // Hovering the map shows what is under the pointer.
  const box = (await page.getByRole('figure', { name: /Space map/ }).boundingBox())!;
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await expect(page.getByRole('tooltip')).toBeVisible();
  await page.screenshot({ path: 'test-results/space-map.png' });
});

test('F08-AC2: largest files of the disk', async ({ page }) => {
  await openSpace(page, 'Windows');
  await page.getByRole('tab', { name: 'Largest files' }).click();
  const table = page.getByRole('table');
  await expect(table.getByRole('row').nth(1)).toBeVisible();
  // pagefile.sys (4 GB) is not the largest: disk images are generated larger.
  const first = await table.getByRole('row').nth(1).getByRole('cell').first().textContent();
  expect(first).toMatch(/GB/);
});

test('F08-AC3: statistics by type, for the disk and for the whole catalog', async ({ page }) => {
  await openSpace(page, 'Windows');
  await page.getByRole('tab', { name: 'File types' }).click();
  await expect(page.getByRole('heading', { name: 'By category' })).toBeVisible();
  await expect(page.getByText('Videos', { exact: true }).first()).toBeVisible();
  await expect(page.getByRole('heading', { name: 'By size' })).toBeVisible();
  const before = await page.getByText(/files, .* (GB|TB)$/).first().textContent();
  await page.getByRole('radio', { name: 'Whole catalog' }).click();
  await expect(page.getByRole('radio', { name: 'Whole catalog' })).toHaveAttribute('aria-checked', 'true');
  await expect(page.getByText(/files, .* (GB|TB)$/).first()).not.toHaveText(before!);
  await page.screenshot({ path: 'test-results/space-types.png', fullPage: true });
});

test('F08-AC4: a disk that is not connected can be analysed, not opened', async ({ page }) => {
  await openSpace(page, 'Backup Foto');
  await expect(page.getByText(/disk not connected/)).toBeVisible();
  await expect(list(page).getByRole('listitem').first()).toBeVisible();
  await expect(page.getByRole('button', { name: /^Open: / })).toHaveCount(0);
  await page.getByRole('button', { name: 'Disks' }).first().click();
  await expect(page.getByRole('heading', { name: 'Disks', level: 1 })).toBeVisible();
});

test('a connected disk offers open and show in Explorer for each item', async ({ page }) => {
  await openSpace(page, 'Windows');
  await list(page).getByRole('button', { name: /^Show in Explorer: / }).first().click();
  await expect(page.getByRole('alert')).toHaveCount(0);
});
