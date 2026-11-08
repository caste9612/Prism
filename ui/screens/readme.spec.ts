import { expect, test, type Page } from '@playwright/test';
import { diskTab, openCatalog, openDisk, openWhole } from '../e2e/nav';

// Screenshots for the README, from the simulated backend (fictional data). Not a test: run with
// `npm run screens`; the images go to docs/screenshots/.

const OUT = '../docs/screenshots';

/** The questions about ambiguous disks of the simulated catalog would cover the first screen. */
async function withoutQuestions(page: Page) {
  await page.addStyleTag({ content: '.identify { display: none !important; }' });
}

async function settle(page: Page) {
  await page.waitForLoadState('networkidle');
  await page.waitForTimeout(800);
}

test('catalog', async ({ page }) => {
  // A taller window: every group of disks, the NAS and the disk in a drawer (the app scrolls
  // inside its window, so a full-page shot would not show more).
  await page.setViewportSize({ width: 1280, height: 1360 });
  await openCatalog(page);
  await withoutQuestions(page);
  await settle(page);
  await page.screenshot({ path: `${OUT}/catalog.png` });
});

test('catalog, dark', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'dark' });
  await openCatalog(page);
  await withoutQuestions(page);
  await settle(page);
  await page.screenshot({ path: `${OUT}/catalog-dark.png` });
});

test('space of a disk', async ({ page }) => {
  await openCatalog(page);
  await openDisk(page, 'Data', 'Space');
  await settle(page);
  await page.screenshot({ path: `${OUT}/space.png` });
});

test('duplicates', async ({ page }) => {
  await openCatalog(page);
  await openWhole(page, 'Duplicates');
  await settle(page);
  await page.screenshot({ path: `${OUT}/duplicates.png` });
});

test('similar pictures', async ({ page }) => {
  await openCatalog(page);
  await openWhole(page, 'Similar');
  await settle(page);
  await page.screenshot({ path: `${OUT}/similar.png` });
});

test('search', async ({ page }) => {
  await openCatalog(page);
  await page.getByRole('searchbox', { name: 'Search every disk' }).fill('f');
  const box = page.getByRole('textbox', { name: 'Search all disks' });
  await box.fill('foto');
  await expect(page.getByRole('listbox', { name: 'Results' }).getByRole('option').first()).toBeVisible();
  await settle(page);
  await page.screenshot({ path: `${OUT}/search.png` });
});

test('health of a disk', async ({ page }) => {
  await openCatalog(page);
  await openDisk(page, 'Data');
  await diskTab(page, 'Health');
  await settle(page);
  await page.screenshot({ path: `${OUT}/health.png` });
});
