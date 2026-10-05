import { parseMoney, compareMoney } from "../utils/money.ts";
import type { Cents } from "../types/engine.ts";

export type PlayerWorkingOrder = {
  readonly owner: string;
  readonly id: number;
  readonly code: string;
  readonly side: "Buy" | "Sell";
  readonly price: Cents;
  readonly remainingQty: number;
  readonly venue: "auction" | "continuous";
  readonly frozen: "cash" | "shares";
};

function record(value: unknown, path: string): Readonly<Record<string, unknown>> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError(`${path} 必须是对象`);
  }
  return value as Readonly<Record<string, unknown>>;
}

function nonnegativeSafeInteger(value: unknown, path: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) {
    throw new TypeError(`${path} 必须是非负安全整数`);
  }
  return value;
}

export function normalizePlayerWorkingOrders(value: unknown): readonly PlayerWorkingOrder[] {
  if (!Array.isArray(value)) throw new TypeError("玩家活动委托必须是数组");
  return value.map((item, index) => {
    const path = `玩家活动委托[${index}]`;
    const order = record(item, path);
    const expected = ["owner", "id", "code", "side", "price", "remainingQty", "venue", "frozen"];
    if (Object.keys(order).length !== expected.length || expected.some((key) => !Object.hasOwn(order, key))) {
      throw new TypeError(`${path} 字段不符合 DTO 契约`);
    }
    if (typeof order.code !== "string" || order.code.length === 0) throw new TypeError(`${path}.code 必须是非空证券代码`);
    if (typeof order.owner !== "string" || !/^(0|[1-9][0-9]*)$/.test(order.owner) || BigInt(order.owner) > 18446744073709551615n) {
      throw new TypeError(`${path}.owner 必须是规范 u64 AccountID 字符串`);
    }
    if (order.side !== "Buy" && order.side !== "Sell") throw new TypeError(`${path}.side 无效`);
    if (order.venue !== "auction" && order.venue !== "continuous") throw new TypeError(`${path}.venue 无效`);
    if (order.frozen !== "cash" && order.frozen !== "shares") throw new TypeError(`${path}.frozen 无效`);
    if (order.frozen !== (order.side === "Buy" ? "cash" : "shares")) throw new TypeError(`${path}.frozen 与 side 不一致`);
    const price = parseMoney(order.price, `${path}.price`);
    if (compareMoney(price, "0") <= 0) throw new TypeError(`${path}.price 必须是正数（Money 原始分值）`);
    const remainingQty = nonnegativeSafeInteger(order.remainingQty, `${path}.remainingQty`);
    if (remainingQty === 0) throw new TypeError(`${path}.remainingQty 必须是正股数`);
    if (remainingQty > 0xffff_ffff) throw new TypeError(`${path}.remainingQty 必须是 u32`);
    return {
      owner: order.owner,
      id: nonnegativeSafeInteger(order.id, `${path}.id`),
      code: order.code,
      side: order.side,
      price,
      remainingQty,
      venue: order.venue,
      frozen: order.frozen,
    };
  });
}
