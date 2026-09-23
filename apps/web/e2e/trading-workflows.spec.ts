import { expect, test, type Locator, type Page } from "@playwright/test";

const STOCK_CODE = "600101";
const AUCTION_PRICE = "10.08";
const CONTINUOUS_PRICE = "10.98";
type E2EGlobal = typeof globalThis & {
  __STOCK_GAME_E2E__?: {
    advanceToTick(target: number): Promise<number>;
    snapshot(): { accounts: Record<string, { cash: number }> };
  };
};

async function expectEngineReady(page: Page): Promise<void> {
  await expect(page.locator(".app-root")).toBeVisible({ timeout: 30_000 });
  await expect(page.locator(".app-error")).toHaveCount(0);
  await page.waitForFunction(() => Boolean((globalThis as E2EGlobal).__STOCK_GAME_E2E__));
  await expect(page.locator(".app-root")).toHaveAttribute("data-game-tick", "0");
}

async function openGame(page: Page): Promise<void> {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/?tradingE2E=1");
  await expectEngineReady(page);
}

async function advanceToTick(page: Page, target: number): Promise<number> {
  return page.evaluate(async (nextTick) => {
    const controls = (globalThis as E2EGlobal).__STOCK_GAME_E2E__;
    if (!controls) throw new Error("E2E engine controls are unavailable");
    return controls.advanceToTick(nextTick);
  }, target);
}

function orderPanel(page: Page): Locator {
  return page.locator("#section-order");
}

async function fillLimitBuy(page: Page, price: string, quantity = "100"): Promise<void> {
  const panel = orderPanel(page);
  await panel.getByLabel("委托类型").selectOption("limit");
  await panel.getByPlaceholder("委托价").fill(price);
  await panel.getByPlaceholder("买入按手；零股一次卖完").fill(quantity);
  await panel.getByRole("button", { name: "买入", exact: true }).click();
}

function playerOrder(page: Page): Locator {
  return page.locator(".player-order-item").filter({ hasText: STOCK_CODE });
}

function availableCash(page: Page): Locator {
  return page.locator(".assets .asset").filter({ hasText: "可用资金" }).locator(".value");
}

function normalizeVisibleText(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

test("开盘集合竞价明确拒绝市价委托", async ({ page }) => {
  await openGame(page);
  const panel = orderPanel(page);
  await panel.getByLabel("委托类型").selectOption("market");
  await expect(panel.getByPlaceholder("市价委托无需价格")).toBeDisabled();
  await panel.getByRole("button", { name: "买入", exact: true }).click();

  await advanceToTick(page, 1);

  await expect(page.locator(".notice")).toContainText("集合竞价仅接受限价委托");
  await expect(playerOrder(page)).toHaveCount(0);
});

test("09:15–09:20 的未成交限价委托可撤销并释放冻结", async ({ page }) => {
  await openGame(page);
  await fillLimitBuy(page, AUCTION_PRICE);
  await advanceToTick(page, 1);

  const order = playerOrder(page);
  await expect(order).toContainText("资金已冻结");
  await expect(order).toContainText(`${AUCTION_PRICE} 元`);
  await expect(order).toContainText("100 股");
  await order.getByRole("button", { name: "撤单" }).click();
  await advanceToTick(page, 2);

  await expect(order).toHaveCount(0);
  await expect(page.getByText("暂无活动委托；限价单未成交时会显示在这里。")).toBeVisible();
});

test("09:20–09:25 的集合竞价委托不可撤销且继续冻结", async ({ page }) => {
  await openGame(page);
  await fillLimitBuy(page, AUCTION_PRICE);
  await advanceToTick(page, 1);
  const order = playerOrder(page);
  await expect(order).toContainText("资金已冻结");

  await advanceToTick(page, 3);
  await order.getByRole("button", { name: "撤单" }).click();
  await advanceToTick(page, 4);

  await expect(page.locator(".notice")).toContainText("集合竞价委托当前不可撤销");
  await expect(order).toContainText("资金已冻结");

  await advanceToTick(page, 9);
  await expect(order).toHaveAttribute("data-order-venue", "continuous");
  await order.getByRole("button", { name: "撤单" }).click();
  await advanceToTick(page, 10);
  await expect(order).toHaveCount(0);
});

test("连续竞价展示活动委托冻结，并明确拒绝资金不足的买单", async ({ page }) => {
  await openGame(page);
  await advanceToTick(page, 9);
  const cashBefore = await availableCash(page).innerText();

  await fillLimitBuy(page, CONTINUOUS_PRICE);
  await advanceToTick(page, 10);

  const order = playerOrder(page);
  await expect(order).toContainText("资金已冻结");
  await expect(availableCash(page)).not.toHaveText(cashBefore);

  await fillLimitBuy(page, "11.20", "1000000");
  await advanceToTick(page, 11);

  await expect(page.locator(".notice")).toContainText("资金不足");
  await expect(order).toContainText("资金已冻结");
});

test("本地存档经刷新读档后保留资金、持仓与活动委托，并可继续推进", async ({ page }) => {
  await openGame(page);
  await advanceToTick(page, 9);
  await fillLimitBuy(page, CONTINUOUS_PRICE);
  await advanceToTick(page, 10);
  await expect(playerOrder(page)).toContainText("资金已冻结");

  const savedTick = Number(await page.locator(".app-root").getAttribute("data-game-tick"));
  const savedCash = await availableCash(page).innerText();
  const positions = page.locator("#section-positions tbody");
  const orders = page.locator(".player-order-list");
  const savedPositions = normalizeVisibleText(await positions.innerText());
  const savedOrders = normalizeVisibleText(await orders.innerText());
  await page.getByTitle("快存到 LocalStorage").click();
  await expect(page.locator(".notice")).toContainText("已存档");

  await page.reload();
  await expectEngineReady(page);
  await page.getByTitle("从 LocalStorage 快读").click();
  await expect(page.locator(".notice")).toContainText("已读档");
  await expect(availableCash(page)).toHaveText(savedCash);
  await expect.poll(async () => normalizeVisibleText(await positions.innerText())).toBe(savedPositions);
  await expect.poll(async () => normalizeVisibleText(await orders.innerText())).toBe(savedOrders);
  await expect(page.locator(".app-root")).toHaveAttribute("data-game-tick", String(savedTick));

  await advanceToTick(page, savedTick + 1);
  await expect(page.locator(".app-root")).toHaveAttribute("data-game-tick", String(savedTick + 1));
});
