import assert from "node:assert/strict";
import test from "node:test";
import { normalizePlayerWorkingOrders } from "./player-working-orders.ts";

test("preserves raw Money minor units in the read-only player order DTO", () => {
  // wire 的 code 为数值 StockId；id 0 ↔ 代码串 "000812"（ADR 0041）。
  assert.deepEqual(normalizePlayerWorkingOrders([{
    id: 7,
    code: 0,
    side: "Buy",
    price: "1000",
    remainingQty: 200,
    venue: "Auction",
    frozen: "Cash",
  }]), [{
    id: 7,
    code: "000812",
    side: "Buy",
    price: "1000",
    remainingQty: 200,
    venue: "Auction",
    frozen: "Cash",
  }]);
});

test("rejects malformed order DTOs and unsafe u64 projections", () => {
  assert.throws(() => normalizePlayerWorkingOrders({}), /数组/);
  assert.throws(() => normalizePlayerWorkingOrders([{
    id: Number.MAX_SAFE_INTEGER + 1,
    code: 0,
    side: "Buy",
    price: "1234",
    remainingQty: 200,
    venue: "Auction",
    frozen: "Cash",
  }]), /安全整数/);
  assert.throws(() => normalizePlayerWorkingOrders([{
    id: 7,
    code: 0,
    side: "Buy",
    price: "1234",
    remainingQty: 200,
    venue: "Continuous",
    frozen: "Shares",
    extra: true,
  }]), /字段/);
});

test("requires positive price and remaining quantity with side-matched frozen assets", () => {
  const order = { id: 7, code: 0, side: "Buy", price: "1234", remainingQty: 200, venue: "Continuous", frozen: "Cash" };
  assert.throws(() => normalizePlayerWorkingOrders([{ ...order, price: "0" }]), /Money 原始分值/);
  assert.throws(() => normalizePlayerWorkingOrders([{ ...order, price: 1234 }]), /金额必须是规范十进制整数分字符串/);
  assert.throws(() => normalizePlayerWorkingOrders([{ ...order, remainingQty: 0 }]), /正股数/);
  assert.throws(() => normalizePlayerWorkingOrders([{ ...order, frozen: "Shares" }]), /与 side 不一致/);
});
