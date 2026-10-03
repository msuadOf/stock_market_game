import assert from "node:assert/strict";
import test from "node:test";
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { setImmediate } from "node:timers/promises";
import { createTauriHost } from "./tauri-host.ts";
import type { SessionSetup } from "../types/engine.ts";

test("Tauri save pins generation for keyed and latest candidates and rejects a late response after load", async () => {
  const originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", { configurable: true, value: { crypto: globalThis.crypto } });
  const requests: Record<string, unknown>[] = [];
  let completeSave: ((slot: unknown) => void) | null = null;
  const snapshot = { seq: 42, tick: 0, day: 0, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} };
  mockIPC((command, args) => {
    if (command === "create_session") return "session-1";
    if (command === "host_capabilities") return { npcDecisionDiagnostics: false };
    if (command === "engine_baseline") return { snapshot, generation: "1", timeline_id: "timeline-1" };
    if (command === "restore_session") return { snapshot, generation: "2", timeline_id: "timeline-2" };
    if (command === "save_session") {
      requests.push(args as Record<string, unknown>);
      return new Promise((resolve) => { completeSave = resolve; });
    }
    return null;
  }, { shouldMockEvents: true });
  try {
    const host = await createTauriHost({} as SessionSetup, 1n);
    const candidate = { seq: 42, settledDate: "2030-01-05" };
    const keyed = host.save(candidate);
    assert.deepEqual(requests[0], { sessionId: "session-1", generation: "1", candidate });
    completeSave!({ saved: true });
    await keyed;
    const latest = host.save();
    assert.deepEqual(requests[1], { sessionId: "session-1", generation: "1" });
    await host.load({});
    completeSave!({ old: true });
    await assert.rejects(latest, /generation/);
    host.dispose();
  } finally {
    clearMocks();
    if (originalWindow === undefined) delete (globalThis as { window?: unknown }).window;
    else Object.defineProperty(globalThis, "window", originalWindow);
  }
});

const snapshot = { seq: 42, tick: 0, day: 0, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} };
type IpcHandler = Parameters<typeof mockIPC>[0];

async function withTauriHost(run: (host: Awaited<ReturnType<typeof createTauriHost>>, calls: string[]) => Promise<void>, handler: IpcHandler = () => undefined) {
  const originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", { configurable: true, value: { crypto: globalThis.crypto } });
  const calls: string[] = [];
  mockIPC((command, args) => {
    calls.push(command);
    const response = handler(command, args);
    if (response !== undefined) return response;
    if (command === "create_session") return "session-1";
    if (command === "host_capabilities") return { npcDecisionDiagnostics: true };
    if (command === "engine_baseline") return { snapshot, generation: "1", timeline_id: "timeline-1" };
    if (command === "restore_session") return { snapshot: { ...snapshot, tick: 2 }, generation: "2", timeline_id: "timeline-2" };
    return null;
  }, { shouldMockEvents: true });
  let host: Awaited<ReturnType<typeof createTauriHost>> | undefined;
  try {
    host = await createTauriHost({} as SessionSetup, 1n);
    await run(host, calls);
  } finally {
    host?.dispose();
    clearMocks();
    if (originalWindow === undefined) delete (globalThis as { window?: unknown }).window;
    else Object.defineProperty(globalThis, "window", originalWindow);
  }
}

test("Tauri restore 切换 timeline 后过滤旧 event，并在新 baseline callback 后 resume", async () => {
  await withTauriHost(async (host, calls) => {
    const updates: unknown[] = [];
    const failures: unknown[] = [];
    host.start((update) => { updates.push(update); calls.push(`callback:${update.generation}`); }, (failure) => failures.push(failure));
    await setImmediate();
    calls.length = 0;
    await host.load({});
    assert.deepEqual(calls, ["pause_session", "restore_session", "callback:2", "resume_session"]);
    const update = { stale: true };
    await emit("engine-event", { session_id: "session-1", timeline_id: "timeline-1", update });
    await emit("engine-failure", { session_id: "session-1", timeline_id: "timeline-1", code: "OLD", message: "旧错误", events: [] });
    assert.equal(updates.length, 2);
    assert.equal(failures.length, 0);
    await emit("engine-event", { session_id: "session-1", timeline_id: "timeline-2", update });
    assert.deepEqual(updates.at(-1), { type: "protocol", generation: "2", update, civilDate: null, revision: null });
  });
});

test("Tauri 同 generation refresh 使 orders 与 NPC 查询失效，但 save 仍只检查 generation", async () => {
  let finishOrders!: (value: unknown) => void;
  let finishDiagnostics!: (value: unknown) => void;
  let finishSave!: (value: unknown) => void;
  await withTauriHost(async (host) => {
    const orders = assert.rejects(host.playerWorkingOrders(), /玩家活动委托响应属于已过期/);
    const diagnostics = assert.rejects(host.npcDecisionTrace!(1), /NPC 诊断响应属于已过期/);
    const saved = host.save();
    await host.refreshBaseline();
    finishOrders({ generation: "1", value: [] });
    finishDiagnostics({ generation: "1", value: {} });
    finishSave({ saved: true });
    await Promise.all([orders, diagnostics]);
    assert.deepEqual(await saved, { saved: true });
  }, (command) => {
    if (command === "player_working_orders") return new Promise((resolve) => { finishOrders = resolve; });
    if (command === "npc_decision_diagnostics") return new Promise((resolve) => { finishDiagnostics = resolve; });
    if (command === "save_session") return new Promise((resolve) => { finishSave = resolve; });
  });
});

test("Tauri restore snapshot 解析失败保留先写 generation/timeline 与旧 baseline，不 resume", async () => {
  await withTauriHost(async (host, calls) => {
    const updates: unknown[] = [];
    host.start((update) => updates.push(update));
    await setImmediate();
    calls.length = 0;
    await assert.rejects(host.load({}), /Tauri restore snapshot/);
    assert.deepEqual(calls, ["pause_session", "restore_session"]);
    assert.equal(host.tick(), 0);
    await emit("engine-event", { session_id: "session-1", timeline_id: "timeline-2", update: {} });
    assert.equal((updates.at(-1) as { generation: string }).generation, "2");
  }, (command) => command === "restore_session" ? { snapshot: null, generation: "2", timeline_id: "timeline-2" } : undefined);
});

test("Tauri baseline callback 抛错保留已安装状态，并阻止 restore 后 resume", async () => {
  await withTauriHost(async (host, calls) => {
    host.start((update) => { if (update.generation === "2") throw new Error("callback 失败"); });
    await setImmediate();
    calls.length = 0;
    await assert.rejects(host.load({}), /callback 失败/);
    assert.deepEqual(calls, ["pause_session", "restore_session"]);
    assert.equal(host.tick(), 2);
  });
});

test("Tauri restore 或 resume 失败仍保留既有 running 时序", async () => {
  let attempt = 0;
  let rejectResume = false;
  await withTauriHost(async (host, calls) => {
    host.start(() => {});
    await setImmediate();
    calls.length = 0;
    await assert.rejects(host.load({}), /restore 失败/);
    assert.deepEqual(calls, ["pause_session", "restore_session"]);
    assert.equal(host.tick(), 0);
    rejectResume = true;
    calls.length = 0;
    await assert.rejects(host.load({}), /resume 失败/);
    assert.deepEqual(calls, ["pause_session", "restore_session", "resume_session"]);
    assert.equal(host.tick(), 2);
    calls.length = 0;
    await assert.rejects(host.load({}), /没有递增 generation/);
    assert.equal(calls[0], "pause_session", "失败的 resume 不改变既有 running 标记");
  }, (command) => {
    if (command === "restore_session" && ++attempt === 1) return Promise.reject(new Error("restore 失败"));
    if (command === "resume_session" && rejectResume) return Promise.reject(new Error("resume 失败"));
  });
});

test("Tauri dispose 后晚到 refresh 与 civilDate 保留原处理，而 save 由 disposed guard 拒绝", async () => {
  let finishRefresh!: (value: unknown) => void;
  let finishDate!: (value: unknown) => void;
  let finishSave!: (value: unknown) => void;
  let baselineCalls = 0;
  await withTauriHost(async (host) => {
    const refresh = host.refreshBaseline();
    const date = host.civilDate!();
    const saved = assert.rejects(host.save(), /已过期会话 generation/);
    host.dispose();
    finishRefresh({ snapshot: { ...snapshot, tick: 3 }, generation: "1", timeline_id: "late" });
    finishDate({ value: "2030-01-02" });
    finishSave({ saved: true });
    await Promise.all([refresh, saved]);
    assert.equal(host.tick(), 3);
    assert.equal(await date, "2030-01-02");
  }, (command) => {
    if (command === "engine_baseline" && ++baselineCalls > 1) return new Promise((resolve) => { finishRefresh = resolve; });
    if (command === "civil_date") return new Promise((resolve) => { finishDate = resolve; });
    if (command === "save_session") return new Promise((resolve) => { finishSave = resolve; });
  });
});

test("Tauri 拒绝初始 generation 不匹配及 restore 非 nextGeneration", async () => {
  await assert.rejects(withTauriHost(async () => { assert.fail("初始化不应成功"); }, (command) => command === "engine_baseline"
    ? { snapshot, generation: "2", timeline_id: "wrong" } : undefined), /初始基线 generation 与新会话不匹配/);
  await withTauriHost(async (host) => {
    await assert.rejects(host.load({}), /没有递增 generation/);
    assert.equal(host.tick(), 0);
  }, (command) => command === "restore_session" ? { snapshot, generation: "9007199254740993", timeline_id: "wrong" } : undefined);
});

test("Tauri dispose 后晚到 load 保留原安装 restore baseline 的行为", async () => {
  let finishRestore!: (value: unknown) => void;
  await withTauriHost(async (host) => {
    const loading = host.load({});
    host.dispose();
    finishRestore({ snapshot: { ...snapshot, tick: 5 }, generation: "2", timeline_id: "late-load" });
    await loading;
    assert.equal(host.tick(), 5);
    assert.equal(host.snapshot().seq, 42);
  }, (command) => command === "restore_session" ? new Promise((resolve) => { finishRestore = resolve; }) : undefined);
});
