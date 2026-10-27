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


test("送转非法 raw 与 Money 上限保持可见，修正前不回写，外部有效回填重置字段", () => {
  const applied: unknown[] = [];
  const preferences = { ...preference, stock_distribution: { min_distributable_profit: "100", shares_per_existing_share_micros: 1, max_cumulative_expansion_micros: 1, cycles_between_proposals: 1 } };
  const outer = module.CompanyPreferencesInput({ companies: [{ company: 9, preferences } as never], onApply: (...args) => applied.push(args) });
  const control = nodes(outer).find(element => typeof element.type === "function" && element.type.name === "StockPreferenceControls");
  assert.ok(control);
  const render = memoryHook(control.type as (props: Record<string, unknown>) => unknown, control.props);
  const find = (label: string) => nodes(render.render()).find(element => element.props["aria-label"] === label)!;
  (find("触发门槛（元）").props.onChange as Function)({ currentTarget: { value: "92233720368547758.08" } });
  assert.equal(find("触发门槛（元）").props.value, "92233720368547758.08");
  assert.equal(applied.length, 0);
  assert.ok(nodes(render.render()).some(element => element.props.role === "alert"));
  (find("触发门槛（元）").props.onChange as Function)({ currentTarget: { value: "90071992547409.93" } });
  assert.equal(applied.length, 1);
  assert.deepEqual(applied[0], [9, { ...preferences, stock_distribution: { ...preferences.stock_distribution, min_distributable_profit: "9007199254740993" } }]);
  render.render({ ...control.props, preferences: { ...preferences, stock_distribution: { ...preferences.stock_distribution, min_distributable_profit: "200" } } });
  assert.equal(find("触发门槛（元）").props.value, "2.00");
});

test("字段边界拒绝小数 bp、零间隔和过大送转比例，不钳制", () => {
  for (const fields of [["1.1", "1", "1"], ["0", "1", "1"], ["10001", "1", "1"], ["1", "1", "0"], ["1", "1", "65536"]]) assert.equal(module.cashPreferenceFromFields(...fields as [string, string, string]).ok, false);
  for (const fields of [["1", "10000001", "1", "1"], ["1", "1", "1000000001", "1"], ["0", "1", "1", "1"]]) assert.equal(module.stockPreferenceFromFields(...fields as [string, string, string, string]).ok, false);
});

test("未配置送转启用为空 raw，半填不回写；取消为未配置", () => {
  const applied: unknown[] = [];
  const outer = module.CompanyPreferencesInput({ companies: [{ company: 17, preferences: preference } as never], onApply: (...args) => applied.push(args) });
  const control = nodes(outer).find(element => typeof element.type === "function" && element.type.name === "StockPreferenceControls");
  assert.ok(control);
  const render = memoryHook(control.type as (props: Record<string, unknown>) => unknown, control.props);
  const toggle = () => nodes(render.render()).find(element => element.props.name === "stock-preference-17")!;
  (toggle().props.onChange as Function)({ currentTarget: { checked: true } });
  assert.equal(applied.length, 0);
  const input = () => nodes(render.render()).find(element => element.props["aria-label"] === "触发门槛（元）")!;
  assert.equal(input().props.value, "");
  (input().props.onChange as Function)({ currentTarget: { value: "100" } });
  assert.equal(applied.length, 0);
  assert.ok(nodes(render.render()).some(element => element.props.role === "alert"));
  (toggle().props.onChange as Function)({ currentTarget: { checked: false } });
  assert.equal(nodes(render.render()).filter(element => element.props["aria-label"] === "触发门槛（元）").length, 0);
  assert.equal(toggle().props.checked, false);
});

test("外部配置更新时保留未完成 raw 并明确提示，不静默丢编辑", () => {
  const outer = module.CompanyPreferencesInput({ companies: [{ company: 17, preferences: preference } as never], onApply() {} });
  const control = nodes(outer).find(element => typeof element.type === "function" && element.type.name === "CashPreferenceControls");
  assert.ok(control);
  const render = memoryHook(control.type as (props: Record<string, unknown>) => unknown, control.props);
  const find = () => nodes(render.render()).find(element => element.props["aria-label"] === "目标派息比例（基点）")!;
  (find().props.onChange as Function)({ currentTarget: { value: "unfinished" } });
  render.render({ ...control.props, preferences: { ...preference, cash_dividend: { ...preference.cash_dividend, target_payout_bp: 5000 } } });
  assert.equal(find().props.value, "unfinished");
  const alerts = nodes(render.render()).filter(element => element.props.role === "alert");
  assert.match(JSON.stringify(alerts), /外部配置已更新/);
});

test("外部关闭送转/现金配置保留非法raw与冲突提示，显式取消才丢弃", () => {
  for (const [name, field, label] of [["StockPreferenceControls", "stock_distribution", "触发门槛（元）"], ["CashPreferenceControls", "cash_dividend", "目标派息比例（基点）"]] as const) {
    const preferences = { ...preference, stock_distribution: { min_distributable_profit: "100", shares_per_existing_share_micros: 1, max_cumulative_expansion_micros: 1, cycles_between_proposals: 1 } };
    const outer = module.CompanyPreferencesInput({ companies: [{ company: 17, preferences } as never], onApply() { assert.fail("非法raw和取消本地冲突不应回写"); } });
    const control = nodes(outer).find(element => typeof element.type === "function" && element.type.name === name)!;
    const render = memoryHook(control.type as (props: Record<string, unknown>) => unknown, control.props);
    const input = () => nodes(render.render()).find(element => element.props["aria-label"] === label);
    (input()!.props.onChange as Function)({ currentTarget: { value: "unfinished" } });
    render.render({ ...control.props, preferences: { ...preferences, [field]: null } });
    assert.equal(input()?.props.value, "unfinished");
    assert.match(JSON.stringify(nodes(render.render()).filter(element => element.props.role === "alert")), /外部配置已更新/);
    const toggle = nodes(render.render()).find(element => element.props.type === "checkbox")!;
    (toggle.props.onChange as Function)({ currentTarget: { checked: false } });
    assert.equal(input(), undefined);
    assert.equal(nodes(render.render()).filter(element => element.props.role === "alert").length, 0);
  }
});

test("取消现金/送转的回调失败显式报错，保持原启用状态和raw", () => {
  for (const name of ["StockPreferenceControls", "CashPreferenceControls"]) {
    const preferences = { ...preference, stock_distribution: { min_distributable_profit: "100", shares_per_existing_share_micros: 1, max_cumulative_expansion_micros: 1, cycles_between_proposals: 1 } };
    const outer = module.CompanyPreferencesInput({ companies: [{ company: 17, preferences } as never], onApply() { throw new Error("保存失败：保留原配置"); } });
    const control = nodes(outer).find(element => typeof element.type === "function" && element.type.name === name)!;
    const render = memoryHook(control.type as (props: Record<string, unknown>) => unknown, control.props);
    const toggle = () => nodes(render.render()).find(element => element.props.type === "checkbox")!;
    assert.doesNotThrow(() => (toggle().props.onChange as Function)({ currentTarget: { checked: false } }));
    assert.equal(toggle().props.checked, true);
    assert.match(JSON.stringify(nodes(render.render()).filter(element => element.props.role === "alert")), /保存失败/);
  }
});
