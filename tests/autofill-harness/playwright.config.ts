import { defineConfig } from "@playwright/test";

// The extension under test is `extension/dist-test` (`pnpm --filter
// @pwm/extension build:test`), talking to a sync server on
// http://127.0.0.1:8080 (`just server`). Fixture pages are served over
// HTTPS by fixtures/serve.ts, started below.
export default defineConfig({
  testDir: "tests",
  // One browser profile with one account; the tests share it in order.
  workers: 1,
  fullyParallel: false,
  timeout: 60_000,
  reporter: [["list"], ["html", { open: "never" }]],
  webServer: {
    command: "node fixtures/serve.ts",
    url: "https://127.0.0.1:8443/health",
    ignoreHTTPSErrors: true,
    reuseExistingServer: !process.env.CI,
    timeout: 30_000,
  },
});
