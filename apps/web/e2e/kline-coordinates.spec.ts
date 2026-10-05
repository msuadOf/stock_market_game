import { expect, test, type Locator, type Page } from "@playwright/test";

test.setTimeout(10_000);

async function openChart(page: Page, width: number) {
  await page.setViewportSize({ width, height: 833 });
  await page.goto("/?tradingE2E=1");
  await page.getByRole("navigation", { name: "桌面主导航" }).getByRole("button", { name: "个股", exact: true }).click();
  await page.getByRole("tab", { name: "日K", exact: true }).click();
  const chart = page.locator(".shared-kline-host .msd-kline");
  await expect(chart.getByLabel("KDJ坐标", { exact: true })).toBeVisible();
  return chart;
}

async function expectAlignedPlots(chart: Locator) {
  const bounds = await chart.locator(".kline-plot-canvas").evaluateAll(elements => elements.map(element => {
    const rect = element.getBoundingClientRect();
    return { x: rect.x, width: rect.width };
  }));
  expect(bounds).toHaveLength(3);
  for (const bound of bounds) {
    expect(Math.abs(bound.x - bounds[0].x)).toBeLessThan(.1);
    expect(Math.abs(bound.width - bounds[0].width)).toBeLessThan(.1);
  }
  const lineX = await chart.locator(".kline-crosshair").evaluateAll(elements => elements.map(element => element.getBoundingClientRect().x));
  if (lineX.length > 0) {
    expect(lineX).toHaveLength(3);
    for (const x of lineX) expect(Math.abs(x - lineX[0])).toBeLessThan(.1);
  }
}

test("桌面坐标文字不随缩放拉伸，价量指标和点选交易日同步吸附", async ({ page }) => {
  const chart = await openChart(page, 1020);
  const axis = chart.getByLabel("价格坐标，单位为元", { exact: true });
  await expect(axis.locator("span").first()).toHaveCSS("font-size", "11px");
  await expectAlignedPlots(chart);
  await chart.locator(".msd-candle-chart").click({ position: { x: 80, y: 50 } });
  await expect(chart.locator(".kline-selected-time")).toBeVisible();
  await expectAlignedPlots(chart);
  await chart.getByRole("button", { name: "放大K线", exact: true }).click();
  await expect(chart).toHaveAttribute("data-capacity", "48");
  await expect(axis.locator("span").first()).toHaveCSS("font-size", "11px");
  await chart.getByRole("button", { name: "MACD", exact: true }).click();
  await expect(chart.getByLabel("MACD坐标", { exact: true })).toBeVisible();
  await expectAlignedPlots(chart);
  await expect(chart.getByLabel("游戏交易日坐标", { exact: true })).not.toContainText("1970");
});

test("当前桌面窗口下单栏展开后周K坐标和底部指标按钮完整可见", async ({ page }) => {
  const chart = await openChart(page, 902);
  await page.getByRole("button", { name: "买入此股票", exact: true }).click();
  await page.getByRole("tab", { name: "周K", exact: true }).click();
  const bounds = await chart.boundingBox();
  const controls = await chart.locator(".shared-indicator-controls").boundingBox();
  const axis = await chart.getByLabel("KDJ坐标", { exact: true }).boundingBox();
  if (!bounds || !controls || !axis) throw new Error("坐标或指标按钮缺失");
  expect(controls.y + controls.height).toBeLessThanOrEqual(bounds.y + bounds.height + .1);
  expect(axis.y + axis.height).toBeLessThanOrEqual(bounds.y + bounds.height + .1);
  for (const coordinate of await chart.locator(".kline-coordinate-axis:not(.kline-coordinate-mirror)").all()) {
    const labels = await coordinate.evaluate(element => {
      const outer = element.getBoundingClientRect();
      return [...element.querySelectorAll("span")].filter(span => span.getBoundingClientRect().height > 0).map(span => {
        const rect = span.getBoundingClientRect();
        return { top: rect.top - outer.top, bottom: rect.bottom - outer.bottom };
      });
    });
    for (const label of labels) {
      expect(label.top).toBeGreaterThanOrEqual(-.1);
      expect(label.bottom).toBeLessThanOrEqual(.1);
    }
    const positions = await coordinate.evaluate(element => [...element.querySelectorAll("span")].map(span => span.getBoundingClientRect()).filter(rect => rect.height > 0).map(rect => ({ top: rect.top, bottom: rect.bottom })));
    for (let index = 1; index < positions.length; index++) expect(positions[index].top).toBeGreaterThanOrEqual(positions[index - 1].bottom - .1);
  }
  await expectAlignedPlots(chart);
  expect(await page.locator("html").evaluate(el => el.scrollWidth > el.clientWidth)).toBe(false);
});

test("320px手机复用同一坐标组件，省略左侧镜像且不发生横向溢出", async ({ page }) => {
  await openChart(page, 1020);
  await page.setViewportSize({ width: 320, height: 844 });
  await page.getByRole("button", { name: /稳健实业 600101/ }).click();
  const chart = page.locator(".mobile-detail-page .msd-kline");
  await expect(chart.getByLabel("价格坐标，单位为元", { exact: true })).toBeVisible();
  await expect(chart.locator(".kline-coordinate-mirror").first()).toBeHidden();
  await expectAlignedPlots(chart);
  await chart.locator(".msd-candle-chart").click({ position: { x: 60, y: 40 } });
  await expect(chart.locator(".kline-selected-time")).toBeVisible();
  await expectAlignedPlots(chart);
  expect(await page.locator("html").evaluate(el => el.scrollWidth > el.clientWidth)).toBe(false);
});
