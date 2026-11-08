import { readFileSync, writeFileSync } from 'node:fs';
import { expect, test, type Browser, type Page } from '@playwright/test';
import { diskTab, openCatalog, openDisk, openWhole } from '../e2e/nav';

// Screenshots for the README, from the simulated backend (invented disks and files). Not a test:
// run with `npm run screens`; the images go to docs/screenshots/.
//
// Every screen is taken at 1440×900 (a common laptop window), then placed in a window frame —
// title bar, rounded corners, shadow — on a soft background, so all the images have the same
// landscape shape.

const OUT = '../docs/screenshots';
const ICON = readFileSync('../crates/prism-app/icons/32x32.png').toString('base64');

/** The questions about ambiguous disks of the simulated catalog would cover the first screen. */
async function withoutQuestions(page: Page) {
  await page.addStyleTag({ content: '.identify { display: none !important; }' });
}

async function settle(page: Page) {
  await page.waitForLoadState('networkidle');
  await page.waitForTimeout(800);
}

/** The window frame around a screenshot, in the light or dark theme. */
function frame(png: Buffer, dark: boolean): string {
  const c = dark
    ? { bg: '#0b0d10', glow1: 'rgba(240,78,35,.20)', glow2: 'rgba(91,157,255,.16)', bar: '#16191e', text: '#d6dae0', line: 'rgba(255,255,255,.08)' }
    : { bg: '#e6e9ee', glow1: 'rgba(240,78,35,.16)', glow2: 'rgba(91,157,255,.18)', bar: '#ffffff', text: '#3a4049', line: 'rgba(17,20,24,.10)' };
  return `<!doctype html><html><head><style>
    html, body { margin: 0; }
    body {
      width: 1600px; height: 1056px; display: grid; place-items: center;
      background:
        radial-gradient(900px 600px at 0% 0%, ${c.glow1}, transparent 70%),
        radial-gradient(900px 600px at 100% 100%, ${c.glow2}, transparent 70%),
        ${c.bg};
      font-family: 'Segoe UI Variable Text', 'Segoe UI', system-ui, sans-serif;
    }
    .window {
      width: 1440px; border-radius: 12px; overflow: hidden;
      border: 1px solid ${c.line};
      box-shadow: 0 30px 80px rgba(17,20,24,${dark ? '.55' : '.22'}), 0 6px 18px rgba(17,20,24,${dark ? '.4' : '.10'});
    }
    .bar {
      height: 36px; display: flex; align-items: center; gap: 10px; padding: 0 0 0 14px;
      background: ${c.bar}; color: ${c.text}; font-size: 13px; border-bottom: 1px solid ${c.line};
    }
    .bar img { width: 16px; height: 16px; }
    .bar .title { flex: 1; }
    .bar .ctl { width: 46px; height: 36px; display: grid; place-items: center; opacity: .75; }
    .bar svg { stroke: ${c.text}; stroke-width: 1; fill: none; }
    .shot { display: block; width: 1440px; height: 900px; }
  </style></head><body>
    <div class="window">
      <div class="bar">
        <img src="data:image/png;base64,${ICON}" alt="" />
        <span class="title">Prism</span>
        <span class="ctl"><svg width="10" height="10"><path d="M0 5.5h10" /></svg></span>
        <span class="ctl"><svg width="10" height="10"><rect x=".5" y=".5" width="9" height="9" /></svg></span>
        <span class="ctl"><svg width="10" height="10"><path d="M.5 .5l9 9M9.5 .5l-9 9" /></svg></span>
      </div>
      <img class="shot" src="data:image/png;base64,${png.toString('base64')}" alt="" />
    </div>
  </body></html>`;
}

/** Takes the screen as it is now and writes it, framed, to docs/screenshots/<name>.png. */
async function shoot(browser: Browser, page: Page, name: string, dark = false) {
  const png = await page.screenshot();
  const context = await browser.newContext({ viewport: { width: 1600, height: 1056 }, deviceScaleFactor: 1.25 });
  const framePage = await context.newPage();
  await framePage.setContent(frame(png, dark));
  await framePage.waitForLoadState('load');
  writeFileSync(`${OUT}/${name}.png`, await framePage.screenshot());
  await context.close();
}

test('catalog', async ({ browser, page }) => {
  await openCatalog(page);
  await withoutQuestions(page);
  await settle(page);
  await shoot(browser, page, 'catalog');
});

test('catalog, dark', async ({ browser, page }) => {
  await page.emulateMedia({ colorScheme: 'dark' });
  await openCatalog(page);
  await withoutQuestions(page);
  await settle(page);
  await shoot(browser, page, 'catalog-dark', true);
});

test('space of a disk', async ({ browser, page }) => {
  await openCatalog(page);
  await openDisk(page, 'Data', 'Space');
  await settle(page);
  await shoot(browser, page, 'space');
});

test('duplicates', async ({ browser, page }) => {
  await openCatalog(page);
  await openWhole(page, 'Duplicates');
  await settle(page);
  await shoot(browser, page, 'duplicates');
});

test('similar pictures', async ({ browser, page }) => {
  await openCatalog(page);
  await openWhole(page, 'Similar');
  await settle(page);
  await shoot(browser, page, 'similar');
});

test('search', async ({ browser, page }) => {
  await openCatalog(page);
  await page.getByRole('searchbox', { name: 'Search every disk' }).fill('f');
  const box = page.getByRole('textbox', { name: 'Search all disks' });
  await box.fill('foto');
  await expect(page.getByRole('listbox', { name: 'Results' }).getByRole('option').first()).toBeVisible();
  await settle(page);
  await shoot(browser, page, 'search');
});

test('health of a disk', async ({ browser, page }) => {
  await openCatalog(page);
  await openDisk(page, 'Data');
  await diskTab(page, 'Health');
  await settle(page);
  await shoot(browser, page, 'health');
});
