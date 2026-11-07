import { expect, test, type Page } from '@playwright/test';

// The similar images screen with the simulated backend (src/lib/mock-similar.ts): a RAW photo
// with an exported JPEG and a copy on the disconnected backup (differences 3 and 1), a logo and
// its small copy (5), a scanned receipt and a JPEG of it (9).

async function openSimilar(page: Page) {
  await page.goto('/');
  await page.getByRole('button', { name: 'Similar', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Similar images', level: 1 })).toBeVisible();
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
  await page.screenshot({ path: 'test-results/similar.png', fullPage: true });
});

test('F11-AC1: the similarity threshold and the disk filter', async ({ page }) => {
  await openSimilar(page);
  const slider = page.getByRole('slider');
  await slider.fill('10');
  await expect(page.getByText('3 groups of similar images')).toBeVisible();
  await expect(page.getByRole('article', { name: 'ricevuta.tif' })).toBeVisible();
  await page.getByRole('combobox', { name: 'Disk' }).selectOption({ label: 'NAS Big-Storage' });
  await expect(page.getByText('1 groups of similar images')).toBeVisible();
  await slider.fill('0');
  await expect(page.getByText('No similar images with this setting.')).toBeVisible();
});
