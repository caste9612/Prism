import { expect, test, type Page } from '@playwright/test';

// The duplicates screen with the simulated backend (src/lib/mock-duplicates.ts): a video on
// Data, on the disconnected "Backup Foto" and on the NAS; an ISO on Windows and Data; an archive
// only on the disconnected backup; a photo twice on Data and once on the backup; a PDF twice on
// Windows. The default rule keeps the oldest copy.

async function openDuplicates(page: Page) {
  await page.goto('/');
  await page.getByRole('button', { name: 'Duplicates' }).click();
  await expect(page.getByRole('heading', { name: 'Duplicates', level: 1 })).toBeVisible();
}

const group = (page: Page, name: string) => page.getByRole('article', { name });

test('F10-AC1/AC2: identical files on every disk, where each copy is, space to recover', async ({ page }) => {
  await openDuplicates(page);
  await expect(page.getByText('5 groups · 9.4 GB recoverable')).toBeVisible();
  const video = group(page, 'Vacanze 2019.mp4');
  await expect(video).toContainText('3 copies');
  await expect(video).toContainText('8.4 GB recoverable');
  await expect(video).toContainText('Backup Foto · cassetto 2');
  await expect(video).toContainText('not connected');
  await expect(video).toContainText('NAS Big-Storage');
  // F06-AC5: the analysis is not over, the list may grow.
  await expect(page.getByText('Content analysis still has 12,480 files to read')).toBeVisible();
  await page.screenshot({ path: 'test-results/duplicates.png', fullPage: true });
});

test('F10-AC3: rules propose the copies to delete, the user changes them', async ({ page }) => {
  await openDuplicates(page);
  const video = group(page, 'Vacanze 2019.mp4');
  const boxes = video.getByRole('checkbox');
  // Keep the oldest: the copy on Data stays, the one on the NAS is proposed.
  await expect(boxes.nth(0)).not.toBeChecked();
  await expect(boxes.nth(2)).toBeChecked();
  await expect(page.getByRole('region', { name: '4 copies selected · 4.9 GB' })).toBeVisible();

  // Keep the copy on the NAS instead.
  const rules = page.getByRole('group').filter({ hasText: 'Selection rules' }).or(page.locator('details.rules'));
  await rules.getByRole('combobox', { name: 'Selection rules' }).first().selectOption('keepOnVolume');
  await rules.getByRole('combobox', { name: 'Disk' }).selectOption({ label: 'NAS Big-Storage' });
  await page.getByRole('button', { name: 'Apply rules' }).click();
  await expect(boxes.nth(0)).toBeChecked();
  await expect(boxes.nth(2)).not.toBeChecked();

  // Never in folders containing "Video": nothing of the video is proposed.
  await page.getByRole('button', { name: 'Add a rule' }).click();
  await rules.getByRole('combobox', { name: 'Selection rules' }).nth(1).selectOption('neverIn');
  await rules.getByRole('textbox', { name: 'Text' }).fill('video');
  await page.getByRole('button', { name: 'Apply rules' }).click();
  await expect(boxes.nth(0)).not.toBeChecked();
  await expect(boxes.nth(2)).not.toBeChecked();
  // A manual change.
  await boxes.nth(0).check();
  await expect(boxes.nth(0)).toBeChecked();
});

test('F10-AC4: copies on disconnected disks are not selectable and one copy always stays', async ({ page }) => {
  await openDuplicates(page);
  // Every copy of the archive is on the disconnected backup.
  const album = group(page, 'Album.zip');
  for (const box of await album.getByRole('checkbox').all()) await expect(box).toBeDisabled();
  // The video copy on the backup.
  await expect(group(page, 'Vacanze 2019.mp4').getByRole('checkbox').nth(1)).toBeDisabled();
  // Two copies of the PDF: the second is proposed, the first cannot join it.
  const pdf = group(page, 'Relazione finale.pdf');
  await expect(pdf.getByRole('checkbox').nth(1)).toBeChecked();
  await expect(pdf.getByRole('checkbox').nth(0)).toBeDisabled();
  await pdf.getByRole('checkbox').nth(1).uncheck();
  await expect(pdf.getByRole('checkbox').nth(0)).toBeEnabled();
});

test('F16-AC4: keeping only copies on disconnected disks needs a second confirmation', async ({ page }) => {
  await openDuplicates(page);
  await page.getByRole('combobox', { name: 'Type' }).selectOption('images');
  const photo = group(page, 'IMG_2041.jpg');
  await expect(page.getByRole('article')).toHaveCount(1);
  await photo.getByRole('checkbox').nth(0).check();
  await expect(photo).toContainText('Only copies on disconnected disks would remain.');

  await page.getByRole('button', { name: /Delete selected copies/ }).click();
  const confirm = page.getByRole('dialog', { name: 'Delete 2 copies (12.2 MB)?' });
  await expect(confirm).toContainText('Prism reads each file again first');
  await expect(confirm).toContainText('go to the Recycle Bin');
  await confirm.getByRole('button', { name: 'Delete' }).click();

  const offline = page.getByRole('dialog', { name: 'Keep only copies on disconnected disks?' });
  await expect(offline).toBeVisible();
  await offline.getByRole('button', { name: 'Delete anyway' }).click();
  await expect(page.getByRole('status').filter({ hasText: 'Deleted' })).toContainText('Deleted: 2. Freed: 12.2 MB.');
  await expect(page.getByText('No identical files found with these filters.')).toBeVisible();
});

test('F16-AC2: copies on a disk without Recycle Bin are deleted permanently, after saying so', async ({ page }) => {
  await openDuplicates(page);
  await page.getByRole('combobox', { name: 'Type' }).selectOption('videos');
  await page.getByRole('button', { name: /Delete selected copies/ }).click();
  const confirm = page.getByRole('dialog', { name: 'Delete 1 copies (4.2 GB)?' });
  await expect(confirm).toContainText('they will be deleted permanently');
  await confirm.getByRole('button', { name: 'Cancel' }).click();
  await expect(confirm).toBeHidden();
  await expect(group(page, 'Vacanze 2019.mp4')).toBeVisible();
});

test('F06-AC5: progress on the cards; pause and resume from the settings', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('article', { name: 'Windows' })).toContainText('Content analysis: 45% · in progress');
  await expect(page.getByRole('article', { name: 'Data' })).toContainText('Content analysed');

  await page.getByRole('button', { name: 'Settings' }).click();
  await page.getByRole('checkbox', { name: 'Pause the content analysis' }).check();
  await expect(page.getByRole('checkbox', { name: 'Pause it while the PC runs on battery' })).not.toBeChecked();
  // F21-AC3: the analysis can be turned off per disk; network drives are never analysed.
  const disks = page.getByRole('group', { name: 'Disks to analyse' });
  await expect(disks.getByRole('checkbox')).toHaveCount(3);
  await expect(disks.getByRole('checkbox', { name: /NAS/ })).toHaveCount(0);
  await disks.getByRole('checkbox', { name: /^Data/ }).uncheck();
  await page.getByRole('button', { name: 'Disks' }).click();
  await page.getByRole('button', { name: 'Settings' }).click();
  await expect(page.getByRole('group', { name: 'Disks to analyse' }).getByRole('checkbox', { name: /^Data/ })).not.toBeChecked();
  await page.getByRole('button', { name: 'Duplicates' }).click();
  await expect(page.getByText('Content analysis is paused with 12,480 files still to read.')).toBeVisible();
  await page.getByRole('button', { name: 'Resume' }).click();
  await expect(page.getByText('Content analysis still has 12,480 files to read')).toBeVisible();
});
