import assert from "node:assert/strict";
import test from "node:test";
import { normalizePlayerWorkingOrders } from "./player-working-orders.ts";

test("活动委托 owner 使用规范 u64 AccountID 字符串", { timeout: 10000 }, () => {
  const order = { owner: "18446744073709551615", id: 7, code: "600000", side: "Buy", price: "1234", remainingQty: 200, venue: "continuous", frozen: "cash" };
  assert.equal(normalizePlayerWorkingOrders([order])[0].owner, order.owner);
  for (const owner of [0, "", "01", "-1", "18446744073709551616"]) {
    assert.throws(() => normalizePlayerWorkingOrders([{ ...order, owner }]), /owner/);
  }
});

test("preserves raw Money minor units in the read-only player order DTO", () => {
  assert.deepEqual(normalizePlayerWorkingOrders([{
    owner: "0", id: 7,
    code: "600000",
    side: "Buy",
    price: "1000",
    remainingQty: 200,
    venue: "auction",
    frozen: "cash",
  }]), [{
    owner: "0", id: 7,
    code: "600000",
    side: "Buy",
    price: "1000",
    remainingQty: 200,
    venue: "auction",
    frozen: "cash",
  }]);
});

test("rejects malformed order DTOs and unsafe u64 projections", () => {
  assert.throws(() => normalizePlayerWorkingOrders({}), /数组/);
  assert.throws(() => normalizePlayerWorkingOrders([{
    owner: "0", id: Number.MAX_SAFE_INTEGER + 1,
    code: "600000",
    side: "Buy",
    price: "1234",
    remainingQty: 200,
    venue: "auction",
    frozen: "cash",
  }]), /安全整数/);
  assert.throws(() => normalizePlayerWorkingOrders([{
    owner: "0", id: 7,
    code: "600000",
    side: "Buy",
    price: "1234",
    remainingQty: 200,
    venue: "continuous",
    frozen: "shares",
    extra: true,
  }]), /字段/);
});

test("requires positive price and remaining quantity with side-matched frozen assets", () => {
  const order = { owner: "0", id: 7, code: "600000", side: "Buy", price: "1234", remainingQty: 200, venue: "continuous", frozen: "cash" };
  assert.throws(() => normalizePlayerWorkingOrders([{ ...order, price: "0" }]), /Money 原始分值/);
  assert.throws(() => normalizePlayerWorkingOrders([{ ...order, price: 1234 }]), /金额必须是规范十进制整数分字符串/);
  assert.throws(() => normalizePlayerWorkingOrders([{ ...order, remainingQty: 0 }]), /正股数/);
  assert.throws(() => normalizePlayerWorkingOrders([{ ...order, frozen: "shares" }]), /与 side 不一致/);
});
