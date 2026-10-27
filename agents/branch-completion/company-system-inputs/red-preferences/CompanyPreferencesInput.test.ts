import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import type { ReactElement } from "react";
import { createServer, type ViteDevServer } from "vite";
import { memoryHook } from "../../app/hook-test-runtime.ts";

let vite: ViteDevServer;
let module: typeof import("./CompanyPreferencesInput.tsx");
before(async () => {
  vite = await createServer({ root: new URL("../../../", import.meta.url).pathname, configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  module = await vite.ssrLoadModule("/src/components/company/CompanyPreferencesInput.tsx") as typeof module;
});
after(async () => { if (vite) await vite.close(); });

type Element = ReactElement<Record<string, unknown>>;
function nodes(value: unknown): Element[] {
  if (Array.isArray(value)) return value.flatMap(nodes);
  if (typeof value !== "object" || value === null || !("props" in value)) return [];
  const element = value as Element;
  return [element, ...nodes(element.props.children)];
}
const preference = { cash_dividend: { target_payout_bp: 3000, min_distributable_profit: "100000000", cycles_between_proposals: 1 }, stock_distribution: null };

test("Money 门槛精确保留 i64 上界并显式拒绝越界", () => {
  assert.equal(module.cashPreferenceFromFields("3000", "92233720368547758.07", "1").ok, true);
  const result = module.cashPreferenceFromFields("3000", "92233720368547758.08", "1");
  assert.equal(result.ok, false);
  assert.equal(module.stockPreferenceFromFields("92233720368547758.08", "1", "1", "1").ok, false);
});

test("真实事件保留非法 raw 文本、显示错误、不回写，再修正时以数值 company 回调", () => {
  const applied: unknown[] = [];
  const outer = module.CompanyPreferencesInput({ companies: [{ company: 17, preferences: preference } as never], onApply: (...args) => applied.push(args) });
  const controls = nodes(outer).find(element => typeof element.type === "function" && element.type.name === "CashPreferenceControls");
  assert.ok(controls);
  const render = memoryHook(controls.type as (props: Record<string, unknown>) => unknown, controls.props);
  const input = () => nodes(render.render()).find(element => element.props["aria-label"] === "目标派息比例（基点）")!;
  (input().props.onChange as Function)({ currentTarget: { value: "bad" } });
  assert.equal(input().props.value, "bad");
  assert.equal(applied.length, 0);
  assert.ok(nodes(render.render()).some(element => element.props.role === "alert"));
  (input().props.onChange as Function)({ currentTarget: { value: "4500" } });
  assert.deepEqual(applied, [[17, { ...preference, cash_dividend: { ...preference.cash_dividend, target_payout_bp: 4500 } }]]);
  assert.equal(nodes(render.render()).filter(element => element.props.role === "alert").length, 0);
});
