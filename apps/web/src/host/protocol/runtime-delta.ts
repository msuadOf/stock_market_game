import type { AccountSnap } from "../../types/generated/AccountSnap.ts";
import type { RuntimeDelta } from "../../types/generated/RuntimeDelta.ts";
import type { Snapshot } from "../../types/generated/Snapshot.ts";
import type { PlayerWorkingOrder } from "../player-working-orders.ts";
import { canonicalJson } from "./canonical.ts";
import { boolean, enumValue, exact, field, mapEntries, record, safeInteger, safeU32, text, values } from "./guards.ts";
import { ProtocolError, type ProtocolState } from "./types.ts";
import { parseMoney, TRADING_PHASES } from "./wire-values.ts";

function malformed(where: string, message: string): never {
  throw new ProtocolError("PROTOCOL_MALFORMED", where, message);
}

function workingOrder(value: unknown, path: string): PlayerWorkingOrder {
  const source = record(value, path);
  exact(source, ["id", "code", "side", "price", "remainingQty", "venue", "frozen"], path);
  const order = {
    id: safeInteger(field(source, "id", path), `${path}.id`),
    code: text(field(source, "code", path), `${path}.code`),
    side: enumValue(field(source, "side", path), ["Buy", "Sell"], `${path}.side`),
    price: parseMoney(field(source, "price", path), `${path}.price`),
    remainingQty: safeU32(field(source, "remainingQty", path), `${path}.remainingQty`),
    venue: enumValue(field(source, "venue", path), ["auction", "continuous"], `${path}.venue`),
    frozen: enumValue(field(source, "frozen", path), ["cash", "shares"], `${path}.frozen`),
  };
  if (order.code.length === 0 || order.price <= 0 || order.remainingQty === 0
    || order.frozen !== (order.side === "Buy" ? "cash" : "shares")) {
    malformed(path, "活动委托代码、价格、余量或冻结资源不合法");
  }
  return order;
}

export function parseBaselineWorkingOrders(value: unknown, markets: Snapshot["markets"]): Record<number, PlayerWorkingOrder> {
  const path = "protocol.baseline.working_orders";
  const orders: Record<number, PlayerWorkingOrder> = {};
  for (const [index, entry] of values(value, path).entries()) {
    const order = workingOrder(entry, `${path}[${index}]`);
    if (Object.hasOwn(orders, order.id) || !Object.hasOwn(markets, order.code)) {
      malformed(path, "基线委托 ID 必须唯一，且证券必须属于当前市场");
    }
    orders[order.id] = order;
  }
  return orders;
}

function validateAccount(account: AccountSnap, path: string): void {
  if (account.cash < 0 || account.reserved_cash < 0 || account.reserved_cash > account.cash) {
    malformed(path, "账户现金或资金占用不合法");
  }
  for (const [code, position] of Object.entries(account.positions)) {
    if (position.t1_locked > position.qty || position.invested_cents < 0 || position.recovered_cents < 0) {
      malformed(`${path}.positions.${code}`, "持仓数量、T+1 锁定或成本不合法");
    }
  }
  for (const [code, reserved] of Object.entries(account.reserved_sell_qty)) {
    const position = account.positions[code];
    if (position === undefined || reserved > position.qty - position.t1_locked) {
      malformed(`${path}.reserved_sell_qty.${code}`, "卖出占用超出可卖持仓");
    }
  }
}

export function parseRuntimeDelta(
  value: unknown,
  path: string,
  parseAccount: (value: unknown, path: string) => AccountSnap,
): RuntimeDelta {
  const source = record(value, path);
  exact(source, ["seq_from", "seq_to", "tick", "day", "phase", "accounts", "working_orders"], path);
  const accounts = mapEntries(field(source, "accounts", path), `${path}.accounts`, parseAccount);
  for (const [id, account] of Object.entries(accounts)) {
    if (id !== "0") malformed(`${path}.accounts.${id}`, "增量只允许公开玩家账户，不允许 NPC 私有账户");
    validateAccount(account, `${path}.accounts.${id}`);
  }
  const orderPath = `${path}.working_orders`;
  const orders = record(field(source, "working_orders", path), orderPath);
  exact(orders, ["reset", "upserts", "removed"], orderPath);
  const reset = boolean(field(orders, "reset", orderPath), `${orderPath}.reset`);
  const upserts = values(field(orders, "upserts", orderPath), `${orderPath}.upserts`).map((order, index) => workingOrder(order, `${orderPath}.upserts[${index}]`));
  const removed = values(field(orders, "removed", orderPath), `${orderPath}.removed`).map((id, index) => safeInteger(id, `${orderPath}.removed[${index}]`));
  const ids = new Set<number>();
  for (const id of [...upserts.map((order) => order.id), ...removed]) {
    if (ids.has(id)) malformed(orderPath, "委托增量 ID 不得重复或同时新增与删除");
    ids.add(id);
  }
  if (reset && removed.length !== 0) malformed(orderPath, "委托 reset 不得同时包含删除项");
  return {
    seq_from: safeInteger(field(source, "seq_from", path), `${path}.seq_from`),
    seq_to: safeInteger(field(source, "seq_to", path), `${path}.seq_to`),
    tick: safeInteger(field(source, "tick", path), `${path}.tick`),
    day: safeU32(field(source, "day", path), `${path}.day`),
    phase: enumValue(field(source, "phase", path), TRADING_PHASES, `${path}.phase`),
    accounts,
    working_orders: { reset, upserts, removed },
  };
}

export function applyRuntimeDelta(
  state: Pick<ProtocolState, "snapshot" | "playerWorkingOrders" | "playerOrdersReady">,
  delta: RuntimeDelta,
  markets: Snapshot["markets"],
  activeDailyCandles: Snapshot["active_daily_candles"],
): Pick<ProtocolState, "snapshot" | "playerWorkingOrders" | "playerOrdersReady"> {
  const orderPath = "protocol.reduce.runtime_delta.working_orders";
  if (!state.playerOrdersReady && !delta.working_orders.reset) malformed(orderPath, "基线后的首批委托增量必须显式 reset");
  const playerWorkingOrders: Record<number, PlayerWorkingOrder> = delta.working_orders.reset ? {} : { ...state.playerWorkingOrders };
  for (const id of delta.working_orders.removed) {
    if (!Object.hasOwn(playerWorkingOrders, id)) malformed(orderPath, `不能删除未知活动委托 ${id}`);
    delete playerWorkingOrders[id];
  }
  for (const order of delta.working_orders.upserts) {
    if (!Object.hasOwn(markets, order.code)) malformed(orderPath, `委托证券 ${order.code} 不在当前市场`);
    playerWorkingOrders[order.id] = workingOrder(order, orderPath);
  }
  let accounts = state.snapshot.accounts;
  for (const [id, account] of Object.entries(delta.accounts)) {
    if (!Object.hasOwn(accounts, id)) malformed("protocol.reduce.runtime_delta.accounts", `账户 ${id} 不在当前基线`);
    if (Object.keys(account.positions).some((code) => !Object.hasOwn(markets, code))) {
      malformed("protocol.reduce.runtime_delta.accounts", "持仓证券不在当前市场");
    }
    if (canonicalJson(accounts[Number(id)]) !== canonicalJson(account)) {
      if (accounts === state.snapshot.accounts) accounts = { ...accounts };
      accounts[Number(id)] = account;
    }
  }
  return {
    snapshot: {
      ...state.snapshot, tick: delta.tick, seq: delta.seq_to, day: delta.day, phase: delta.phase, accounts,
      markets: retainUnchangedEntries(state.snapshot.markets, markets),
      active_daily_candles: retainUnchangedEntries(state.snapshot.active_daily_candles, activeDailyCandles),
    },
    playerWorkingOrders: delta.working_orders.reset || delta.working_orders.upserts.length > 0 || delta.working_orders.removed.length > 0
      ? playerWorkingOrders : state.playerWorkingOrders,
    playerOrdersReady: true,
  };
}

export function retainUnchangedEntries<Values>(before: Record<string, Values>, next: Record<string, Values>): Record<string, Values> {
  const retained: Record<string, Values> = {};
  let changed = Object.keys(before).length !== Object.keys(next).length;
  for (const [key, value] of Object.entries(next)) {
    if (Object.hasOwn(before, key) && canonicalJson(before[key]) === canonicalJson(value)) {
      retained[key] = before[key]!;
    } else {
      retained[key] = value;
      changed = true;
    }
  }
  return changed ? retained : before;
}
