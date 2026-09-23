import assert from "node:assert/strict";
import test from "node:test";
import {
  aSharePriceLimits,
  maxAShareOrderQuantity,
  parseShareQuantity,
  parseYuanPrice,
  validateAShareQuantity,
} from "./trade-input.ts";

test("trade price parsing never silently rounds sub-cent input", () => {
  assert.equal(parseYuanPrice("10.01"), 1_001);
  assert.equal(parseYuanPrice("10.1"), 1_010);
  assert.throws(() => parseYuanPrice("10.001"), /两位小数/);
});

test("A-share quantities use board lots while permitting one complete odd-lot remainder", () => {
  assert.doesNotThrow(() => validateAShareQuantity("Buy", parseShareQuantity("100"), 0));
  assert.throws(() => validateAShareQuantity("Buy", 99, 0), /100 股/);
  assert.doesNotThrow(() => validateAShareQuantity("Sell", 50, 150));
  assert.doesNotThrow(() => validateAShareQuantity("Sell", 150, 150));
  assert.throws(() => validateAShareQuantity("Sell", 25, 150), /全部不足 100 股/);
  assert.throws(() => validateAShareQuantity("Sell", 200, 150), /可卖数量不足/);
});

test("A-share UI limits use category rules and positive half-up rounding", () => {
  assert.deepEqual(aSharePriceLimits(1_015, "MainBoard"), { down: 914, up: 1_117 });
  assert.deepEqual(aSharePriceLimits(1_000, "StMainBoard"), { down: 900, up: 1_100 });
  assert.deepEqual(aSharePriceLimits(3_680, "ChiNext"), { down: 2_944, up: 4_416 });
  assert.throws(
    () => aSharePriceLimits(Number.MAX_SAFE_INTEGER, "ChiNext"),
    /超出可安全处理范围/,
  );
});

test("ChiNext quantity caps are explicit at the UI validation boundary", () => {
  assert.equal(maxAShareOrderQuantity("ChiNext"), 300_000);
  assert.equal(maxAShareOrderQuantity("ChiNext", true), 150_000);
  assert.doesNotThrow(() => validateAShareQuantity("Buy", 300_000, 0, 300_000));
  assert.throws(() => validateAShareQuantity("Buy", 300_100, 0, 300_000), /300000 股/);
});
