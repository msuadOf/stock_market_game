import assert from "node:assert/strict";
import test from "node:test";
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { setImmediate } from "node:timers/promises";
import { createTauriHost } from "./tauri-host.ts";
import type { SessionSetup } from "../types/engine.ts";
import { DEFAULT_SETUP } from "../config/defaults.ts";

test("Tauri save pins generation for keyed and latest candidates and rejects a late response after load", async () => {
  const originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", { configurable: true, value: { crypto: globalThis.crypto } });
  const requests: Record<string, unknown>[] = [];
  let completeSave: ((slot: unknown) => void) | null = null;
  const snapshot = { seq: 42, tick: 0, day: 0, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} };
  mockIPC((command, args) => {
    if (command === "create_session") return { sessionId: "session-1", setup: DEFAULT_SETUP, seed: "1", resumed: false };
    if (command === "host_capabilities") return { npcDecisionDiagnostics: false, indicatorCapabilities: { intradayAverage: true, macd: true, priceKdj: true, candleKdj: true }, personalTradeHistory: true };
    if (command === "engine_baseline") return { snapshot, generation: "1", timeline_id: "timeline-1", civil_date: "2030-01-01" };
    if (command === "restore_session") return { snapshot, generation: "2", timeline_id: "timeline-2", civil_date: "2030-01-01" };
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

test("Tauri当前分钟使用独立Rust命令并拒绝旧generation", { timeout: 10000 }, async () => {
  const query = { code: "600000" };
  const response = { code: "600000", date: "2030-01-02", observed_at: { date: "2030-01-02", second_of_day: 0 }, live: true, status: "Trading", phase: "Continuous", bars: [] };
  let stale = false, received: unknown;
  await withTauriHost(async host => {
    assert.deepEqual(await host.queryCurrentMinuteHistory(query), response);
    assert.deepEqual(received, { sessionId: "session-1", generation: "1", query });
    stale = true;
    await assert.rejects(host.queryCurrentMinuteHistory(query), /generation/);
  }, (command, args) => {
    if (command === "current_minute_history") { received = args; return { generation: stale ? "0" : "1", response }; }
    return undefined;
  });
});

test("Tauri永久量价历史使用显式Rust命令和generation", { timeout: 10000 }, async () => {
  const query = { code: "600000", date_from: "2030-01-02", date_to: "2030-01-03", after: null, page_size: 2 };
  const page = { code: "600000", entries: ["2030-01-02", "2030-01-03"].map(date => ({ date, availability: "NotEnded", bars: [], daily_candle: null })), next_cursor: null, settled_through: null };
  let received: unknown;
  await withTauriHost(async (host) => {
    assert.deepEqual(await host.queryMarketHistory(query), page);
  }, (command, args) => {
    if (command === "market_history") { received = args; return { generation: "1", page }; }
    return undefined;
  });
  assert.deepEqual(received, { sessionId: "session-1", generation: "1", query });
});

test("Tauri按需VWAP与个人交割单查询只调用显式Rust能力命令", { timeout: 10000 }, async () => {
  const calls: { command: string; args: unknown }[] = [];
  await withTauriHost(async (host) => {
    assert.equal(host.capabilities.indicatorCapabilities.intradayAverage, true);
    assert.deepEqual(await host.calculateIntradayAverage({ turnoverCents: "260000", tradeCount: 2, volumeShares: 300 }), {
      turnoverCents: "260000", volumeShares: 300,
    });
    assert.deepEqual(await host.calculateIntradayAverageCurve({ seriesKey: "600000:1:901", samples: [{ turnoverCents: "260000", tradeCount: 2, volumeShares: 300 }] }), [
      { turnoverCents: "260000", volumeShares: 300 },
    ]);
    assert.deepEqual(await host.queryPersonalTradeConfirmations(), []);
  }, (command, args) => {
    calls.push({ command, args });
    if (command === "host_capabilities") return {
      npcDecisionDiagnostics: false,
      indicatorCapabilities: { intradayAverage: true, macd: true, priceKdj: true, candleKdj: true },
      personalTradeHistory: true,
    };
    if (command === "calculate_intraday_average") return { turnover_cents: "260000", volume_shares: 300 };
    if (command === "calculate_intraday_average_curve") return { series_key: "600000:1:901", results: [{ turnover_cents: "260000", volume_shares: 300 }] };
    if (command === "personal_trade_confirmations") return { generation: "1", confirmations: [] };
    return undefined;
  });
  assert.ok(calls.some(({ command }) => command === "calculate_intraday_average"));
  const history = calls.find(({ command }) => command === "personal_trade_confirmations");
  assert.ok(history);
  assert.equal((history.args as Record<string, unknown>).account, undefined);
});

const snapshot = { seq: 42, tick: 0, day: 0, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} };
test("Tauri本人日期页通过严格query参数与generation绑定，不允许指定账户", { timeout: 10000 }, async () => {
  const query = { date_from: "2030-01-02", date_to: "2030-01-04", code: "600001", side: "Buy", before_receipt: null, as_of_receipt: null, page_size: 100 } as const;
  const page = { request: query, confirmations: [], next_cursor: null, as_of_receipt: "10", start_date: "2030-01-02", current_date: "2030-01-05", settled_through: "2030-01-04" };
  let received: unknown;
  await withTauriHost(async (host) => { assert.deepEqual(await host.queryPersonalTradeHistory(query), page); }, (command, args) => {
    if (command === "personal_trade_history") { received = args; return { generation: "1", page }; }
    return undefined;
  });
  assert.deepEqual(received, { sessionId: "session-1", generation: "1", query });
});
type IpcHandler = Parameters<typeof mockIPC>[0];

async function withTauriHost(run: (host: Awaited<ReturnType<typeof createTauriHost>>, calls: string[]) => Promise<void>, handler: IpcHandler = () => undefined) {
  const originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", { configurable: true, value: { crypto: globalThis.crypto } });
  const calls: string[] = [];
  mockIPC((command, args) => {
    calls.push(command);
    const response = handler(command, args);
    if (response !== undefined) return response;
    if (command === "create_session") return { sessionId: "session-1", setup: DEFAULT_SETUP, seed: "1", resumed: false };
    if (command === "host_capabilities") return { npcDecisionDiagnostics: true, indicatorCapabilities: { intradayAverage: true, macd: true, priceKdj: true, candleKdj: true }, personalTradeHistory: true };
    if (command === "engine_baseline") return { snapshot, generation: "1", timeline_id: "timeline-1", civil_date: "2030-01-01" };
    if (command === "restore_session") return { snapshot: { ...snapshot, tick: 2 }, generation: "2", timeline_id: "timeline-2", civil_date: "2030-01-01" };
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

test("Tauri enqueue 严格携带当前 generation，restore 后旧入队确认不得报告新代成功", { timeout: 10000 }, async () => {
  const requests: unknown[] = [];
  let complete: (() => void) | undefined;
  await withTauriHost(async (host) => {
    const intent = { Cancel: { code: "600101", id: 7 } };
    const pending = host.submitIntent(intent);
    const rejected = assert.rejects(pending, /已过期.*generation/);
    await host.load({});
    assert.ok(complete);
    complete();
    await rejected;
    assert.deepEqual(requests[0], { sessionId: "session-1", generation: "1", intent });
    const current = host.submitIntent(intent);
    await Promise.resolve();
    assert.ok(complete);
    complete();
    await current;
    assert.deepEqual(requests[1], { sessionId: "session-1", generation: "2", intent });
  }, (command, args) => {
    if (command !== "enqueue") return undefined;
    requests.push(args);
    return new Promise<void>((resolve) => { complete = resolve; });
  });
});

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
    const diagnostics = assert.rejects(host.npcDecisionTrace!("1"), /NPC 诊断响应属于已过期/);
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

test("Tauri 股票历史查询绑定 generation，刷新后拒绝旧结果，调用参数不含账户", { timeout: 10000 }, async () => {
  let finishHistory!: (value: unknown) => void;
  await withTauriHost(async (host, calls) => {
    const history = assert.rejects(host.queryStockHistory("600000"), /股票历史响应属于已过期/);
    await host.refreshBaseline();
    finishHistory({ generation: "1", value: { code: "600000", daily_candles: [], active_daily_candle: null } });
    await history;
    assert.ok(calls.includes("query_stock_history"));
  }, (command, args) => {
    if (command === "query_stock_history") {
      assert.deepEqual(args, { sessionId: "session-1", generation: "1", code: "600000" });
      return new Promise((resolve) => { finishHistory = resolve; });
    }
  });
});

test("Tauri restore snapshot 解析失败保留先写 generation/timeline 与旧 baseline，不 resume", async () => {
  await withTauriHost(async (host, calls) => {
    const updates: unknown[] = [];
    host.start((update) => { updates.push(update); });
    await setImmediate();
    calls.length = 0;
    await assert.rejects(host.load({}), /Tauri restore snapshot/);
    assert.deepEqual(calls, ["pause_session", "restore_session"]);
    assert.equal(host.tick(), 0);
    await emit("engine-event", { session_id: "session-1", timeline_id: "timeline-2", update: {} });
    assert.equal((updates.at(-1) as { generation: string }).generation, "2");
  }, (command) => command === "restore_session" ? { snapshot: null, generation: "2", timeline_id: "timeline-2", civil_date: "2030-01-01" } : undefined);
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

test("Tauri restore 失败后保留已确认暂停，不冒充 running", async () => {
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
    await host.load({});
    assert.deepEqual(calls, ["restore_session"]);
    assert.equal(host.tick(), 2);
    await assert.rejects(host.start(() => {}), /resume 失败/);
    calls.length = 0;
    await assert.rejects(host.load({}), /没有递增 generation/);
    assert.equal(calls[0], "restore_session", "已经确认暂停后不得冒充 running");
  }, (command) => {
    if (command === "restore_session" && ++attempt === 1) return Promise.reject(new Error("restore 失败"));
    if (command === "resume_session" && rejectResume) return Promise.reject(new Error("resume 失败"));
  });
});

test("Tauri dispose 后晚到 refresh 与 save 拒绝，不能复活基线", async () => {
  let finishRefresh!: (value: unknown) => void;
  let finishDate!: (value: unknown) => void;
  let finishSave!: (value: unknown) => void;
  let baselineCalls = 0;
  await withTauriHost(async (host) => {
    const refresh = assert.rejects(host.refreshBaseline(), /已销毁会话/);
    const date = host.civilDate!();
    const saved = assert.rejects(host.save(), /已过期会话 generation/);
    host.dispose();
    finishRefresh({ snapshot: { ...snapshot, tick: 3 }, generation: "1", timeline_id: "late", civil_date: "2030-01-01" });
    finishDate({ value: "2030-01-02" });
    finishSave({ saved: true });
    await Promise.all([refresh, saved]);
    assert.throws(() => host.tick(), /基线尚未就绪/);
    assert.equal(await date, "2030-01-02");
  }, (command) => {
    if (command === "engine_baseline" && ++baselineCalls > 1) return new Promise((resolve) => { finishRefresh = resolve; });
    if (command === "civil_date") return new Promise((resolve) => { finishDate = resolve; });
    if (command === "save_session") return new Promise((resolve) => { finishSave = resolve; });
  });
});

test("Tauri 拒绝初始 generation 不匹配及 restore 非 nextGeneration", async () => {
  await assert.rejects(withTauriHost(async () => { assert.fail("初始化不应成功"); }, (command) => command === "engine_baseline"
    ? { snapshot, generation: "2", timeline_id: "wrong", civil_date: "2030-01-01" } : undefined), /初始基线 generation 与新会话不匹配/);
  await withTauriHost(async (host) => {
    await assert.rejects(host.load({}), /没有递增 generation/);
    assert.equal(host.tick(), 0);
  }, (command) => command === "restore_session" ? { snapshot, generation: "9007199254740993", timeline_id: "wrong", civil_date: "2030-01-01" } : undefined);
});

test("Tauri坏恢复generation、snapshot或日期不能提前确认所选档配置", { timeout: 10000 }, async () => {
  for (const response of [
    { snapshot, generation: "1", timeline_id: "bad-generation", civil_date: "2030-01-01" },
    { snapshot: null, generation: "2", timeline_id: "bad-snapshot", civil_date: "2030-01-01" },
    { snapshot, generation: "2", timeline_id: "bad-date", civil_date: "2030-02-30" },
  ]) {
    let confirmations = 0;
    await withTauriHost(async host => {
      await assert.rejects(host.load({}, "selected-archive", () => { confirmations++; }));
      assert.equal(confirmations, 0);
    }, command => command === "restore_session" ? response : undefined);
  }
});

test("Tauri start 与 stop 等待 IPC 应用确认，恢复不重送旧 baseline", { timeout: 10000 }, async () => {
  let finishControl: (() => void) | null = null;
  await withTauriHost(async (host) => {
    let applied = false;
    const updates: unknown[] = [];
    const starting = host.start((update) => { updates.push(update); }).then(() => { applied = true; });
    await setImmediate();
    assert.equal(applied, false);
    assert.ok(finishControl);
    finishControl();
    await starting;
    const stopping = host.stop();
    await setImmediate();
    assert.ok(finishControl);
    finishControl();
    await stopping;
    const restarting = host.start((update) => { updates.push(update); });
    await setImmediate();
    assert.ok(finishControl);
    finishControl();
    await restarting;
    assert.equal(updates.length, 1);
  }, (command) => ["resume_session", "pause_session"].includes(command)
    ? new Promise<void>((resolve) => { finishControl = resolve; }) : undefined);
});

test("Tauri cached baseline 消费者拒绝时 start 不得恢复 actor 或标记已交付", { timeout: 10000 }, async () => {
  await withTauriHost(async (host, calls) => {
    calls.length = 0;
    await assert.rejects(host.start(() => false), /消费者拒绝.*baseline/);
    assert.equal(calls.includes("resume_session"), false);
    const updates: unknown[] = [];
    await host.start((update) => { updates.push(update); return true; });
    assert.equal(updates.length, 1);
    assert.equal(calls.filter((command) => command === "resume_session").length, 1);
  });
});

test("Tauri refresh baseline 被拒绝时暂停 actor，并在重启时重新交付", { timeout: 10000 }, async () => {
  await withTauriHost(async (host, calls) => {
    let accept = true;
    await host.start(() => accept);
    accept = false;
    calls.length = 0;
    await assert.rejects(host.refreshBaseline(), /消费者拒绝.*baseline/);
    assert.deepEqual(calls, ["engine_baseline", "pause_session"]);
    const updates: unknown[] = [];
    await host.start((update) => { updates.push(update); return true; });
    assert.equal(updates.length, 1);
    assert.equal(calls.filter((command) => command === "resume_session").length, 1);
  });
});

test("Tauri restore baseline 被拒绝时不恢复 actor，并在重启时重新交付", { timeout: 10000 }, async () => {
  await withTauriHost(async (host, calls) => {
    await host.start((update) => update.generation !== "2");
    calls.length = 0;
    await assert.rejects(host.load({}), /消费者拒绝.*baseline/);
    assert.deepEqual(calls, ["pause_session", "restore_session"]);
    assert.equal(host.tick(), 2);
    const updates: unknown[] = [];
    await host.start((update) => { updates.push(update); return true; });
    assert.equal(updates.length, 1);
    assert.equal((updates[0] as { generation: string }).generation, "2");
    assert.equal(calls.filter((command) => command === "resume_session").length, 1);
  });
});

test("Tauri 未启动消费者时 refresh 与 restore 不标记 baseline 已交付", { timeout: 10000 }, async () => {
  for (const operation of ["refreshBaseline", "load"] as const) {
    await withTauriHost(async (host, calls) => {
      if (operation === "load") await host.load({});
      else await host.refreshBaseline();
      assert.equal(calls.includes("resume_session"), false);
      const updates: unknown[] = [];
      await host.start((update) => { updates.push(update); });
      assert.equal(updates.length, 1);
    });
  }
});

test("Tauri dispose 即使 IPC 失败也释放监听并显式拒绝", { timeout: 10000 }, async () => {
  await withTauriHost(async (host, calls) => {
    await assert.rejects(host.dispose(), /stop 失败/);
    assert.ok(calls.includes("stop_session"));
    await host.dispose();
  }, (command) => command === "stop_session" ? Promise.reject(new Error("stop 失败")) : undefined);
});

test("Tauri dispose 后晚到 load 拒绝且不安装 restore baseline", async () => {
  let finishRestore!: (value: unknown) => void;
  await withTauriHost(async (host) => {
    const loading = assert.rejects(host.load({}), /已销毁会话/);
    host.dispose();
    finishRestore({ snapshot: { ...snapshot, tick: 5 }, generation: "2", timeline_id: "late-load", civil_date: "2030-01-01" });
    await loading;
    assert.throws(() => host.tick(), /基线尚未就绪/);
    assert.throws(() => host.snapshot(), /基线尚未就绪/);
  }, (command) => command === "restore_session" ? new Promise((resolve) => { finishRestore = resolve; }) : undefined);
});


test("Tauri 恢复提交在 baseline/恢复运行前通知，恢复运行失败仍保留提交通知", { timeout: 10000 }, async () => {
  let restored = false, refuseResume = false;
  await withTauriHost(async (host, calls) => {
    await host.start(update => { if (update.generation === "2") assert.equal(restored, true); });
    calls.length = 0;
    refuseResume = true;
    await assert.rejects(host.load({}, "selected-archive", () => { restored = true; calls.push("restored-setup"); }), /恢复运行失败/);
    assert.equal(restored, true);
    assert.deepEqual(calls, ["pause_session", "restore_session", "restored-setup", "resume_session"]);
    assert.equal(host.tick(), 2);
  }, (command, args) => {
    if (command === "restore_session") assert.deepEqual(args, { sessionId: "session-1", generation: "1", slot: {}, archiveSlotId: "selected-archive" });
    return command === "resume_session" && refuseResume ? Promise.reject(new Error("恢复运行失败")) : undefined;
  });
});
