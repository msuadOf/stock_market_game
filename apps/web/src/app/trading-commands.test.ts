import assert from "node:assert/strict";
import test from "node:test";
import { useTradingCommands } from "./useTradingCommands.ts";
import { commandHostFixture } from "./command-host-test-fixture.ts";
import { memoryHook } from "./hook-test-runtime.ts";
import { DEFAULT_SETUP } from "../config/defaults.ts";
import { PlayerOrderRefreshGate, type PlayerWorkingOrder } from "../components/player-orders.ts";
import { AutoOrderManager } from "../components/auto-order-manager.ts";
import type { AccountSnap, Intent } from "../types/engine.ts";
import type { EngineHost } from "../host/engine-host.ts";
import { store, clearAutoOrders } from "../store/store.ts";

const order: PlayerWorkingOrder = { id: 1, code: "600101", side: "Buy", price: 1000, remainingQty: 100, venue: "continuous", frozen: "cash" };
function fixture() {
  const notices: string[] = [], intents: Intent[] = [];
  const host = commandHostFixture({ submitIntent: async (intent: Intent) => { intents.push(intent); }, playerWorkingOrders: async () => [order] });
  const options = { hostRef: { current: host as EngineHost | null }, playerOrderRefreshGateRef: { current: new PlayerOrderRefreshGate() },
    autoOrderMgrRef: { current: new AutoOrderManager(async () => {}) }, activeSetup: DEFAULT_SETUP,
    playerAccount: { cash: 1_000_000, reserved_cash: 0, reserved_sell_qty: { "600101": 100 }, positions: { "600101": { qty: 250, t1_locked: 100, invested_cents: 250_000, recovered_cents: 0 } } } satisfies AccountSnap,
    protocolPlayerOrders: {} as Record<number, PlayerWorkingOrder>, playerOrdersReady: false, setNotice: (value: string) => { notices.push(value); } };
  return { options, host, notices, intents, hook: memoryHook(useTradingCommands, options) };
}
test("TradingCommands 共享私有表单，限价符号与 T+1/冻结股份预检保持原语义", async () => {
  const f = fixture(); let commands = f.hook.render();
  commands.setPriceChoice("highest"); commands.setQtyText("100"); commands = f.hook.render();
  await commands.submit("Buy"); assert.deepEqual(f.intents[0], { PlaceLimit: { code: "600101", side: "Buy", price: "Highest", qty: 100 } });
  commands.setQtyText("50"); commands = f.hook.render(); assert.ok(commands.buildIntent("Sell"));
  commands.setQtyText("100"); commands = f.hook.render(); assert.equal(commands.buildIntent("Sell"), null); assert.match(f.notices.at(-1)!, /可卖数量不足/);
});
test("TradingCommands 使用活动 setup 的创业板市价单上限；缺失规则不提交", async () => {
  const f = fixture(); f.options.activeSetup = { ...DEFAULT_SETUP, stocks: DEFAULT_SETUP.stocks.map((stock) => ({ ...stock, category: "ChiNext" as const })) };
  let commands = f.hook.render(); commands.setOrderKind("market"); commands.setQtyText("150100"); commands = f.hook.render();
  await commands.submit("Buy"); assert.equal(f.intents.length, 0); assert.match(f.notices.at(-1)!, /150000/);
  f.options.activeSetup = { ...DEFAULT_SETUP, stocks: [] }; commands = f.hook.render(); commands.setQtyText("100"); commands = f.hook.render();
  assert.equal(commands.buildIntent("Buy"), null); assert.match(f.notices.at(-1)!, /缺少股票.*规则配置/);
});
test("取消中集合由同一 hook 持有，入队失败移除，协议 callback 可稳定清空", async () => {
  const f = fixture(); let commands = f.hook.render(); const clear = commands.clearCancelingOrderIds;
  await commands.cancelPlayerOrder(order); commands = f.hook.render(); assert.equal(commands.cancelingOrderIds.has(1), true); assert.equal(commands.clearCancelingOrderIds, clear);
  clear(); assert.equal(f.hook.render().cancelingOrderIds.size, 0);
  f.host.submitIntent = async () => { throw new Error("入队失败"); }; await f.hook.render().cancelPlayerOrder(order);
  assert.equal(f.hook.render().cancelingOrderIds.size, 0); assert.match(f.notices.at(-1)!, /撤单请求未入队.*入队失败/);
});
test("过期查询结果不能回写，新查询与 protocol 列表使用同一 hook 接口", async () => {
  const f = fixture(); let resolve!: (orders: readonly PlayerWorkingOrder[]) => void;
  const stableRefresh = f.hook.render().refreshPlayerOrders;
  f.hook.render().setQtyText("200");
  assert.equal(f.hook.render().refreshPlayerOrders, stableRefresh);
  f.options.hostRef.current = null;
  f.options.playerOrderRefreshGateRef.current.invalidate();
  assert.equal(f.hook.render().refreshPlayerOrders, stableRefresh);
  f.options.hostRef.current = f.host;
  f.host.playerWorkingOrders = () => new Promise((done) => { resolve = done; });
  const refresh = f.hook.render().refreshPlayerOrders(); f.options.playerOrderRefreshGateRef.current.invalidate(); resolve([order]); await refresh;
  assert.deepEqual(f.hook.render().playerOrders, []);
  f.host.playerWorkingOrders = async () => [order]; await f.hook.render().refreshPlayerOrders(); let commands = f.hook.render(); assert.deepEqual(commands.playerOrders, [order]);
  const clear = commands.clearPlayerOrders; clear(); assert.deepEqual(f.hook.render().playerOrders, []);
  f.options.playerOrdersReady = true; f.options.protocolPlayerOrders = { 2: { ...order, id: 2 }, 1: order }; commands = f.hook.render();
  assert.deepEqual(commands.playerOrders.map((item) => item.id), [1, 2]); assert.equal(commands.clearPlayerOrders, clear);
});
test("条件单先按活动规则预检，再复用现有 manager/Redux 登记", () => {
  const f = fixture(); store.dispatch(clearAutoOrders()); let commands = f.hook.render();
  commands.setAutoType("buyTrigger"); commands.setAutoTrigger("10.00"); commands = f.hook.render(); commands.addAuto();
  const item = store.getState().autoOrders.items.at(-1)!;
  assert.equal(item.triggerPrice, 1000); assert.equal(item.qty, 100); assert.equal(item.side, "Buy");
  assert.equal(f.options.autoOrderMgrRef.current.list().at(-1)?.id, item.id); store.dispatch(clearAutoOrders());
});

test("G22：输入即刻产生字段错误，修正清除；买卖方向预检关联数量", { timeout: 10000 }, () => {
  const f = fixture(); let commands = f.hook.render();
  assert.deepEqual(commands.fieldErrors, {});
  commands.setPriceText("1.234"); commands.setQtyText("非数字"); commands = f.hook.render();
  assert.match(commands.fieldErrors.price!, /两位小数/);
  assert.match(commands.fieldErrors.quantity!, /正整数股/);
  assert.equal(commands.buildIntent("Buy"), null);
  commands.setPriceText("10.00"); commands.setQtyText("50"); commands = f.hook.render();
  assert.equal(commands.fieldErrors.price, undefined); assert.equal(commands.fieldErrors.quantity, undefined);
  assert.equal(commands.buildIntent("Buy"), null);
  assert.match(f.hook.render().fieldErrors.quantity!, /100 股的整数倍/);
  assert.ok(commands.buildIntent("Sell"));
  assert.equal(f.hook.render().fieldErrors.quantity, undefined);
  commands.setAutoTrigger("0"); commands.setAutoQty("-1"); commands = f.hook.render();
  assert.match(commands.autoFieldErrors.price!, /价格/); assert.match(commands.autoFieldErrors.quantity!, /正整数股/);
});

test("G68：配置替换后的委托证券只能从当前 activeSetup 选择", { timeout: 10000 }, () => {
  const f = fixture(); f.hook.render();
  const stock = { ...DEFAULT_SETUP.stocks[0]!, code: "300999", name: "当前证券", category: "ChiNext" as const };
  f.options.activeSetup = { ...DEFAULT_SETUP, stocks: [stock] };
  let commands = f.hook.render(); assert.equal(commands.form.tradeCode, "300999");
  commands.setPriceChoice("highest"); commands = f.hook.render();
  assert.deepEqual(commands.buildIntent("Buy"), { PlaceLimit: { code: "300999", side: "Buy", price: "Highest", qty: 100 } });
});

test("G22：提交前未触碰的价格错误随价格字段禁用即时撤下", { timeout: 10000 }, () => {
  const f = fixture(); let commands = f.hook.render();
  assert.equal(commands.buildIntent("Buy"), null);
  assert.match(f.hook.render().fieldErrors.price!, /价格/);
  commands.setOrderKind("market"); commands = f.hook.render();
  assert.equal(commands.fieldErrors.price, undefined);
  commands.setOrderKind("limit"); commands.setPriceChoice("highest"); commands = f.hook.render();
  assert.equal(commands.fieldErrors.price, undefined);
});
