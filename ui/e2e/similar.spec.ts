import { expect, test, type Page } from '@playwright/test';
import { openCatalog, openDisk, openWhole } from './nav';

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
  // Files can be chosen for deletion only on connected disks (REVIEW D67).
  await expect(page.getByText('Select the files to delete')).toBeVisible();
  await expect(photo.getByRole('checkbox')).toHaveCount(2);
  await page.screenshot({ path: 'test-results/similar.png', fullPage: true });
});

test('D67, F16: similar files are deleted after a confirmation, one of each group stays', async ({ page }) => {
  await openSimilar(page);
  const logo = page.getByRole('article', { name: 'logo.png' });
  const small = logo.getByRole('checkbox', { name: 'Select logo piccolo.png to delete' });
  const big = logo.getByRole('checkbox', { name: 'Select logo.png to delete' });
  await small.check();
  // The last file of the group cannot be chosen.
  await expect(big).toBeDisabled();
  const bar = page.getByRole('region', { name: /Selected: 1/ });
  await expect(bar).toContainText('They go to the Recycle Bin');
  await logo.evaluate((el) => el.scrollIntoView({ block: 'center' }));
  await page.screenshot({ path: 'test-results/similar-delete.png' });
  await bar.getByRole('button', { name: /Delete selected/ }).click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toContainText('Similar files are not identical copies');
  await expect(dialog).toContainText('At least one file of each group stays.');
  await dialog.getByRole('button', { name: 'Delete', exact: true }).click();
  await expect(page.getByRole('status').filter({ hasText: 'Deleted: 1.' })).toBeVisible();
  // The group has one file left: it is no longer a group.
  await expect(page.getByText('1 group of similar images')).toBeVisible();
  await expect(page.getByRole('article', { name: 'logo.png' })).toHaveCount(0);
});

test('F11-AC1, D70: the similarity threshold and the groups among some disks only', async ({ page }) => {
  await openSimilar(page);
  const slider = page.getByRole('slider');
  await slider.fill('10');
  await expect(page.getByText('3 groups of similar images')).toBeVisible();
  await expect(page.getByRole('article', { name: 'ricevuta.tif' })).toBeVisible();
  const disks = page.getByRole('group', { name: 'Disks' });
  await expect(disks.getByRole('button', { name: 'All disks' })).toHaveAttribute('aria-pressed', 'true');
  // One disk alone: the receipt has no similar file on it.
  await disks.getByRole('button', { name: 'NAS Media' }).click();
  await expect(page.getByText('No similar images with this setting.')).toBeVisible();
  // Two disks: the groups among their files, without the copy on the backup disk.
  await disks.getByRole('button', { name: 'Data' }).click();
  await expect(page.getByText('2 groups of similar images')).toBeVisible();
  await expect(page.getByText('Groups among the files of the chosen disks only')).toBeVisible();
  const photo = page.getByRole('article', { name: 'IMG_2041.CR2' });
  await expect(photo).toContainText('2 images');
  await expect(photo).not.toContainText('Backup Foto');
  await page.screenshot({ path: 'test-results/similar-disks.png', fullPage: true });
  await disks.getByRole('button', { name: 'All disks' }).click();
  await expect(page.getByText('3 groups of similar images')).toBeVisible();
  await slider.fill('0');
  await expect(page.getByText('No similar images with this setting.')).toBeVisible();
});

test('D70, D71: from the screen of a disk, other disks can be added', async ({ page }) => {
  await openCatalog(page);
  await openDisk(page, 'Data', 'Similar');
  const disks = page.getByRole('group', { name: 'Disks' });
  await expect(disks.getByRole('button', { name: 'Data' })).toHaveAttribute('aria-pressed', 'true');
  // Among the files of Data: the photo and its export, without the copy on the backup.
  const photo = page.getByRole('article', { name: 'IMG_2041.CR2' });
  await expect(photo).toContainText('2 images');
  await disks.getByRole('button', { name: 'Backup Foto' }).click();
  await expect(photo).toContainText('3 images');
  await expect(photo).toContainText('Backup Foto');
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
