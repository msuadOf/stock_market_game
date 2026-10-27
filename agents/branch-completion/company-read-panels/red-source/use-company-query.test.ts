import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import React, { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { createServer, type ViteDevServer } from "vite";
import { useCompanyQuery } from "./use-company-query.ts";

/** 仅实现 ReactDOM 所需的内存 DOM；hooks、调度、effect 与 cleanup 均由真实 React 执行。 */
class MemoryNode {
  nodeType: number;
  nodeName: string;
  tagName: string;
  namespaceURI = "http://www.w3.org/1999/xhtml";
  ownerDocument: MemoryDocument;
  parentNode: MemoryNode | null = null;
  childNodes: MemoryNode[] = [];
  style: Record<string, unknown> = {};
  attributes: Record<string, string> = {};
  private content = "";
  constructor(name: string, document: MemoryDocument, type = 1) {
    this.nodeName = name.toUpperCase(); this.tagName = this.nodeName;
    this.ownerDocument = document; this.nodeType = type;
  }
  get firstChild() { return this.childNodes[0] ?? null; }
  get lastChild() { return this.childNodes.at(-1) ?? null; }
  get textContent(): string { return this.content + this.childNodes.map((child) => child.textContent).join(""); }
  set textContent(value: string) { this.content = value; this.childNodes.forEach((child) => { child.parentNode = null; }); this.childNodes = []; }
  get nodeValue() { return this.content; }
  set nodeValue(value: string) { this.content = value; }
  appendChild(child: MemoryNode) { if (child.parentNode) child.parentNode.removeChild(child); this.childNodes.push(child); child.parentNode = this; return child; }
  removeChild(child: MemoryNode) { this.childNodes.splice(this.childNodes.indexOf(child), 1); child.parentNode = null; return child; }
  insertBefore(child: MemoryNode, before: MemoryNode) { if (child.parentNode) child.parentNode.removeChild(child); this.childNodes.splice(this.childNodes.indexOf(before), 0, child); child.parentNode = this; return child; }
  setAttribute(key: string, value: string) { this.attributes[key] = String(value); }
  removeAttribute(key: string) { delete this.attributes[key]; }
  addEventListener() {}
  removeEventListener() {}
}
class MemoryDocument {
  nodeType = 9;
  activeElement: MemoryNode | null = null;
  defaultView = { HTMLElement: MemoryNode, HTMLIFrameElement: class {} };
  createElement(name: string) { return new MemoryNode(name, this); }
  createElementNS(_namespace: string, name: string) { return this.createElement(name); }
  createTextNode(value: string) { const node = new MemoryNode("#text", this, 3); node.nodeValue = value; return node; }
  addEventListener() {}
  removeEventListener() {}
}
const document = new MemoryDocument();
const savedWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
Object.defineProperty(globalThis, "window", { configurable: true, value: document.defaultView });
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
let vite: ViteDevServer;
let tax: typeof import("./DividendTaxPanel.tsx");
let contract: typeof import("./CompanyContractPanel.tsx");
let preferences: typeof import("./PreferenceRejectionsPanel.tsx");
let rejections: typeof import("./RightsSubscriptionRejectionPanel.tsx");
before(async () => {
  vite = await createServer({ configFile: false, root: new URL("../../../", import.meta.url).pathname, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  [tax, contract, preferences, rejections] = await Promise.all([
    vite.ssrLoadModule("/src/components/company/DividendTaxPanel.tsx"),
    vite.ssrLoadModule("/src/components/company/CompanyContractPanel.tsx"),
    vite.ssrLoadModule("/src/components/company/PreferenceRejectionsPanel.tsx"),
    vite.ssrLoadModule("/src/components/company/RightsSubscriptionRejectionPanel.tsx"),
  ]) as [typeof tax, typeof contract, typeof preferences, typeof rejections];
});
after(async () => { await vite.close(); if (savedWindow) Object.defineProperty(globalThis, "window", savedWindow); else Reflect.deleteProperty(globalThis, "window"); Reflect.deleteProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT"); });
function mounted() {
  const container = document.createElement("div");
  const root: Root = createRoot(container as unknown as Element);
  return { container, render: async (element: React.ReactNode) => { await act(async () => { root.render(element); }); }, unmount: async () => { await act(async () => root.unmount()); } };
}
function deferred<T>() {
  let resolve!: (value: T) => void; let reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function Probe({ query, refreshKey }: { query: (() => Promise<string>) | undefined; refreshKey: string }) {
  const [data, error, loading, refresh] = useCompanyQuery(query, refreshKey);
  return createElement("div", null, loading ? "loading" : error === null ? data === null ? "idle" : data : error, createElement("button", { onClick: refresh }, "refresh"));
}
function elements(node: MemoryNode): MemoryNode[] { return [node, ...node.childNodes.flatMap(elements)]; }
async function click(container: MemoryNode, text: string) {
  const button = elements(container).find((node) => node.tagName === "BUTTON" && node.textContent === text);
  assert.ok(button, `未找到按钮：${text}`);
  const key = Object.keys(button).find((name) => name.startsWith("__reactProps$")); assert.ok(key);
  const props = (button as unknown as Record<string, { onClick: () => void }>)[key]!;
  await act(async () => props.onClick());
}

test("移除宿主回调立即清除旧owner事实，迟到成功不能污染新面板", async () => {
  const view = mounted(); const request = deferred<string>();
  await view.render(createElement(Probe, { query: () => request.promise, refreshKey: "owner-a" }));
  await act(async () => request.resolve("旧账户税表"));
  assert.match(view.container.textContent, /旧账户税表/);
  await view.render(createElement(Probe, { query: undefined, refreshKey: "owner-b" }));
  assert.doesNotMatch(view.container.textContent, /旧账户税表/);
  await view.unmount();
});

test("股息税无实际划收回执时保留未划收额，不宣称资金不足、已尝试或结清", async () => {
  const view = mounted();
  await view.render(createElement(tax.DividendTaxPanel, { refreshKey: "owner-b", onQuery: async () => ({ status: { mode: "AShareIndividual", identity: "Personal", stocks: [] }, outstanding: [{ account: "9", stock: "600001", outstanding: { numerator: "3", denominator: "2" }, latest_collection: null }] }) }));
  assert.match(view.container.textContent, /3\/2 分/);
  assert.match(view.container.textContent, /尚无实际划收回执/);
  assert.doesNotMatch(view.container.textContent, /资金不足|已结清|已尝试/);
  await view.unmount();
});
