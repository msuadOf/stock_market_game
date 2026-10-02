import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createElement, type ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";

let vite: ViteDevServer;
let views: typeof import("./HostStatusViews.tsx");

before(async () => {
  vite = await createServer({
    configFile: false,
    appType: "custom",
    server: { middlewareMode: true, ws: false },
    optimizeDeps: { noDiscovery: true },
  });
  views = await vite.ssrLoadModule("/src/app/HostStatusViews.tsx") as typeof views;
});

after(async () => {
  if (vite) await vite.close();
});

const failure = {
  code: "STEP_FATAL",
  where: "engine.commit_tick",
  message: "收据校验失败",
  context: {
    tick: 12,
    seq: 42,
    token: "private-token",
    nested: { requestId: 7, password: "private-password" },
    rawSnippet: "private-save-contents",
  },
  cause: new Error("外层校验失败", { cause: new Error("内部收据不一致") }),
  recoverable: false,
  recoveryActions: ["复制反馈后检查运行环境"],
};

test("结构化错误显示真实详情、原因链与建议，context 脱敏且不改变原对象", () => {
  let html = "";
  assert.doesNotThrow(() => { html = renderToStaticMarkup(createElement(views.FatalHostError, { error: failure, onRetry: () => {} })); });
  assert.match(html, /STEP_FATAL/);
  assert.match(html, /engine\.commit_tick/);
  assert.match(html, /收据校验失败/);
  assert.match(html, /外层校验失败/);
  assert.match(html, /内部收据不一致/);
  assert.match(html, /tick/);
  assert.match(html, /42/);
  assert.match(html, /不可直接恢复/);
  assert.match(html, /复制反馈后检查运行环境/);
  assert.match(html, /复制错误反馈/);
  assert.doesNotMatch(html, /private-token|private-password|private-save-contents/);
  assert.equal(failure.context.token, "private-token");
});

test("仅三字段或字符串的错误仍可见，未提供的原因和恢复能力明确未知", () => {
  let html = "";
  assert.doesNotThrow(() => { html = renderToStaticMarkup(createElement(views.FatalHostError, {
    error: { code: "PROTOCOL_CURSOR", where: "protocol.seq", message: "序号不连续" },
    onRetry: () => {},
  })); });
  assert.match(html, /PROTOCOL_CURSOR/);
  assert.match(html, /原因链：未提供/);
  assert.match(html, /上下文：未提供/);
  assert.match(html, /恢复能力：未知/);
  const legacy = renderToStaticMarkup(createElement(views.FatalHostError, { error: "worker 请求超时", onRetry: () => {} }));
  assert.match(legacy, /worker 请求超时/);
  assert.match(legacy, /错误码：未提供/);
  assert.match(legacy, /刷新页面重试/);
});

type CopyStatus = { readonly kind: string; readonly message: string };
type CopyProps = { onCopy?: (report: (status: CopyStatus) => void) => Promise<void> };

function copyCallback(props: Parameters<typeof views.FatalHostError>[0]): NonNullable<CopyProps["onCopy"]> {
  const children = views.FatalHostError(props).props.children as ReactElement<CopyProps>[];
  const copy = children.find((child) => typeof child.props.onCopy === "function");
  assert.ok(copy, "FatalHostError 必须提供可调用的一键复制反馈回调");
  return copy.props.onCopy!;
}

test("复制回调使用注入 Clipboard 并报告真实成功，不把敏感 context 放入剪贴板", async () => {
  const copied: string[] = [];
  const statuses: CopyStatus[] = [];
  const copy = copyCallback({
    error: failure,
    onRetry: () => {},
    clipboard: { writeText: async (value: string) => { copied.push(value); } },
  });
  await copy((status) => statuses.push(status));
  assert.equal(copied.length, 1);
  assert.match(copied[0]!, /STEP_FATAL/);
  assert.match(copied[0]!, /engine\.commit_tick/);
  assert.match(copied[0]!, /内部收据不一致/);
  assert.doesNotMatch(copied[0]!, /private-token|private-password|private-save-contents/);
  assert.deepEqual(statuses.map((status) => status.kind), ["copying", "copied"]);
});

test("剪贴板拒绝或不可用显式提示失败、真实原因与手动反馈动作", async () => {
  for (const clipboard of [null, { writeText: async () => { throw new Error("Permission denied"); } }]) {
    const statuses: CopyStatus[] = [];
    await copyCallback({ error: "可见错误", onRetry: () => {}, clipboard })((status) => statuses.push(status));
    assert.deepEqual(statuses.map((status) => status.kind), ["copying", "failed"]);
    assert.match(statuses[1]!.message, /复制失败/);
    assert.match(statuses[1]!.message, /手动复制/);
    assert.match(statuses[1]!.message, clipboard === null ? /Clipboard.*不可用/ : /Permission denied/);
  }
});
