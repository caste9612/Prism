import { expect, test, type Page } from '@playwright/test';
import { openCatalog, openWhole } from './nav';

// The similar images screen with the simulated backend (src/lib/mock-similar.ts): a RAW photo
// with an exported JPEG and a copy on the disconnected backup (differences 3 and 1), a logo and
// its small copy (5), a scanned receipt and a JPEG of it (9).

async function openSimilar(page: Page) {
  await openCatalog(page);
  await openWhole(page, 'Similar');
  await expect(page.getByRole('tab', { name: 'Images' })).toHaveAttribute('aria-selected', 'true');
}

test('F11-AC1/AC3: groups with previews side by side, resolution, size and disk', async ({ page }) => {
  await openSimilar(page);
  await expect(page.getByText('2 groups of similar images')).toBeVisible();
  const photo = page.getByRole('article', { name: 'IMG_2041.CR2' });
  await expect(photo).toContainText('3 images');
  await expect(photo).toContainText('6000×4000 · 25.0 MB');
  await expect(photo).toContainText('Backup Foto · cassetto 2');
  await expect(photo).toContainText('not connected');
  await expect(photo).toContainText('1.9 MB · difference: 3');
  // Previews of the connected copies; none for the disconnected one.
  await expect(photo.getByRole('img', { name: 'IMG_2041.CR2' })).toBeVisible();
  await expect(photo.getByRole('img', { name: 'mare-2041.jpg' })).toBeVisible();
  await expect(photo).toContainText('Disk not connected: no preview');
  await expect(page.getByText('The content analysis still has 3,210 images to read')).toBeVisible();
  // Nothing is deleted from this screen.
  await expect(page.getByText('Nothing is deleted from here')).toBeVisible();
  await expect(page.getByRole('checkbox')).toHaveCount(0);
  await page.screenshot({ path: 'test-results/similar.png', fullPage: true });
});

test('F11-AC1: the similarity threshold and the disk filter', async ({ page }) => {
  await openSimilar(page);
  const slider = page.getByRole('slider');
  await slider.fill('10');
  await expect(page.getByText('3 groups of similar images')).toBeVisible();
  await expect(page.getByRole('article', { name: 'ricevuta.tif' })).toBeVisible();
  await page.getByRole('combobox', { name: 'Disk' }).selectOption({ label: 'NAS Big-Storage' });
  await expect(page.getByText('1 group of similar images')).toBeVisible();
  await slider.fill('0');
  await expect(page.getByText('No similar images with this setting.')).toBeVisible();
});

test('F12-AC1: similar videos with length, resolution and previews', async ({ page }) => {
  await openSimilar(page);
  await page.getByRole('tab', { name: 'Videos' }).click();
  await expect(page.getByText('1 group of similar videos')).toBeVisible();
  const group = page.getByRole('article', { name: 'Vacanze 2019.mp4' });
  await expect(group).toContainText('2 videos');
  await expect(group).toContainText('12:34 · 1920×1080 · 4.2 GB');
  await expect(group).toContainText('12:35 · 1280×720 · 1.1 GB · difference: 1');
  await expect(group.getByRole('img', { name: 'Vacanze 2019.mp4' })).toBeVisible();
  await expect(page.getByText('The content analysis still has 42 videos to read')).toBeVisible();
  await page.getByRole('tab', { name: 'Images' }).click();
  await expect(page.getByText('2 groups of similar images')).toBeVisible();
});
