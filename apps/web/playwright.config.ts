import { defineConfig } from "@playwright/test";

const corepack = process.env.COREPACK_BIN ?? "corepack";
const port = process.env.PLAYWRIGHT_PORT ?? "4173";
const baseURL = `http://127.0.0.1:${port}`;

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: false,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 2 : 0,
  reporter: process.env.CI ? "github" : "list",
  use: {
    baseURL,
    browserName: "chromium",
    trace: process.env.PLAYWRIGHT_TRACE === "on" ? "on" : "retain-on-failure",
  },
  webServer: {
    command: `${corepack} pnpm build && ${corepack} pnpm preview --host 127.0.0.1 --port ${port}`,
    url: baseURL,
    reuseExistingServer: !process.env.CI,
    timeout: 180_000,
  },
});
