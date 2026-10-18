import assert from "node:assert/strict";
import test from "node:test";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { createTauriHost } from "./tauri-host.ts";
import type { SessionSetup } from "../types/engine.ts";

const initialSnapshot = { seq: 42, tick: 0, day: 0, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} };
type IpcHandler = Parameters<typeof mockIPC>[0];

async function withInitializationIpc(run: (calls: { command: string; args: Record<string, unknown> }[]) => Promise<void>, handler: IpcHandler) {
  const originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", { configurable: true, value: { crypto: globalThis.crypto } });
  const calls: { command: string; args: Record<string, unknown> }[] = [];
  mockIPC((command, args) => {
    calls.push({ command, args: args as Record<string, unknown> });
    const response = handler(command, args);
    if (response !== undefined) return response;
    if (command === "plugin:event|listen") return (args as { handler: number }).handler;
    if (command === "create_session") return "owned-session";
    if (command === "engine_baseline") return { snapshot: initialSnapshot, generation: "1", timeline_id: "owned-timeline", civil_date: "2030-01-01" };
    if (command === "host_capabilities") return { npcDecisionDiagnostics: false };
    return null;
  });
  try {
    await run(calls);
  } finally {
    clearMocks();
    if (originalWindow === undefined) delete (globalThis as { window?: unknown }).window;
    else Object.defineProperty(globalThis, "window", originalWindow);
  }
}

test("Tauri 第二个 listener 初始化失败释放第一个，不创建或停止任何会话", { timeout: 10000 }, async () => {
  await withInitializationIpc(async (calls) => {
    await assert.rejects(createTauriHost({} as SessionSetup, 1n), /Tauri 会话初始化失败.*failure listener 失败/);
    assert.deepEqual(calls.map(({ command }) => command), ["plugin:event|listen", "plugin:event|listen", "plugin:event|unlisten"]);
    assert.equal(calls[2]!.args.event, "engine-event");
    assert.equal(calls[2]!.args.eventId, calls[0]!.args.handler);
  }, (command, args) => command === "plugin:event|listen" && (args as { event: string }).event === "engine-failure"
    ? Promise.reject(new Error("failure listener 失败")) : undefined);
});

test("Tauri 首个 listener 或 create_session 失败不误停止已有其他会话", { timeout: 10000 }, async () => {
  for (const failedCommand of ["plugin:event|listen", "create_session"]) {
    await withInitializationIpc(async (calls) => {
      await assert.rejects(createTauriHost({} as SessionSetup, 1n), /初始化失败/);
      assert.equal(calls.some(({ command }) => command === "stop_session"), false);
      assert.equal(calls.filter(({ command }) => command === "plugin:event|unlisten").length, failedCommand === "create_session" ? 2 : 0);
    }, (command) => command === failedCommand ? Promise.reject(new Error("初始化失败")) : undefined);
  }
});

test("Tauri 会话创建后的 baseline 或 capabilities 失败释放两个 listener 并停止本人会话", { timeout: 10000 }, async () => {
  for (const failedCommand of ["engine_baseline", "host_capabilities"]) {
    await withInitializationIpc(async (calls) => {
      await assert.rejects(createTauriHost({} as SessionSetup, 1n), /初始化失败/);
      const stopped = calls.filter(({ command }) => command === "stop_session");
      assert.equal(stopped.length, 1);
      assert.deepEqual(stopped[0]!.args, { sessionId: "owned-session" });
      assert.deepEqual(calls.filter(({ command }) => command === "plugin:event|unlisten").map(({ args }) => args.event).sort(), ["engine-event", "engine-failure"]);
    }, (command) => command === failedCommand ? Promise.reject(new Error(`${failedCommand} 初始化失败`)) : undefined);
  }
});

test("Tauri 初始化失败保留原错误且汇总全部清理错误，某一项失败不阻止其他清理", { timeout: 10000 }, async () => {
  await withInitializationIpc(async (calls) => {
    await assert.rejects(createTauriHost({} as SessionSetup, 1n), (error: unknown) => {
      assert.ok(error instanceof AggregateError);
      assert.match(error.message, /baseline 初始化失败/);
      assert.match(error.message, /engine-event.*释放失败/);
      assert.match(error.message, /engine-failure.*释放失败/);
      assert.match(error.message, /stop_session.*停止失败/);
      assert.equal(error.errors.length, 4);
      return true;
    });
    assert.equal(calls.filter(({ command }) => command === "plugin:event|unlisten").length, 2);
    assert.equal(calls.filter(({ command }) => command === "stop_session").length, 1);
  }, (command, args) => {
    if (command === "engine_baseline") return Promise.reject(new Error("baseline 初始化失败"));
    if (command === "plugin:event|unlisten") return Promise.reject(new Error(`${(args as { event: string }).event} 释放失败`));
    if (command === "stop_session") return Promise.reject(new Error("停止失败"));
  });
});

test("Tauri 非法创建响应不取得 handle 归属，非法 baseline 或 capability 响应回收已取得会话", { timeout: 10000 }, async () => {
  for (const [failedCommand, response, expectedStops] of [
    ["create_session", { session_id: "untrusted-session" }, 0],
    ["engine_baseline", { snapshot: null, generation: "1", timeline_id: "owned-timeline", civil_date: "2030-01-01" }, 1],
    ["host_capabilities", { npcDecisionDiagnostics: "invalid" }, 1],
  ] as const) {
    await withInitializationIpc(async (calls) => {
      await assert.rejects(createTauriHost({} as SessionSetup, 1n), /Tauri 会话初始化失败/);
      assert.equal(calls.filter(({ command }) => command === "plugin:event|unlisten").length, 2);
      const stopped = calls.filter(({ command }) => command === "stop_session");
      assert.equal(stopped.length, expectedStops);
      if (expectedStops > 0) assert.deepEqual(stopped[0]!.args, { sessionId: "owned-session" });
    }, (command) => command === failedCommand ? response : undefined);
  }
});

test("Tauri 初始化失败必须等 stop_session 确认，不能提前交还初始化终态", { timeout: 10000 }, async () => {
  let completeStop!: () => void;
  let stopRequested!: () => void;
  const stopping = new Promise<void>((resolve) => { stopRequested = resolve; });
  await withInitializationIpc(async () => {
    let settled = false;
    const initializing = assert.rejects(createTauriHost({} as SessionSetup, 1n), /baseline 失败/).then(() => { settled = true; });
    await stopping;
    assert.equal(settled, false);
    completeStop();
    await initializing;
    assert.equal(settled, true);
  }, (command) => {
    if (command === "engine_baseline") return Promise.reject(new Error("baseline 失败"));
    if (command === "stop_session") return new Promise<void>((resolve) => { completeStop = resolve; stopRequested(); });
  });
});
