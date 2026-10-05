import { expect, test } from "@playwright/test";
import { readFile } from "node:fs/promises";
import { createServer, type ViteDevServer } from "vite";

let vite: ViteDevServer;
let renderQuote: (volume: number) => string;
let css: string;

test.beforeAll(async () => {
  vite = await createServer({ configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  const [{ createElement }, { renderToStaticMarkup }] = await Promise.all(["react", "react-dom/server"].map((specifier) => import(specifier)));
  const { MobileStockDetail } = await vite.ssrLoadModule("/src/mobile/MobileStockDetail.tsx");
  const { ChartSettingsFixture } = await vite.ssrLoadModule("/src/test-support/ChartSettingsFixture.tsx");
  css = await readFile(new URL("../src/mobile/MobileStockDetail.css", import.meta.url), "utf8");
  renderQuote = (volume) => renderToStaticMarkup(createElement(ChartSettingsFixture, null, createElement(MobileStockDetail, {
    code: "600101", name: "稳健实业",
    market: { last_price: "1232", last_close: "1120", best_bid: "1232", best_ask: "1233", bids: [], asks: [] },
    minutePoints: [], auctionPoints: [], dailyCandles: [],
    activeDailyCandle: { time: 0, open: 11.2, high: 12.32, low: 11.2, close: 12.32, rawPrices: { open: "1120", high: "1232", low: "1120", close: "1232" }, volume },
    trades: [], elapsedMinutes: 19, totalMinutes: 240, period: "分时", infoTab: "盘口", indicatorCalculator: null,
    speed: 1, measuredSpeed: "实测 0x", running: false, gameDay: 1, gameTick: 901,
    onPeriodChange() {}, onInfoTabChange() {}, onSpeedChange() {}, onPauseToggle() {}, onBack() {}, onPrevious() {}, onNext() {},
  })));
});

test.afterAll(async () => { if (vite) await vite.close(); });

for (const width of [320, 390]) {
  test(`${width}px 报价摘要完整显示成交量与单位，不截断行情数值`, async ({ page }) => {
    test.setTimeout(10_000);
    await page.setViewportSize({ width, height: 844 });
    // 使用实际组件和CSS，只隔离报价几何；fixture不冒称真实WASM成交。
    for (const [volume, displayed] of [[289300, "2893手"], [999999, "9999.99手"], [0, "0手"]] as const) {
      await page.setContent(`<style>body{margin:0}${css}</style>${renderQuote(volume)}`);
      const summary = page.getByRole("region", { name: "股票报价摘要" });
      await expect(summary).toContainText(displayed);
      const geometry = await summary.locator(".msd-stock-stats b").evaluateAll((values) => values.map((value) => {
        const rect = value.getBoundingClientRect();
        const cell = value.parentElement!.getBoundingClientRect();
        return { text: value.textContent, hiddenWidth: value.scrollWidth - value.clientWidth, left: rect.left - cell.left, right: cell.right - rect.right };
      }));
      for (const value of geometry) {
        expect(value.hiddenWidth, `${value.text}不应省略`).toBeLessThanOrEqual(1);
        expect(value.left).toBeGreaterThanOrEqual(-1);
        expect(value.right).toBeGreaterThanOrEqual(-1);
      }
      const priceText = await summary.locator(".msd-last span, .msd-day-prices span").evaluateAll((cells) => cells.map((cell) => {
        const range = cell.ownerDocument.createRange();
        range.selectNodeContents(cell);
        const text = range.getBoundingClientRect();
        const column = cell.parentElement!.getBoundingClientRect();
        return { text: cell.textContent, left: text.left - column.left, right: column.right - text.right };
      }));
      for (const value of priceText) {
        expect(value.left, `${value.text}保持在报价列内`).toBeGreaterThanOrEqual(-1);
        expect(value.right, `${value.text}不覆盖相邻报价列`).toBeGreaterThanOrEqual(-1);
      }
      expect(await page.locator("html").evaluate((element) => element.scrollWidth)).toBe(width);
    }
  });
}
