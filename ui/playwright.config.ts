import { defineConfig } from '@playwright/test';

// The interface code is typed for the browser only (no Node types): declare the one Node global
// this file needs.
declare const process: { env: Record<string, string | undefined> };

const CI = !!process.env.CI;

// End-to-end tests of the interface in a real browser with the simulated backend (the page runs
// outside Tauri, so `src/lib/api.ts` picks `mockBackend`). Browser: Playwright's Chromium
// (`npx playwright install chromium --only-shell`, the engine of WebView2).
export default defineConfig({
  testDir: 'e2e',
  fullyParallel: true,
  forbidOnly: CI,
  retries: 0,
  reporter: CI ? 'github' : 'list',
  timeout: 30_000,
  use: {
    baseURL: 'http://localhost:5174',
    headless: true,
    locale: 'en-US',
    colorScheme: 'light',
    viewport: { width: 1200, height: 800 },
  },
  webServer: {
    // A port different from the one used by `tauri dev` (5173), so both can run together.
    command: 'npx vite --port 5174 --strictPort',
    url: 'http://localhost:5174',
    reuseExistingServer: !CI,
    timeout: 60_000,
  },
});
