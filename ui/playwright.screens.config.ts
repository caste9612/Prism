import { defineConfig } from '@playwright/test';

// The screenshots of the README (docs/screenshots/), taken from the interface with the simulated
// backend: fictional disks and files, never anyone's real data. `npm run screens`.
export default defineConfig({
  testDir: 'screens',
  workers: 1,
  reporter: 'list',
  timeout: 60_000,
  use: {
    baseURL: 'http://localhost:5174',
    headless: true,
    locale: 'en-US',
    colorScheme: 'light',
    viewport: { width: 1280, height: 800 },
    deviceScaleFactor: 2,
  },
  webServer: {
    command: 'npx vite --port 5174 --strictPort',
    url: 'http://localhost:5174',
    reuseExistingServer: true,
    timeout: 60_000,
  },
});
