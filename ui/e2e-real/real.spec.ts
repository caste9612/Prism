import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync, rmSync, statSync, utimesSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { chromium, expect, test, type Browser, type Page } from '@playwright/test';

// Pre-release check of the REAL development app (docs/TESTING.md §6): every screen on the real
// catalog, read-only, plus the file operations on the PRISMTEST test disk only.
//
// Safety (CLAUDE.md, "Sicurezza dei dati"): on real disks these tests only read and navigate;
// they never press a button that deletes, cleans or copies. The file operations run on the
// volume labelled PRISMTEST, in a folder created here and removed at the end; the debug build
// also refuses any deletion outside its sandbox (REVIEW L10).

const PORT = process.env.PRISM_CDP_PORT ?? '9333';
const SHOTS = 'test-results/real';
const DIR_NAME = 'prism-real-test';
const MB = 1_000_000;

let browser: Browser;
let page: Page;
let testRoot: string | null = null;
let restoreLanguage = 'system';
let restoreTheme = 'system';

/** The letter of the PRISMTEST volume, or null when it is not mounted. */
function prismTestRoot(): string | null {
  const out = execFileSync('powershell', ['-NoProfile', '-Command', "(Get-Volume | Where-Object FileSystemLabel -eq 'PRISMTEST' | Select-Object -First 1).DriveLetter"], {
    encoding: 'utf8',
  }).trim();
  return /^[A-Z]$/.test(out) ? `${out}:\\` : null;
}

function bytes(size: number, seed: number): Buffer {
  const b = Buffer.alloc(size);
  let x = seed;
  for (let i = 0; i < size; i += 4) {
    x = (x * 1103515245 + 12345) >>> 0;
    b.writeUInt32LE(x, Math.min(i, size - 4));
  }
  return b;
}

/** A 24-bit BMP of `size`×`size` pixels: the same picture at every size (gradients, a dark square). */
function picture(size: number): Buffer {
  const row = Math.ceil((size * 3) / 4) * 4;
  const data = row * size;
  const b = Buffer.alloc(54 + data);
  b.write('BM', 0, 'ascii');
  b.writeUInt32LE(54 + data, 2);
  b.writeUInt32LE(54, 10);
  b.writeUInt32LE(40, 14);
  b.writeInt32LE(size, 18);
  b.writeInt32LE(size, 22);
  b.writeUInt16LE(1, 26);
  b.writeUInt16LE(24, 28);
  b.writeUInt32LE(data, 34);
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const u = x / size;
      const v = y / size;
      const dark = u > 0.25 && u < 0.6 && v > 0.3 && v < 0.7;
      const o = 54 + y * row + x * 3;
      b[o] = dark ? 20 : Math.round((255 * (u + v)) / 2);
      b[o + 1] = dark ? 20 : Math.round(255 * u);
      b[o + 2] = dark ? 20 : Math.round(255 * v);
    }
  }
  return b;
}

/**
 * The test folder on PRISMTEST: two identical files, an empty folder, a source and a backup,
 * and one picture at two sizes (similar, not identical).
 */
function prepareTestFolder(root: string): string {
  const dir = join(root, DIR_NAME);
  rmSync(dir, { recursive: true, force: true });
  for (const d of ['dup\\a', 'dup\\b', 'empty', 'src', 'dst', 'pics']) mkdirSync(join(dir, d), { recursive: true });
  writeFileSync(join(dir, 'pics\\prism-real-photo.bmp'), picture(512));
  writeFileSync(join(dir, 'pics\\prism-real-photo-small.bmp'), picture(256));
  const content = bytes(2 * MB, 7);
  const old = new Date(Date.now() - 3 * 86_400_000);
  writeFileSync(join(dir, 'dup\\a\\prism-real-dup.bin'), content);
  utimesSync(join(dir, 'dup\\a\\prism-real-dup.bin'), old, old);
  writeFileSync(join(dir, 'dup\\b\\prism-real-dup.bin'), content);
  for (let i = 1; i <= 3; i++) {
    const f = join(dir, `src\\prism-real-file-${i}.bin`);
    writeFileSync(f, bytes(1_500_000 + i, 100 + i));
    utimesSync(f, old, old);
  }
  // The backup already has the first file.
  writeFileSync(join(dir, 'dst\\prism-real-file-1.bin'), bytes(1_500_001, 101));
  utimesSync(join(dir, 'dst\\prism-real-file-1.bin'), old, old);
  return dir;
}

function filesUnder(dir: string): string[] {
  if (!existsSync(dir)) return [];
  return readdirSync(dir).flatMap((n) => {
    const p = join(dir, n);
    return statSync(p).isDirectory() ? filesUnder(p) : [p];
  });
}

const card = (name: string) => page.getByRole('article', { name });
const tabs = () => page.getByRole('navigation', { name: 'Functions of the disk' });

async function home() {
  await page.getByRole('button', { name: 'Prism — catalog' }).click();
  await expect(page.getByRole('heading', { name: 'Disks', level: 1 })).toBeVisible();
}

async function openDisk(name: string, tab?: string) {
  await home();
  await card(name).getByRole('button', { name, exact: true }).click();
  await expect(page.getByRole('heading', { name, level: 1 })).toBeVisible();
  if (tab) {
    await tabs().getByRole('button', { name: tab, exact: true }).click();
    await expect(tabs().getByRole('button', { name: tab, exact: true })).toHaveAttribute('aria-current', 'page');
  }
}

/** How far the screen is wider than the window (real names are long): must be 0. */
async function wider(): Promise<number> {
  return page.evaluate(() => {
    const main = document.querySelector('main')!;
    return Math.max(document.documentElement.scrollWidth - document.documentElement.clientWidth, main.scrollWidth - main.clientWidth);
  });
}

async function shot(name: string) {
  await page.waitForTimeout(400);
  await page.screenshot({ path: `${SHOTS}/${name}.png`, fullPage: true });
}

async function openSettings() {
  await page.getByRole('button', { name: /^(Settings|Impostazioni)$/ }).click();
}

test.describe.configure({ mode: 'serial' });

test.beforeAll(async () => {
  browser = await chromium.connectOverCDP(`http://127.0.0.1:${PORT}`);
  const pages = browser.contexts().flatMap((c) => c.pages());
  const main = pages.find((p) => !p.url().includes('view=search'));
  if (!main) throw new Error('Prism is not running with the WebView2 debug port (see the config).');
  page = main;
  await page.setViewportSize({ width: 1280, height: 860 }).catch(() => {});
  // The checks read English texts: switch the language, put the user's back at the end.
  await openSettings();
  restoreLanguage = await page.locator('#set-language').inputValue();
  restoreTheme = await page.locator('#set-theme').inputValue();
  await page.locator('#set-language').selectOption('en');
  await page.locator('#set-theme').selectOption('light');
  await home();
  testRoot = prismTestRoot();
});

test.afterAll(async () => {
  if (page) {
    await openSettings();
    await page.locator('#set-theme').selectOption(restoreTheme);
    await page.locator('#set-language').selectOption(restoreLanguage);
    await page.getByRole('button', { name: /^Prism — (catalog|catalogo)$/ }).click();
  }
  if (testRoot && prismTestRoot() === testRoot) rmSync(join(testRoot, DIR_NAME), { recursive: true, force: true });
});

test('catalog: cards, filters, list view, whole-catalog block, panels', async () => {
  const filters = page.getByRole('group', { name: 'Show' });
  const all = Number((await filters.getByRole('button', { name: /^All/ }).innerText()).replace(/\D/g, ''));
  expect(all).toBeGreaterThan(0);
  await expect(page.getByRole('article')).toHaveCount(all);
  const whole = page.getByRole('region', { name: 'Whole catalog' });
  await expect(whole).not.toContainText('…', { timeout: 60_000 });
  await expect(page.getByRole('region', { name: 'In progress' })).toBeVisible();
  await expect(page.getByRole('region', { name: 'Last actions' })).toBeVisible();
  await shot('01-catalog');
  await page.getByRole('button', { name: 'List' }).click();
  await expect(page.getByRole('table')).toBeVisible();
  await shot('02-catalog-list');
  await page.getByRole('button', { name: 'Cards' }).click();
  for (const f of ['Connected', 'Network', 'In the drawer']) {
    await filters.getByRole('button', { name: new RegExp(`^${f}`) }).click();
  }
  await filters.getByRole('button', { name: /^All/ }).click();
  await expect(page.getByRole('article')).toHaveCount(all);
});

test('new disks: the proposal opens and closes without indexing', async () => {
  const pill = page.getByRole('button', { name: /new disks?$/ });
  test.skip((await pill.count()) === 0, 'no new disk on this PC');
  await pill.click();
  await expect(page.getByRole('dialog', { name: 'New disks' })).toBeVisible();
  await shot('03-new-disks');
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog', { name: 'New disks' })).toHaveCount(0);
});

test('every tab of an indexed real disk renders (read-only)', async () => {
  const indexed = await page.getByRole('article').filter({ hasText: /Updated/ }).filter({ hasNotText: 'PRISMTEST' }).first();
  const name = (await indexed.getAttribute('aria-label'))!;
  await openDisk(name);
  await expect(page.getByRole('figure', { name: /Space map/ })).toBeVisible();
  await expect(page.getByRole('table', { name: 'Folder content, largest first' }).locator('tbody tr').first()).toBeVisible();
  await shot('10-disk-space');
  expect(await wider()).toBe(0);
  // The name of the first folder: a file row has only "Open" and "Show in Explorer" (never pressed).
  const firstFolder = page.getByRole('table', { name: 'Folder content, largest first' }).locator('tbody tr button.name').first();
  if ((await firstFolder.count()) > 0) {
    await firstFolder.click();
    await expect(page.getByRole('navigation', { name: 'Folder path' }).locator('[aria-current="location"]')).toBeVisible();
    await page.getByRole('button', { name: 'Parent folder' }).click();
  }
  await page.getByRole('button', { name: 'Largest files' }).click();
  await expect(page.getByRole('table', { name: 'Largest files' }).locator('tbody tr').first()).toBeVisible();
  for (const tab of ['Duplicates', 'Similar', 'Backup', 'Cleanup', 'Health', 'File types']) {
    await tabs().getByRole('button', { name: tab, exact: true }).click();
    await page.waitForTimeout(1_500);
    await expect(page.getByRole('alert')).toHaveCount(0);
    await shot(`11-disk-${tab.toLowerCase().replace(' ', '-')}`);
    expect(await wider(), `${tab} is wider than the window`).toBe(0);
  }
  await expect(page.getByRole('heading', { name: 'By category' })).toBeVisible({ timeout: 60_000 });
});

test('whole catalog: duplicates, similar, backup, file types', async () => {
  await home();
  for (const [cell, name] of [['Duplicates', 'duplicates'], ['Similar', 'similar'], ['Backup', 'backup'], ['Catalogued', 'types']] as const) {
    await home();
    await page.getByRole('region', { name: 'Whole catalog' }).getByRole('button', { name: new RegExp(`^${cell} →`) }).click();
    await page.waitForTimeout(1_500);
    await expect(page.getByRole('alert')).toHaveCount(0);
    await shot(`20-whole-${name}`);
    expect(await wider(), `${cell} is wider than the window`).toBe(0);
  }
});

test('search from the top bar, with the results of every disk', async () => {
  await home();
  await page.getByRole('searchbox', { name: 'Search every disk' }).fill('w');
  const box = page.getByRole('textbox', { name: 'Search all disks' });
  await expect(box).toBeFocused();
  await box.fill('windows');
  await expect(page.getByRole('listbox', { name: 'Results' }).getByRole('option').first()).toBeVisible();
  await shot('30-search');
  await box.fill('colour:red');
  await expect(page.getByRole('alert')).toBeVisible();
});

test('PRISMTEST: the test folder appears after an update', async () => {
  test.skip(testRoot === null, 'the PRISMTEST test disk is not mounted');
  prepareTestFolder(testRoot!);
  await openDisk('PRISMTEST');
  await page.getByRole('button', { name: 'Update', exact: true }).click();
  await expect(page.getByRole('table', { name: 'Folder content, largest first' }).getByRole('button', { name: DIR_NAME, exact: true })).toBeVisible({ timeout: 120_000 });
  await shot('40-prismtest-space');
});

test('PRISMTEST: Explorer menu "Analyse with Prism" opens the folder', async () => {
  test.skip(testRoot === null, 'the PRISMTEST test disk is not mounted');
  const shell = 'D:\\Progetti\\Github\\Prism-next\\target\\debug\\prism-shell.exe';
  test.skip(!existsSync(shell), 'prism-shell.exe is not built (cargo build --workspace)');
  await home();
  execFileSync(shell, ['analyze', join(testRoot!, DIR_NAME)]);
  await expect(page.getByRole('heading', { name: 'PRISMTEST', level: 1 })).toBeVisible();
  await expect(page.getByRole('navigation', { name: 'Folder path' }).locator('[aria-current="location"]')).toHaveText(DIR_NAME);
});

test('PRISMTEST: search finds the new files', async () => {
  test.skip(testRoot === null, 'the PRISMTEST test disk is not mounted');
  await home();
  await page.getByRole('searchbox', { name: 'Search every disk' }).fill('p');
  const box = page.getByRole('textbox', { name: 'Search all disks' });
  // The search index of a volume is rebuilt at most every 30 s (REVIEW L11).
  await expect(async () => {
    await box.fill('');
    await box.fill('prism-real-dup');
    await expect(page.getByRole('listbox', { name: 'Results' }).getByRole('option')).toHaveCount(2, { timeout: 3_000 });
  }).toPass({ timeout: 90_000 });
  await expect(page.getByRole('listbox', { name: 'Results' })).toContainText('PRISMTEST');
});

test('PRISMTEST: name, location and colour, then put back', async () => {
  test.skip(testRoot === null, 'the PRISMTEST test disk is not mounted');
  await home();
  const before = (await card('PRISMTEST').locator('.where').textContent())?.trim() ?? '';
  await card('PRISMTEST').getByRole('button', { name: 'Actions for PRISMTEST' }).click();
  await page.getByRole('menuitem', { name: 'Name, location and colour…' }).click();
  const dialog = page.getByRole('dialog', { name: 'Name, location and colour' });
  const location = dialog.getByLabel('Where it is');
  const previous = await location.inputValue();
  await location.fill('Test bench');
  await dialog.getByRole('button', { name: 'Save' }).click();
  await expect(card('PRISMTEST')).toContainText('Test bench');
  await card('PRISMTEST').getByRole('button', { name: 'Actions for PRISMTEST' }).click();
  await page.getByRole('menuitem', { name: 'Name, location and colour…' }).click();
  await dialog.getByLabel('Where it is').fill(previous);
  await dialog.getByRole('button', { name: 'Save' }).click();
  await expect(card('PRISMTEST').locator('.where')).toHaveText(before);
});

test('PRISMTEST: a duplicate goes to the Recycle Bin and the other copy stays', async () => {
  test.skip(testRoot === null, 'the PRISMTEST test disk is not mounted');
  const a = join(testRoot!, DIR_NAME, 'dup\\a\\prism-real-dup.bin');
  const b = join(testRoot!, DIR_NAME, 'dup\\b\\prism-real-dup.bin');
  await openDisk('PRISMTEST', 'Duplicates');
  await page.getByRole('textbox', { name: 'Path contains' }).fill(`${DIR_NAME}\\dup`);
  await page.getByRole('textbox', { name: 'Path contains' }).press('Enter');
  await page.getByRole('textbox', { name: 'Path contains' }).blur();
  // The content analysis reads the new files in the background, at the lowest priority.
  const group = page.getByRole('article', { name: 'prism-real-dup.bin' });
  await expect(async () => {
    await page.getByRole('button', { name: 'Refresh' }).click();
    await expect(group).toBeVisible({ timeout: 5_000 });
  }).toPass({ timeout: 6 * 60_000, intervals: [10_000] });
  await shot('41-prismtest-duplicates');
  // Keep the oldest (a): b is proposed.
  await expect(page.getByRole('region', { name: /^1 copy selected/ })).toBeVisible();
  await page.getByRole('button', { name: /Delete selected copies/ }).click();
  const confirm = page.getByRole('dialog', { name: /^Delete 1 copy/ });
  await expect(confirm).toContainText('go to the Recycle Bin');
  await confirm.getByRole('button', { name: 'Delete' }).click();
  await expect(page.getByRole('status').filter({ hasText: 'Deleted' })).toContainText('Deleted: 1.');
  expect(existsSync(a)).toBe(true);
  expect(existsSync(b)).toBe(false);
  // F16-AC6: in the last actions of the catalog.
  await home();
  await expect(page.getByRole('region', { name: 'Last actions' })).toContainText('PRISMTEST');
});

test('PRISMTEST: similar pictures show their previews', async () => {
  test.skip(testRoot === null, 'the PRISMTEST test disk is not mounted');
  await openDisk('PRISMTEST', 'Similar');
  // The prints of the pictures follow the content hashes, in the background.
  const group = page.getByRole('article', { name: /^prism-real-photo/ });
  await expect(async () => {
    await page.getByRole('button', { name: 'Refresh' }).click();
    await expect(group).toBeVisible({ timeout: 5_000 });
  }).toPass({ timeout: 6 * 60_000, intervals: [10_000] });
  const previews = group.getByRole('img');
  await expect(previews).toHaveCount(2);
  // Loaded and drawn, not blocked: the window allows only its own images (CSP).
  for (const img of await previews.all()) {
    await expect.poll(() => img.evaluate((e) => (e as HTMLImageElement).naturalWidth)).toBeGreaterThan(0);
  }
  await shot('42-prismtest-similar');
});

test('PRISMTEST: the empty folder is cleaned up into the Recycle Bin', async () => {
  test.skip(testRoot === null, 'the PRISMTEST test disk is not mounted');
  const empty = join(testRoot!, DIR_NAME, 'empty');
  await openDisk('PRISMTEST', 'Cleanup');
  const group = page.getByRole('article', { name: /^Empty folders · PRISMTEST/ });
  await expect(group).toBeVisible({ timeout: 60_000 });
  await shot('42-prismtest-cleanup');
  // Only the folder of the test: everything else is left alone.
  const toggle = group.getByRole('button', { name: /^Select (all|none)$/ });
  if ((await toggle.innerText()) === 'Select none') await toggle.click();
  else {
    await toggle.click();
    await group.getByRole('button', { name: 'Select none' }).click();
  }
  const mine = group.getByRole('checkbox', { name: new RegExp(`${DIR_NAME}\\\\empty`) });
  await expect(mine).toHaveCount(1);
  await mine.check();
  await group.getByRole('button', { name: /^Move to the Recycle Bin/ }).click();
  await page.getByRole('dialog').getByRole('button', { name: 'Move to the Recycle Bin' }).click();
  await expect(page.getByRole('status').filter({ hasText: 'Moved to the Recycle Bin' })).toContainText('Moved to the Recycle Bin: 1.');
  expect(existsSync(empty)).toBe(false);
});

test('PRISMTEST: backup check, then copy of the missing files', async () => {
  test.skip(testRoot === null, 'the PRISMTEST test disk is not mounted');
  const dst = join(testRoot!, DIR_NAME, 'dst');
  await openDisk('PRISMTEST', 'Backup');
  const source = page.getByRole('group', { name: 'Files to protect', exact: true });
  await source.getByRole('combobox', { name: 'Disk' }).selectOption({ label: 'PRISMTEST' });
  await source.getByRole('textbox', { name: 'Folder' }).fill(`${DIR_NAME}\\src`);
  const backup = page.getByRole('group', { name: 'Backup 1', exact: true });
  await backup.getByRole('combobox', { name: 'Disk' }).selectOption({ label: 'PRISMTEST' });
  await backup.getByRole('textbox', { name: 'Folder' }).fill(`${DIR_NAME}\\dst`);
  await page.getByRole('button', { name: 'Check', exact: true }).click();
  const result = page.getByRole('status', { name: /^Result for PRISMTEST/ });
  await expect(result).toContainText('Without a copy: 2 files');
  await shot('43-prismtest-backup');
  await page.getByRole('button', { name: /^Copy the missing files/ }).click();
  const dialog = page.getByRole('dialog', { name: 'Copy the missing files' });
  const to = dialog.getByRole('group', { name: 'Copy to', exact: true });
  await to.getByRole('combobox', { name: 'Disk' }).selectOption({ label: 'PRISMTEST' });
  await to.getByRole('textbox', { name: 'Folder' }).fill(`${DIR_NAME}\\dst`);
  await expect(dialog).toContainText('2 files');
  await dialog.getByRole('button', { name: 'Copy', exact: true }).click();
  await expect(page.getByRole('status', { name: 'Copy result' })).toContainText('Copied and checked: 2', { timeout: 60_000 });
  const copied = filesUnder(dst).map((p) => p.split('\\').pop());
  expect(copied).toEqual(expect.arrayContaining(['prism-real-file-1.bin', 'prism-real-file-2.bin', 'prism-real-file-3.bin']));
});

test('settings: sections, pause and resume the content analysis, dark theme', async () => {
  await openSettings();
  await expect(page.getByRole('heading', { name: 'Settings', level: 1 })).toBeVisible();
  await shot('50-settings');
  const pause = page.getByRole('checkbox', { name: 'Pause the content analysis' });
  const wasPaused = await pause.isChecked();
  await pause.setChecked(!wasPaused);
  await expect(page.locator('footer')).toContainText(wasPaused ? 'lowest priority' : 'paused');
  await pause.setChecked(wasPaused);
  await page.locator('#set-theme').selectOption('dark');
  await home();
  await shot('51-catalog-dark');
  await openDisk((await page.getByRole('article').filter({ hasText: /Updated/ }).first().getAttribute('aria-label'))!);
  await shot('52-disk-dark');
});
