import { expect, test, type Page } from "@playwright/test";

test.setTimeout(10_000);

async function openDesktopChart(page: Page) {
  await page.setViewportSize({ width: 1020, height: 833 });
  await page.goto("/?tradingE2E=1");
  await page.getByRole("navigation", { name: "桌面主导航" }).getByRole("button", { name: "个股", exact: true }).click();
  await page.getByRole("tab", { name: "日K", exact: true }).click();
  return page.locator(".shared-kline-host .msd-kline");
}

test("横竖屏与切股复用均线、副图和各股窗口；详情对齐保持局部关闭", async ({ page }) => {
  const desktop = await openDesktopChart(page);
  await desktop.getByRole("button", { name: /^MA5：/ }).click();
  await desktop.getByRole("button", { name: "MACD", exact: true }).click();
  await desktop.getByRole("button", { name: "放大K线", exact: true }).click();
  await desktop.getByRole("button", { name: "窗口左移", exact: true }).click();
  await expect(desktop).toHaveAttribute("data-capacity", "48");
  await expect(desktop).toHaveAttribute("data-offset", "12");
  await desktop.locator(".msd-candle-chart").click({ position: { x: 80, y: 50 } });
  await expect(desktop.getByRole("complementary", { name: "K线详细信息", exact: true })).toBeVisible();
  await page.setViewportSize({ width: 320, height: 844 });
  await page.getByRole("button", { name: /稳健实业 600101/ }).click();
  const mobile = page.locator(".mobile-detail-page .msd-kline");
  await expect(mobile.getByRole("button", { name: /^MA5：/ })).toHaveAttribute("aria-pressed", "false");
  await expect(mobile.getByRole("button", { name: "MACD", exact: true })).toHaveAttribute("aria-pressed", "true");
  await expect(mobile).toHaveAttribute("data-capacity", "48");
  await expect(mobile).toHaveAttribute("data-offset", "12");
  await expect(mobile.locator(".kline-crosshair")).toHaveCount(0);
  await mobile.getByRole("button", { name: "放大K线", exact: true }).click();
  await page.setViewportSize({ width: 1020, height: 833 });
  await expect(desktop).toHaveAttribute("data-capacity", "30");
  await expect(desktop).toHaveAttribute("data-offset", "12");
  const stocks = page.getByRole("navigation", { name: "个股列表" });
  await stocks.getByRole("button", { name: /芯片科技 002156/ }).click();
  await expect(desktop).toHaveAttribute("data-capacity", "72");
  await expect(desktop.getByRole("button", { name: /^MA5：/ })).toHaveAttribute("aria-pressed", "false");
  await expect(desktop.getByRole("button", { name: "MACD", exact: true })).toHaveAttribute("aria-pressed", "true");
  await stocks.getByRole("button", { name: /稳健实业 600101/ }).click();
  await expect(desktop).toHaveAttribute("data-capacity", "30");
  await expect(desktop).toHaveAttribute("data-offset", "12");
  await expect(desktop.locator(".kline-crosshair")).toHaveCount(0);
});

test("深色外壳的共用K线和盘口保持浅色数据面与清楚的文字、深度色", async ({ page }) => {
  const chart = await openDesktopChart(page);
  await page.getByRole("button", { name: "🌙", exact: true }).click();
  await expect(chart).toHaveCSS("background-color", "rgb(255, 255, 255)");
  await expect(chart.locator(".msd-volume-title")).toHaveCSS("color", "rgb(34, 34, 34)");
  await expect(chart.locator(".msd-kdj-title")).toHaveCSS("color", "rgb(34, 34, 34)");
  const book = page.getByLabel("五档盘口，数量单位为手");
  await expect(book.locator(".msd-book-row").first()).toHaveCSS("color", "rgb(34, 34, 34)");
  await expect(book.locator(".flat").first()).toHaveCSS("color", "rgb(119, 119, 119)");
  expect(await book.evaluate(el => el.ownerDocument.defaultView!.getComputedStyle(el).getPropertyValue("--msd-rise").trim())).toBe("#ef3f49");
  expect(await book.evaluate(el => el.ownerDocument.defaultView!.getComputedStyle(el).getPropertyValue("--msd-fall").trim())).toBe("#009b22");
  await page.setViewportSize({ width: 320, height: 844 });
  await page.getByRole("button", { name: /稳健实业 600101/ }).click();
  await expect(page.locator(".mobile-detail-page .msd-volume-title")).toHaveCSS("color", "rgb(34, 34, 34)");
  expect(await page.locator("html").evaluate(el => el.scrollWidth > el.clientWidth)).toBe(false);
});
