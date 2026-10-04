import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { buildPlayerOrderIntent, orderPriceInputState, playerOrderDescription } from "./symbolic-limit-order.ts";

test("player limit selection maps to fixed, highest, and lowest limit prices", () => {
  assert.deepEqual(buildPlayerOrderIntent("600101", "Buy", 100, "limit", "fixed", "10.01"), {
    PlaceLimit: { code: "600101", side: "Buy", price: { Fixed: "1001" }, qty: 100 },
  });
  assert.deepEqual(buildPlayerOrderIntent("600101", "Buy", 100, "limit", "highest", ""), {
    PlaceLimit: { code: "600101", side: "Buy", price: "Highest", qty: 100 },
  });
  assert.deepEqual(buildPlayerOrderIntent("600101", "Sell", 100, "limit", "highest", ""), {
    PlaceLimit: { code: "600101", side: "Sell", price: "Highest", qty: 100 },
  });
  assert.deepEqual(buildPlayerOrderIntent("600101", "Buy", 100, "limit", "lowest", ""), {
    PlaceLimit: { code: "600101", side: "Buy", price: "Lowest", qty: 100 },
  });
  assert.deepEqual(buildPlayerOrderIntent("600101", "Sell", 100, "limit", "lowest", ""), {
    PlaceLimit: { code: "600101", side: "Sell", price: "Lowest", qty: 100 },
  });
});

test("price selection labels retain limit intent and only market disables price input", () => {
  assert.equal(playerOrderDescription("limit", "fixed", "10.01"), "限价 @ 10.01 元");
  assert.equal(playerOrderDescription("limit", "highest", ""), "最高限价");
  assert.equal(playerOrderDescription("limit", "lowest", ""), "最低限价");
  assert.equal(playerOrderDescription("market", "fixed", ""), "市价");
  assert.deepEqual(orderPriceInputState("limit", "highest"), { disabled: true, placeholder: "最高限价按受理时规则确定" });
  assert.deepEqual(orderPriceInputState("limit", "lowest"), { disabled: true, placeholder: "最低限价按受理时规则确定" });
  assert.deepEqual(orderPriceInputState("market", "fixed"), { disabled: true, placeholder: "市价委托无需价格" });
  assert.deepEqual(orderPriceInputState("limit", "fixed"), { disabled: false, placeholder: "委托价" });
});

test("App wires symbolic limit choices without reclassifying them as market orders", () => {
  const appSource = readFileSync(new URL("../App.tsx", import.meta.url), "utf8");
  assert.match(appSource, /最高限价/);
  assert.match(appSource, /最低限价/);
  const commandsSource = readFileSync(new URL("../app/useTradingCommands.ts", import.meta.url), "utf8");
  assert.match(appSource, /useTradingCommands/);
  assert.match(commandsSource, /buildPlayerOrderIntent/);
  assert.match(appSource, /orderPriceInputState/);
});
