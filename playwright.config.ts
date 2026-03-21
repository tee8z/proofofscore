import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  workers: 1,
  reporter: "html",
  timeout: 120_000,

  use: {
    baseURL: "http://127.0.0.1:8901",
    trace: "on-first-retry",
    screenshot: "only-on-failure",
  },

  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
    {
      name: "mobile-chrome",
      use: { ...devices["Pixel 7"] },
    },
    {
      name: "mobile-iphone",
      use: {
        // iPhone 14 viewport with Chromium (WebKit deps not always available)
        ...devices["iPhone 14"],
        defaultBrowserType: "chromium",
      },
    },
  ],

  webServer: {
    command: process.env.E2E_SERVER_BIN
      ? `${process.env.E2E_SERVER_BIN} --config config/test.toml`
      : "just build-wasm && cargo run --bin server -- --config config/test.toml",
    port: 8901,
    reuseExistingServer: !process.env.CI,
    timeout: 180_000,
    stdout: "pipe",
    stderr: "pipe",
  },
});
