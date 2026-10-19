import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import React, { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
import type { PersonalTradeHistoryPanelProps } from "./PersonalTradeHistoryPanel.tsx";
import type { PersonalTradeHistoryRequest, PersonalTradeHistoryPage } from "../host/engine-host.ts";

let vite: ViteDevServer;
let Panel: typeof import("./PersonalTradeHistoryPanel.tsx").PersonalTradeHistoryPanel;
before(async () => {
  vite = await createServer({ configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  Panel = (await vite.ssrLoadModule("/src/components/PersonalTradeHistoryPanel.tsx")).PersonalTradeHistoryPanel;
});
after(async () => { await vite?.close(); });

test("日期复盘面板有明确日期/证券/方向filters及前后页，不伪造盈亏归因", { timeout: 10000 }, () => {
  const props: PersonalTradeHistoryPanelProps = { scopeKey: "1:0", initialDate: "2030-01-04", codes: ["600001"], query: async () => { throw new Error("SSR不能发查询"); } };
  const html = renderToStaticMarkup(createElement(Panel, props));
  assert.match(html, /type="date"/);
  assert.match(html, /起始自然日/);
  assert.match(html, /结束自然日/);
  assert.match(html, /全部证券/);
  assert.match(html, /全部方向/);
  assert.match(html, /上一页/);
  assert.match(html, /下一页/);
  assert.match(html, /不会.*行情读取经历/);
  assert.doesNotMatch(html, /预计收益|收益归因|已实现盈亏/);
});

type Props = { children?: React.ReactNode; "aria-label"?: string; value?: string; disabled?: boolean; rows?: readonly { receipt_id: string }[]; onChange?: (event: { currentTarget: { value: string } }) => void; onClick?: () => void; onSubmit?: (event: { preventDefault(): void }) => void };
function elements(node: React.ReactNode): React.ReactElement<Props>[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!React.isValidElement<Props>(node)) return [];
  return [node, ...elements(node.props.children)];
}
function text(node: React.ReactNode): string {
  if (Array.isArray(node)) return node.map(text).join("");
  if (React.isValidElement<Props>(node)) return text(node.props.children);
  return typeof node === "string" || typeof node === "number" ? String(node) : "";
}

function mounted() {
  const slots: { value: unknown; dependencies?: readonly unknown[] }[] = [];
  const pendingEffects: (() => void)[] = [];
  const cleanups = new Map<number, () => void>();
  const queries: { request: PersonalTradeHistoryRequest; resolve: (page: PersonalTradeHistoryPage) => void; reject: (failure: Error) => void }[] = [];
  const query = (request: PersonalTradeHistoryRequest): Promise<PersonalTradeHistoryPage> => new Promise((resolve, reject) => { queries.push({ request, resolve, reject }); });
  let scopeKey = "1:0";
  let cursor = 0;
  let tree: React.ReactNode;
  const internals = (React as unknown as { __SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED: { ReactCurrentDispatcher: { current: unknown } } }).__SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED;
  const dispatcher = {
    useState(initial: unknown) { const index = cursor++; if (slots[index] === undefined) slots[index] = { value: typeof initial === "function" ? initial() : initial }; return [slots[index].value, (next: unknown) => { slots[index].value = next; }]; },
    useRef(initial: unknown) { const index = cursor++; if (slots[index] === undefined) slots[index] = { value: { current: initial } }; return slots[index].value; },
    useEffect(effect: () => (() => void), dependencies: readonly unknown[]) {
      const index = cursor++;
      const previous = slots[index]?.dependencies;
      if (previous === undefined || dependencies.some((item, position) => !Object.is(item, previous[position]))) {
        slots[index] = { value: undefined, dependencies };
        pendingEffects.push(() => { cleanups.get(index)?.(); cleanups.set(index, effect()); });
      }
    },
  };
  const render = () => {
    const previous = internals.ReactCurrentDispatcher.current;
    cursor = 0;
    internals.ReactCurrentDispatcher.current = dispatcher;
    try { tree = Panel({ query, scopeKey, initialDate: "2030-01-03", codes: ["600001"] }); }
    finally { internals.ReactCurrentDispatcher.current = previous; }
    while (pendingEffects.length) pendingEffects.shift()!();
  };
  render(); render();
  return {
    queries,
    render,
    submit() { const form = elements(tree).find((node) => node.type === "form"); assert.ok(form); form.props.onSubmit!({ preventDefault() {} }); render(); },
    change(label: string, value: string) { const field = elements(tree).find((node) => node.props["aria-label"] === label); assert.ok(field); field.props.onChange!({ currentTarget: { value } }); render(); },
    click(label: string) { const button = elements(tree).find((node) => node.type === "button" && text(node).includes(label)); assert.ok(button); assert.equal(button.props.disabled, false); button.props.onClick!(); render(); },
    rows() { return elements(tree).find((node) => node.props.rows !== undefined)?.props.rows ?? []; },
    content() { return renderToStaticMarkup(tree); },
    replace(next: string) { scopeKey = next; render(); render(); },
    dispose() { cleanups.forEach((cleanup) => cleanup()); },
  };
}

function page(request: PersonalTradeHistoryRequest, first: boolean): PersonalTradeHistoryPage {
  const rows = (first ? Array.from({ length: 100 }, (_, index) => 100 - index) : [0]).map((receipt) => ({ receipt_id: String(receipt), civil_date: "2030-01-03", code: "600001", side: "Buy" as const, price: "1000", quantity_shares: 100, gross: "100000", actual_fees: { commission: "500", stamp_tax: "0", transfer_fee: "1" } }));
  return { request, confirmations: rows, next_cursor: first ? "1" : null, as_of_receipt: "101", start_date: "2030-01-02", current_date: "2030-01-05", settled_through: "2030-01-04" };
}
async function settle() { for (let index = 0; index < 5; index++) await Promise.resolve(); }

test("本人日期面板真实前后页保持receipt ceiling与查询filters，不重新读市场", { timeout: 10000 }, async () => {
  const ui = mounted();
  try {
    ui.submit();
    assert.equal(ui.queries.length, 1);
    ui.queries[0].resolve(page(ui.queries[0].request, true));
    await settle(); ui.render();
    assert.equal(ui.rows().length, 100);
    ui.click("下一页");
    assert.equal(ui.queries[1].request.before_receipt, "1");
    assert.equal(ui.queries[1].request.as_of_receipt, "101");
    ui.queries[1].resolve(page(ui.queries[1].request, false));
    await settle(); ui.render();
    assert.deepEqual(ui.rows().map((row) => row.receipt_id), ["0"]);
    ui.click("上一页");
    assert.equal(ui.rows().length, 100);
    assert.equal(ui.queries.length, 2);
    ui.click("下一页");
    assert.equal(ui.queries.length, 2);
    assert.equal(ui.rows()[0].receipt_id, "0");
  } finally { ui.dispose(); }
});

test("日期filter与generation/account变化拒绝迟到成功和错误，不把其他页安装当前复盘", { timeout: 10000 }, async () => {
  const ui = mounted();
  try {
    ui.submit();
    ui.change("交割方向筛选", "Sell");
    ui.queries[0].resolve(page(ui.queries[0].request, true));
    await settle(); ui.render();
    assert.equal(ui.rows().length, 0);
    ui.submit();
    assert.equal(ui.queries[1].request.side, "Sell");
    ui.replace("2:7");
    ui.queries[1].reject(new Error("旧本人错误"));
    await settle(); ui.render();
    assert.equal(ui.rows().length, 0);
    assert.doesNotMatch(ui.content(), /旧本人错误/);
    ui.submit();
    const empty = { ...page(ui.queries[2].request, false), confirmations: [] };
    ui.queries[2].resolve(empty);
    await settle(); ui.render();
    assert.match(ui.content(), /不能据此推断市场休市/);
  } finally { ui.dispose(); }
});

test("本人日期查询失败明确展示详情并允许重新请求，不安装假空页", { timeout: 10000 }, async () => {
  const ui = mounted();
  try {
    ui.submit();
    ui.queries[0].reject(new Error("交割历史服务失败：receipt窗口不合法"));
    await settle(); ui.render();
    assert.match(ui.content(), /交割历史服务失败：receipt窗口不合法/);
    assert.doesNotMatch(ui.content(), /不能据此推断市场休市/);
    ui.submit();
    assert.equal(ui.queries.length, 2);
    ui.queries[1].resolve(page(ui.queries[1].request, true));
    await settle(); ui.render();
    assert.equal(ui.rows().length, 100);
    assert.doesNotMatch(ui.content(), /交割历史服务失败/);
  } finally { ui.dispose(); }
});
