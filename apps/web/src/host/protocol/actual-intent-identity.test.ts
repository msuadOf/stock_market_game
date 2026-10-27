import assert from "node:assert/strict";
import test from "node:test";
import { prepareCatalogResolver } from "../catalog-resolver.ts";
import { wireIntentOf, type CodedIntent } from "./stock-identity.ts";

const resolver = (id: number) => prepareCatalogResolver([{ id, code: "600101" }], [{ id: 8, code: "C-600101", name: "发行人", listed_stock: id }]);
const intents: readonly CodedIntent[] = [
  { PlaceLimit: { code: "600101", side: "Buy", price: { Fixed: "1234" }, qty: 100 } },
  { PlaceMarket: { code: "600101", side: "Sell", qty: 200 } },
  { Cancel: { code: "600101", id: 7 } },
];

test("三个intent只将实际公司证券code译成本代StockId，其他字段不变且A/B/A隔离", () => {
  const original = structuredClone(intents);
  for (const id of [0, 5, 0]) {
    for (const intent of intents) {
      const kind = Object.keys(intent)[0] as keyof CodedIntent;
      const payload = Object.values(intent)[0];
      assert.deepEqual(wireIntentOf(intent, resolver(id)), { [kind]: { ...payload, code: id } });
    }
  }
  assert.deepEqual(intents, original);
});

test("实际目录外证券三个variant都在发送前拒绝", () => {
  const actual = prepareCatalogResolver([{ id: 0, code: "002156" }], [{ id: 0, code: "C-002156", name: "发行人", listed_stock: 0 }]);
  for (const intent of intents) assert.throws(() => wireIntentOf(intent, actual), /600101/);
});
