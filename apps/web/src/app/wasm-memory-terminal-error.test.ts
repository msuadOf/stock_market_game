import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createElement, type ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
import type { FeedbackCopyStatus } from "./error-details.ts";
import type { WasmMemoryTerminalFailure } from "./WasmMemoryTerminalError.tsx";

let vite: ViteDevServer;
let views: typeof import("./WasmMemoryTerminalError.tsx");
before(async () => {
  vite = await createServer({ configFile: false, appType: "custom",
    server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  views = await vite.ssrLoadModule("/src/app/WasmMemoryTerminalError.tsx") as typeof views;
});
after(async () => { if (vite) await vite.close(); });

function failure(code: WasmMemoryTerminalFailure["code"]): WasmMemoryTerminalFailure {
  return { code, where: "wasm-memory-storage.recovery", message: "残留账不能继续安全继承",
    cause: new Error("原始存储原因", { cause: new Error("内部校验失败") }),
    context: { carriedResidualBytes: 512, token: "private-token" } };
}

test("仅两种正本终局代码命中guard，不自行裁定其他内存错误的恢复权限", () => {
  assert.equal(views.isWasmMemoryTerminalFailure(failure("MEMORY_RESIDUAL_EXHAUSTED")), true);
  assert.equal(views.isWasmMemoryTerminalFailure(failure("MEMORY_HANDOVER_CORRUPT")), true);
  for (const code of ["MEMORY_MEASUREMENT_NOT_READY", "MEMORY_HANDOVER_WRITE_FAILED", "MEMORY_BUDGET_EXCEEDED", "MEMORY_RESIDUAL_RELOAD_HANDOVER", "STEP_FATAL"]) {
    assert.equal(views.isWasmMemoryTerminalFailure({ ...failure("MEMORY_HANDOVER_CORRUPT"), code }), false);
  }
  for (const input of [null, undefined, "MEMORY_HANDOVER_CORRUPT"]) assert.equal(views.isWasmMemoryTerminalFailure(input), false);
});

test("耗尽显示保险丝原因与全新标签页打开指引，不提供刷新/新局/别档/清账动作", () => {
  const html = renderToStaticMarkup(createElement(views.WasmMemoryTerminalError, { error: failure("MEMORY_RESIDUAL_EXHAUSTED") }));
  assert.match(html, /role="alert"/); assert.match(html, /内存保守残留预算已耗尽/);
  assert.match(html, /保险丝/); assert.match(html, /不代表真实泄漏/); assert.match(html, /全新标签页手动打开原网址/);
  assert.match(html, /复制错误反馈/);
  assert.doesNotMatch(html, />刷新页面重试<|>创建新游戏<|>选择其他日终存档<|>返回启动选择<|>清空残留账</);
  assert.equal((html.match(/<button\b/g) || []).length, 1);
});

test("损坏终局显示真实code/位置/原因链，反馈原样保留且不改变原错误", () => {
  const error = failure("MEMORY_HANDOVER_CORRUPT");
  const html = renderToStaticMarkup(createElement(views.WasmMemoryTerminalError, { error }));
  assert.match(html, /内存残留账无法安全恢复/); assert.match(html, /MEMORY_HANDOVER_CORRUPT/);
  assert.match(html, /wasm-memory-storage\.recovery/); assert.match(html, /原始存储原因/); assert.match(html, /内部校验失败/);
  assert.match(html, /private-token/);
  assert.equal((error.context as { token: string }).token, "private-token");
});

test("终局码覆盖宿主恢复建议的显示和复制投影，原错误元数据保持不变", async () => {
  for (const code of ["MEMORY_RESIDUAL_EXHAUSTED", "MEMORY_HANDOVER_CORRUPT"] as const) {
    const error = { ...failure(code), recoverable: true, recoveryActions: ["刷新页面重试", "创建新游戏"] };
    let copied = "";
    const props = { error, clipboard: { writeText: async (text: string) => { copied = text; } } };
    const html = renderToStaticMarkup(createElement(views.WasmMemoryTerminalError, props));
    assert.match(html, /全新标签页手动打开原网址/); assert.match(html, /不可直接恢复/);
    assert.equal((html.match(/<button\b/g) || []).length, 1);
    assert.doesNotMatch(html, /宿主声明可恢复|宿主建议动作：刷新页面重试/);
    const view = views.WasmMemoryTerminalError(props);
    type CopyProps = { onCopy?: (report: (status: FeedbackCopyStatus) => void) => Promise<void> };
    const copy = (view.props.children as ReactElement<CopyProps>[]).find((child) => typeof child?.props?.onCopy === "function");
    assert.ok(copy?.props.onCopy); await copy.props.onCopy(() => {});
    assert.match(copied, /不可直接恢复/); assert.doesNotMatch(copied, /宿主声明可恢复|宿主建议动作：刷新页面重试/);
    assert.equal(error.recoverable, true); assert.deepEqual(error.recoveryActions, ["刷新页面重试", "创建新游戏"]);
  }
});

test("复用反馈复制回调，成功与失败均如实给状态并保留原始原因与 context", async () => {
  let copied = "";
  const view = views.WasmMemoryTerminalError({ error: failure("MEMORY_RESIDUAL_EXHAUSTED"), clipboard: { writeText: async (text) => { copied = text; } } });
  type CopyProps = { onCopy?: (report: (status: FeedbackCopyStatus) => void) => Promise<void> };
  const copy = (view.props.children as ReactElement<CopyProps>[]).find((child) => typeof child?.props?.onCopy === "function");
  assert.ok(copy?.props.onCopy);
  const statuses: FeedbackCopyStatus[] = []; await copy.props.onCopy((status) => statuses.push(status));
  assert.deepEqual(statuses.map((status) => status.kind), ["copying", "copied"]);
  assert.match(copied, /MEMORY_RESIDUAL_EXHAUSTED/); assert.match(copied, /原始存储原因/); assert.match(copied, /private-token/);
  const cause = new Error("clipboard denied");
  const blocked = views.WasmMemoryTerminalError({ error: failure("MEMORY_HANDOVER_CORRUPT"), clipboard: { writeText: async () => { throw cause; } } });
  const second = (blocked.props.children as ReactElement<CopyProps>[]).find((child) => typeof child?.props?.onCopy === "function");
  assert.ok(second?.props.onCopy); const failed: FeedbackCopyStatus[] = [];
  await second.props.onCopy((status) => failed.push(status));
  assert.equal(failed.at(-1)?.kind, "failed"); assert.match(failed.at(-1)!.message, /clipboard denied/);
});

test("main真实 owner/storage 终局错误对象进入同一终局面", async () => {
  const owner = await vite.ssrLoadModule("/src/host/wasm-document-memory-owner.ts") as {
    WasmDocumentMemoryError: new (code: string, message: string, context?: Record<string, unknown>) => Error & { code: string; where: string };
  };
  const storage = await vite.ssrLoadModule("/src/host/wasm-memory-storage.ts") as {
    WasmMemoryStorageError: new (code: string, where: string, message: string, context: Record<string, unknown>) => Error & { code: string; where: string };
  };
  const cases = [
    new owner.WasmDocumentMemoryError("MEMORY_RESIDUAL_EXHAUSTED", "Memory 预算拒绝开局", { status: "EXHAUSTED" }),
    new storage.WasmMemoryStorageError("MEMORY_HANDOVER_CORRUPT", "wasm-memory-storage", "残留账无法安全读回", { reason: "SEQ_REGRESSED" }),
  ];
  for (const error of cases) {
    assert.equal(views.isWasmMemoryTerminalFailure(error as unknown as WasmMemoryTerminalFailure), true);
    const html = renderToStaticMarkup(createElement(views.WasmMemoryTerminalError, { error: error as unknown as WasmMemoryTerminalFailure }));
    assert.match(html, /全新标签页手动打开原网址/); assert.match(html, /不要使用浏览器恢复或复制标签页/);
    assert.match(html, new RegExp(error.message));
    assert.match(html, new RegExp(error.where));
  }
});

test("组件直接收到非终局、null 或 undefined 错误时显式拒绝，不冒充终局", () => {
  const error = { ...failure("MEMORY_HANDOVER_CORRUPT"), code: "MEMORY_MEASUREMENT_NOT_READY" };
  assert.throws(() => views.WasmMemoryTerminalError({ error: error as WasmMemoryTerminalFailure }), /WasmMemoryTerminalError.*MEMORY_MEASUREMENT_NOT_READY/);
  for (const invalid of [null, undefined]) {
    assert.throws(() => views.WasmMemoryTerminalError({ error: invalid as unknown as WasmMemoryTerminalFailure }), /WasmMemoryTerminalError.*收到非终局错误/);
  }
});

test("Error实例的非枚举message和cause在显示与复制中保留", async () => {
  const error = Object.assign(new Error("非枚举顶层原因", { cause: new Error("非枚举内层原因") }), {
    code: "MEMORY_HANDOVER_CORRUPT" as const, where: "wasm-memory-storage.recovery",
    recoverable: true, recoveryActions: ["创建新游戏"],
  });
  let copied = "";
  const props = { error, clipboard: { writeText: async (text: string) => { copied = text; } } };
  const html = renderToStaticMarkup(createElement(views.WasmMemoryTerminalError, props));
  assert.match(html, /非枚举顶层原因/); assert.match(html, /非枚举内层原因/);
  const view = views.WasmMemoryTerminalError(props);
  type CopyProps = { onCopy?: (report: (status: FeedbackCopyStatus) => void) => Promise<void> };
  const copy = (view.props.children as ReactElement<CopyProps>[]).find((child) => typeof child?.props?.onCopy === "function");
  assert.ok(copy?.props.onCopy); await copy.props.onCopy(() => {});
  assert.match(copied, /非枚举顶层原因/); assert.match(copied, /非枚举内层原因/);
  assert.equal(error.recoverable, true); assert.deepEqual(error.recoveryActions, ["创建新游戏"]);
});

test("终局 Error 的未知诊断 getter 与 stack getter 不执行，面板仍可显示", () => {
  let reads = 0;
  const error = Object.assign(new Error("原始终局错误"), { code: "MEMORY_HANDOVER_CORRUPT" as const, where: "storage" });
  for (const field of ["custom", "name", "stack"]) Object.defineProperty(error, field, {
    enumerable: true, get() { reads++; throw new Error(`${field} getter 不应执行`); },
  });
  let html = "";
  assert.doesNotThrow(() => { html = renderToStaticMarkup(createElement(views.WasmMemoryTerminalError, { error })); });
  assert.match(html, /原始终局错误/);
  assert.match(html, /访问器字段未执行/);
  assert.equal(reads, 0);
});
