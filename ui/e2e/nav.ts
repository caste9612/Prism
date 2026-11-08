import { expect, type Page } from '@playwright/test';

// The paths of the B+C interface (ADR-0019) shared by the end-to-end tests: everything starts
// from the catalog, from a disk or from the "whole catalog" block.

export const card = (page: Page, name: string) => page.getByRole('article', { name });

export async function openCatalog(page: Page) {
  await page.goto('/');
  // The first load of a page compiles the interface in the development server: on a busy PC it
  // can take more than the default 5 s (a run failed so while the installed Prism was indexing).
  await expect(page.getByRole('heading', { name: 'Disks', level: 1 })).toBeVisible({ timeout: 20_000 });
}

/** Opens the screen of a disk from its card, on a tab (the first one, Space, if none). */
export async function openDisk(page: Page, name: string, tab?: string) {
  await card(page, name).getByRole('button', { name, exact: true }).click();
  await expect(page.getByRole('heading', { name, level: 1 })).toBeVisible();
  if (tab) await diskTab(page, tab);
}

export async function diskTab(page: Page, tab: string) {
  const nav = page.getByRole('navigation', { name: 'Functions of the disk' });
  await nav.getByRole('button', { name: tab, exact: true }).click();
  await expect(nav.getByRole('button', { name: tab, exact: true })).toHaveAttribute('aria-current', 'page');
}

/** A function on the whole catalog, from its block: Duplicates, Similar, Backup, Catalogued. */
export async function openWhole(page: Page, cell: string) {
  await page.getByRole('region', { name: 'Whole catalog' }).getByRole('button', { name: new RegExp(`^${cell} →`) }).click();
}

export async function openSettings(page: Page) {
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Settings', level: 1 })).toBeVisible();
}

/** Back to the catalog from anywhere, with the path in the top bar. */
export async function backToCatalog(page: Page, label = 'Catalog') {
  await page.getByRole('button', { name: label, exact: true }).click();
}

/** Opens the menu "⋯" of a card and returns it. */
export async function cardMenu(page: Page, name: string) {
  await card(page, name).getByRole('button', { name: `Actions for ${name}` }).click();
  return page.getByRole('menu', { name: `Actions for ${name}` });
}
