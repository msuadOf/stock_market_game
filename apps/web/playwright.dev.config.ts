import { defineConfig } from "@playwright/test";
import config from "./playwright.config.ts";

const port = process.env.PLAYWRIGHT_PORT ?? "4182";
const baseURL = `http://127.0.0.1:${port}`;

// DEV 专属入口需在真实 Vite 开发环境验收，不能用 production 预览冒充。
export default defineConfig({
  ...config,
  testDir: "./e2e-dev",
  use: { ...config.use, baseURL },
  webServer: {
    command: `pnpm exec vite --mode e2e --host 127.0.0.1 --port ${port} --strictPort`,
    url: baseURL,
    reuseExistingServer: false,
    timeout: 30_000,
  },
});
