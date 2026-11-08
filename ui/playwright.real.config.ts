import { defineConfig } from '@playwright/test';

// Pre-release check of the REAL development app (not the simulated backend): the tests attach to
// the WebView2 of a running `prism.exe` through its debug port. See docs/TESTING.md §6.
//
//   $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9333'
//   .\target\debug\prism.exe            # debug build: deletions are limited to the sandbox
//   npm --prefix ui run e2e:real
//
// Everything that changes files happens only on the PRISMTEST test disk (checked by label),
// in a folder the tests create and remove.
export default defineConfig({
  testDir: 'e2e-real',
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: 'list',
  timeout: 8 * 60_000,
  expect: { timeout: 20_000 },
  use: { actionTimeout: 30_000 },
});
