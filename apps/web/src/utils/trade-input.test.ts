import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import {
  aSharePriceLimits,
  maxAShareOrderQuantity,
  parseShareQuantity,
  parseYuanPrice,
  validateAShareQuantity,
} from "./trade-input.ts";

test("trade price parsing never silently rounds sub-cent input", () => {
  assert.equal(parseYuanPrice("10.01"), "1001");
  assert.equal(parseYuanPrice("10.1"), "1010");
  assert.equal(parseYuanPrice("90071992547409.93"), "9007199254740993");
  assert.throws(() => parseYuanPrice("10.001"), /两位小数/);
  assert.throws(() => parseYuanPrice("invalid"), /价格/);
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
  assert.deepEqual(aSharePriceLimits("1015", "MainBoard"), { down: "914", up: "1117" });
  assert.deepEqual(aSharePriceLimits("1000", "StMainBoard"), { down: "900", up: "1100" });
  assert.deepEqual(aSharePriceLimits("3680", "ChiNext"), { down: "2944", up: "4416" });
  assert.deepEqual(aSharePriceLimits("9007199254740993", "ChiNext"), { down: "7205759403792794", up: "10808639105689192" });
  assert.throws(
    () => aSharePriceLimits("9223372036854775807", "ChiNext"),
    /金额超出 i64 范围/,
  );
});

test("ChiNext quantity caps are explicit at the UI validation boundary", () => {
  assert.equal(maxAShareOrderQuantity("ChiNext"), 300_000);
  assert.equal(maxAShareOrderQuantity("ChiNext", true), 150_000);
  assert.doesNotThrow(() => validateAShareQuantity("Buy", 300_000, 0, 300_000));
  assert.throws(() => validateAShareQuantity("Buy", 300_100, 0, 300_000), /300000 股/);
  assert.doesNotThrow(() => validateAShareQuantity("Buy", 150_000, 0, maxAShareOrderQuantity("ChiNext", true)));
  assert.throws(
    () => validateAShareQuantity("Buy", 150_100, 0, maxAShareOrderQuantity("ChiNext", true)),
    /150000 股/,
  );

  const appSource = readFileSync(new URL("../app/useTradingCommands.ts", import.meta.url), "utf8");
  assert.match(appSource, /maxAShareOrderQuantity\(stock\.category, orderKind === "market"\)/);
});
