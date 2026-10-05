import { expect, test, type Page } from "@playwright/test";
async function open(page: Page, width = 1020, height = 833) {
  await page.setViewportSize({ width, height }); await page.goto("/?tradingE2E=1");
  await expect(page.locator('.app-root')).toBeVisible();
  if (width > height) await page.getByRole('navigation', { name: '桌面主导航' }).getByRole('button', { name: '交易', exact: true }).click();
  else await page.getByRole('navigation', { name: '主导航' }).getByRole('button', { name: '交易', exact: true }).click();
}
async function tick(page: Page, value: number) { await page.evaluate(async value => { await (globalThis as unknown as { __STOCK_GAME_E2E__: { advanceToTick(tick: number): Promise<number> } }).__STOCK_GAME_E2E__.advanceToTick(value); }, value); }
const ticket = (page: Page, side = 'Buy') => page.locator(`.trade-ticket[data-side="${side}"]`);
test('双侧草稿隔离，手机复用；零手有字段错误，全部含费用', async ({ page }) => {
  await open(page);
  await ticket(page).getByLabel('数量（手）').fill('3');
  await expect(ticket(page, 'Sell').getByLabel('数量（手）')).toHaveValue('1');
  await ticket(page, 'Sell').getByRole('button', { name: '1/2', exact: true }).click();
  await expect(ticket(page, 'Sell').getByLabel('数量（手）')).toHaveValue('0');
  await ticket(page, 'Sell').getByRole('button', { name: '卖出', exact: true }).click();
  await expect(ticket(page, 'Sell').getByLabel('数量（手）')).toHaveAttribute('aria-invalid', 'true');
  await expect(page.locator('.player-order-item')).toHaveCount(0);
  await page.setViewportSize({ width: 320, height: 844 });
  await page.getByRole('navigation', { name: '主导航' }).getByRole('button', { name: '交易', exact: true }).click();
  await expect(ticket(page).getByLabel('数量（手）')).toHaveValue('3');
  await ticket(page).getByRole('button', { name: '全部', exact: true }).click();
  await expect(ticket(page).getByLabel('数量（手）')).toHaveValue('8926');
  expect(await page.locator('.app-root').evaluate(el => el.scrollWidth <= el.clientWidth)).toBe(true);
});
test('价格错误只关联价格字段，提交聚焦错误字段', async ({ page }) => {
  await open(page);
  await ticket(page).getByLabel('价格（元）').fill('abc');
  await ticket(page).getByRole('button', { name: '买入', exact: true }).click();
  await expect(ticket(page).getByLabel('价格（元）')).toHaveAttribute('aria-invalid', 'true');
  await expect(ticket(page).getByLabel('数量（手）')).toHaveAttribute('aria-invalid', 'false');
  await expect(ticket(page).getByLabel('价格（元）')).toBeFocused();
  await expect(ticket(page).getByLabel('价格（元）')).toHaveCSS('outline-style', 'solid');
  await expect(ticket(page).getByLabel('价格（元）')).toHaveCSS('outline-width', '2px');
});
test('符号选价解释受理时解析，市价不显示确定价格', async ({ page }) => {
  await open(page);
  await ticket(page).getByRole('button', { name: '最大', exact: true }).click();
  await expect(ticket(page).getByLabel('价格（元）')).toHaveValue('按受理时规则确定');
  await expect(ticket(page)).toContainText('预留估算边界 12.32元；非确定成交价');
  await ticket(page).getByRole('button', { name: '指定', exact: true }).click();
  await expect(ticket(page).getByLabel('价格（元）')).toHaveValue('12.32');
  await expect(ticket(page).getByLabel('价格（元）')).toBeEnabled();
  await ticket(page).getByLabel('委托类型').selectOption('market');
  await expect(ticket(page).getByLabel('价格（元）')).toHaveValue('市价委托无需价格');
});
test('价格与盘口手数分别点击，不覆盖另一侧数量', async ({ page }) => {
  await open(page); await tick(page, 9); await ticket(page).getByLabel('价格（元）').fill('10.08');
  await ticket(page).getByLabel('数量（手）').fill('2'); await ticket(page).getByRole('button', { name: '买入', exact: true }).click(); await tick(page, 10);
  await page.getByRole('navigation', { name: '桌面主导航' }).getByRole('button', { name: '个股', exact: true }).click();
  await page.getByRole('button', { name: '卖出此股票', exact: true }).click();
  await ticket(page, 'Sell').getByLabel('数量（手）').fill('7');
  await page.getByRole('button', { name: '买1价格 10.08', exact: true }).click();
  await expect(ticket(page, 'Sell').getByLabel('价格（元）')).toHaveValue('10.08');
  await expect(ticket(page, 'Sell').getByLabel('数量（手）')).toHaveValue('7');
  await page.getByRole('button', { name: /^买1手数 / }).click();
  await expect(ticket(page, 'Sell').getByLabel('数量（手）')).toHaveValue('0');
  await expect(ticket(page).getByLabel('数量（手）')).toHaveValue('2');
});
test('自动绑定原股票、暂停无重复；全证券撤单保持自动开启', async ({ page }) => {
  await open(page); await tick(page, 9);
  await ticket(page).getByRole('button', { name: '最小', exact: true }).click();
  await ticket(page).getByRole('button', { name: '自动', exact: true }).click();
  await expect(page.locator('.repeating-trades')).toContainText('600101');
  await expect(page.locator('.player-order-item')).toHaveCount(0);
  await tick(page, 10); await tick(page, 11);
  await expect(page.locator('.player-order-item').filter({ hasText: '600101' })).not.toHaveCount(0);
  await page.locator('#section-order').getByLabel('股票', { exact: true }).selectOption('002156');
  await ticket(page).getByRole('button', { name: '最小', exact: true }).click();
  await ticket(page).getByRole('button', { name: '买入', exact: true }).click(); await tick(page, 12);
  await expect(page.locator('.player-order-item').filter({ hasText: '002156' })).toHaveCount(1);
  await page.getByRole('button', { name: '取消所有', exact: true }).click();
  await expect(page.getByRole('status').filter({ hasText: '已提交全部 3 笔挂单的撤单请求；自动保持开启' })).toBeVisible();
  await expect(page.locator('.repeating-trades')).toContainText('600101');
  await tick(page, 13);
  await expect(page.locator('.player-order-item').filter({ hasText: '002156' })).toHaveCount(0);
  await expect(page.locator('.repeating-trades').getByRole('button', { name: '停止自动' })).toBeVisible();
  await page.locator('.repeating-trades').getByRole('button', { name: '停止自动' }).click();
});
