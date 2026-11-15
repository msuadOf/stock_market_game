import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
import type { MarketHistoryPage, MarketHistoryRequest } from "../host/market-history.ts";

let vite: ViteDevServer;
let Panel: typeof import("./RetainedHistoryPanel.tsx").RetainedHistoryPanel;
before(async () => {
  vite = await createServer({ configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  Panel = (await vite.ssrLoadModule("/src/components/RetainedHistoryPanel.tsx")).RetainedHistoryPanel;
});
after(async () => { await vite.close(); });

test("日期分页只提供归档日K，不提供五日分时或日期分钟成交", { timeout: 10000 }, () => {
  const query = async () => { throw new Error("SSR不能读取宿主"); };
  const html = renderToStaticMarkup(React.createElement(Panel, { query, scopeKey: "1:0", code: "600001", currentDate: "2030-01-10" }));
  assert.match(html, /历史日K起始日期/);
  assert.match(html, /每页31个自然日/);
  assert.doesNotMatch(html, /五日分时|长期分钟历史/);
});

type Props = { children?: React.ReactNode; "aria-label"?: string; disabled?: boolean; value?: string; entry?: MarketHistoryPage["entries"][number]; onSubmit?: (event: { preventDefault(): void }) => void; onChange?: (event: { currentTarget: { value: string } }) => void; onClick?: () => void };
function nodes(tree: React.ReactNode): React.ReactElement<Props>[] {
  if (Array.isArray(tree)) return tree.flatMap(nodes);
  if (!React.isValidElement<Props>(tree)) return [];
  return [tree, ...nodes(tree.props.children)];
}
function text(tree: React.ReactNode): string {
  if (Array.isArray(tree)) return tree.map(text).join("");
  if (React.isValidElement<Props>(tree)) return text(tree.props.children);
  return typeof tree === "string" || typeof tree === "number" ? String(tree) : "";
}
function mount() {
  const slots: { value: unknown; deps?: readonly unknown[] }[] = [];
  const effects: (() => void)[] = [];
  const cleanups = new Map<number, () => void>();
  const requests: { request: MarketHistoryRequest; resolve: (value: MarketHistoryPage) => void; reject: (error: Error) => void }[] = [];
  const query = (request: MarketHistoryRequest): Promise<MarketHistoryPage> => new Promise((resolve, reject) => requests.push({ request, resolve, reject }));
  let scopeKey = "1:0", code = "600001", cursor = 0;
  let tree: React.ReactNode;
  const internal = (React as unknown as { __SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED: { ReactCurrentDispatcher: { current: unknown } } }).__SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED;
  const dispatcher = {
    useState(initial: unknown) { const index = cursor++; if (slots[index] === undefined) slots[index] = { value: initial }; return [slots[index].value, (value: unknown) => { slots[index].value = value; }]; },
    useRef(initial: unknown) { const index = cursor++; if (slots[index] === undefined) slots[index] = { value: { current: initial } }; return slots[index].value; },
    useEffect(effect: () => () => void, deps: readonly unknown[]) { const index = cursor++; const previous = slots[index]?.deps; if (previous === undefined || deps.some((value, position) => !Object.is(value, previous[position]))) { slots[index] = { value: undefined, deps }; effects.push(() => { cleanups.get(index)?.(); cleanups.set(index, effect()); }); } },
  };
  function render() {
    const previous = internal.ReactCurrentDispatcher.current;
    cursor = 0; internal.ReactCurrentDispatcher.current = dispatcher;
    try { tree = Panel({ query, scopeKey, code, currentDate: "2030-01-10" }); } finally { internal.ReactCurrentDispatcher.current = previous; }
    while (effects.length) effects.shift()!();
  }
  render(); render();
  return {
    requests, render,
    submit() { nodes(tree).find((node) => node.type === "form")!.props.onSubmit!({ preventDefault() {} }); render(); },
    entries() { return nodes(tree).flatMap((node) => node.props.entry === undefined ? [] : [node.props.entry]); },
    content() { return text(tree); },
    markup() { return renderToStaticMarkup(tree); },
    click(label: string) { const button = nodes(tree).find((node) => node.type === "button" && text(node) === label)!; assert.equal(button.props.disabled, false); button.props.onClick!(); render(); },
    replace() { scopeKey = "2:9"; code = "000001"; render(); render(); },
    change() { nodes(tree).find((node) => node.props["aria-label"] === "历史日K起始日期")!.props.onChange!({ currentTarget: { value: "2030-01-02" } }); render(); },
    dispose() { cleanups.forEach((cleanup) => cleanup()); },
  };
}
function page(request: MarketHistoryRequest, date: string, next: string | null): MarketHistoryPage {
  const daily_candle = { time: Date.parse(`${date}T00:00:00Z`) / 1000, open: "1000", high: "1100", low: "950", close: "1050", volume: 150, trade_stats: { turnover_cents: "155000", trade_count: 3 } };
  return { code: request.code, entries: [{ date, availability: "Traded", bars: [], daily_candle }], next_cursor: next, settled_through: "2030-01-09" };
}
async function settle() { for (let index = 0; index < 8; index++) await Promise.resolve(); }

test("日期窗口下一页使用日期游标，前页缓存不重新读取宿主", { timeout: 10000 }, async () => {
  const ui = mount();
  try {
    ui.change(); ui.submit();
    ui.requests[0].resolve(page(ui.requests[0].request, "2030-01-02", "2030-01-02"));
    await settle(); ui.render(); ui.click("下一页");
    assert.equal(ui.requests[1].request.after, "2030-01-02");
    ui.requests[1].resolve(page(ui.requests[1].request, "2030-01-03", null));
    await settle(); ui.render(); assert.equal(ui.entries()[0].date, "2030-01-03");
    ui.click("上一页"); assert.equal(ui.entries()[0].date, "2030-01-02");
    assert.match(ui.markup(), /日K开\/高\/低\/收（元）/);
    assert.match(ui.markup(), /10.00／11.00／9.50／10.50/);
    assert.match(ui.markup(), />1550</);
    assert.doesNotMatch(ui.markup(), /真实稀疏分钟|<svg/);
    assert.equal(ui.requests.length, 2);
  } finally { ui.dispose(); }
});

test("证券/account/generation或筛选变化丢弃迟到成功和错误，当前错误显式展示", { timeout: 10000 }, async () => {
  const ui = mount();
  try {
    ui.submit(); ui.replace();
    ui.requests[0].resolve(page(ui.requests[0].request, "2030-01-03", null));
    await settle(); ui.render(); assert.deepEqual(ui.entries(), []);
    ui.submit(); ui.change(); ui.requests[1].reject(new Error("旧范围错误"));
    await settle(); ui.render(); assert.doesNotMatch(ui.content(), /旧范围错误/);
    ui.submit(); ui.requests[2].reject(new Error("当前读取失败"));
    await settle(); ui.render(); assert.match(ui.content(), /当前读取失败/);
  } finally { ui.dispose(); }
});
