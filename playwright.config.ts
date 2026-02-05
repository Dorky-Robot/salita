import { defineConfig, devices } from "@playwright/test";

/**
 * Playwright E2E Test Configuration for Salita
 *
 * Builds the Rust binary, starts the server on a test port,
 * and runs browser tests against it.
 */
export default defineConfig({
  testDir: "./e2e",
  testMatch: "**/*.spec.ts",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: process.env.CI ? "list" : "html",
  use: {
    baseURL: "http://localhost:3099",
    trace: "on-first-retry",
    screenshot: "only-on-failure",
  },

  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
      testIgnore: /passkey/,
    },
    {
      name: "Mobile Chrome",
      use: { ...devices["Pixel 5"] },
      testIgnore: /passkey/,
    },
    {
      name: "passkey",
      use: {
        channel: "chrome",
        headless: false,
      },
      testMatch: /passkey/,
      timeout: 120_000,
    },
  ],

  webServer: {
    command:
      "cargo build --release && DATA_DIR=$(mktemp -d) && ./target/release/salita --port 3099 --data-dir $DATA_DIR",
    url: "http://localhost:3099",
    reuseExistingServer: !process.env.CI,
    timeout: 180 * 1000,
  },
});
