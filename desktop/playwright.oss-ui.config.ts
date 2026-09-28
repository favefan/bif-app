// Resolve from the existing tests/e2e lockfile; desktop adds no production or test dependency.
import { defineConfig, devices } from "../tests/e2e/node_modules/@playwright/test";

// Standalone, disposable-server check: deliberately no upstream globalSetup or webServer.
export default defineConfig({
  testDir: "../tests/e2e/features/desktop",
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  reporter: [["list"]],
  timeout: 60_000,
  expect: { timeout: 10_000 },
  use: {
    baseURL: process.env.BASE_URL,
    ...devices["Desktop Chrome"],
    channel: process.env.PLAYWRIGHT_CHANNEL || "chrome",
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  projects: [{ name: "desktop-oss-ui", use: { ...devices["Desktop Chrome"] } }],
});