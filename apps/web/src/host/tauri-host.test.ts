import { market, dailyCandle, mirrorOrigin } from "./protocol-test-fixtures.ts";
import { catalogWireFixture, defaultStockCatalogFixture } from "./catalog-test-fixture.ts";
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
  const snapshot = { seq: 42, tick: 0, day: 0, phase: "Continuous", mirror_origin: mirrorOrigin(0), markets: Object.fromEntries(defaultStockCatalogFixture.map(stock => [String(stock.id), market()])), accounts: {}, daily_candles: Object.fromEntries(defaultStockCatalogFixture.map(stock => [String(stock.id), []])), active_daily_candles: Object.fromEntries(defaultStockCatalogFixture.map(stock => [String(stock.id), dailyCandle()])) };
  mockIPC((command, args) => {
    if (command === "create_session") return "session-1";
    if (command === "host_capabilities") return { npcDecisionDiagnostics: false };
    if (command === "engine_baseline") return { ...catalogWireFixture(snapshot), snapshot, generation: "1", timeline_id: "timeline-1", civil_date: "2030-01-01" };
    if (command === "restore_session") return { ...catalogWireFixture(snapshot), snapshot, generation: "2", timeline_id: "timeline-2", civil_date: "2030-01-01" };
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

const snapshot = { seq: 42, tick: 0, day: 0, phase: "Continuous", mirror_origin: mirrorOrigin(0), markets: Object.fromEntries(defaultStockCatalogFixture.map(stock => [String(stock.id), market()])), accounts: {}, daily_candles: Object.fromEntries(defaultStockCatalogFixture.map(stock => [String(stock.id), []])), active_daily_candles: Object.fromEntries(defaultStockCatalogFixture.map(stock => [String(stock.id), dailyCandle()])) };
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
    if (command === "engine_baseline") return { ...catalogWireFixture(snapshot), snapshot, generation: "1", timeline_id: "timeline-1", civil_date: "2030-01-01" };
    if (command === "restore_session") return { ...catalogWireFixture({ ...snapshot, tick: 2, mirror_origin: mirrorOrigin(2) }), snapshot: { ...snapshot, tick: 2, mirror_origin: mirrorOrigin(2) }, generation: "2", timeline_id: "timeline-2", civil_date: "2030-01-01" };
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
    assert.deepEqual(requests[0], { sessionId: "session-1", generation: "1", intent: { Cancel: { code: 3, id: 7 } } });
    const current = host.submitIntent(intent);
    await Promise.resolve();
    assert.ok(complete);
    complete();
    await current;
    assert.deepEqual(requests[1], { sessionId: "session-1", generation: "2", intent: { Cancel: { code: 3, id: 7 } } });
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

test("Tauri 股票历史查询绑定 generation，刷新后拒绝旧结果，调用参数不含账户", { timeout: 10000 }, async () => {
  let finishHistory!: (value: unknown) => void;
  await withTauriHost(async (host, calls) => {
    const history = assert.rejects(host.queryStockHistory("000812"), /股票历史响应属于已过期/);
    await host.refreshBaseline();
    finishHistory({ generation: "1", value: { code: 0, daily_candles: [], active_daily_candle: null } });
    await history;
    assert.ok(calls.includes("query_stock_history"));
  }, (command, args) => {
    if (command === "query_stock_history") {
      assert.deepEqual(args, { sessionId: "session-1", generation: "1", code: 0 });
      return new Promise((resolve) => { finishHistory = resolve; });
    }
  });
});

test("Tauri restore snapshot 解析失败保留可见 generation/timeline 与旧 baseline，不 resume", async () => {
  await withTauriHost(async (host, calls) => {
    const updates: unknown[] = [];
    host.start((update) => { updates.push(update); });
    await setImmediate();
    calls.length = 0;
    await assert.rejects(host.load({}), /baseline.snapshot/);
    assert.deepEqual(calls, ["pause_session", "restore_session"]);
    assert.equal(host.tick(), 0);
    await emit("engine-event", { session_id: "session-1", timeline_id: "timeline-2", update: {} });
    assert.equal((updates.at(-1) as { generation: string }).generation, "1");
  }, (command) => command === "restore_session" ? { ...catalogWireFixture(null), snapshot: null, generation: "2", timeline_id: "timeline-2", civil_date: "2030-01-01" } : undefined);
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
    finishRefresh({ ...catalogWireFixture({ ...snapshot, tick: 3, mirror_origin: mirrorOrigin(3) }), snapshot: { ...snapshot, tick: 3, mirror_origin: mirrorOrigin(3) }, generation: "1", timeline_id: "late", civil_date: "2030-01-01" });
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
    ? { ...catalogWireFixture(snapshot), snapshot, generation: "2", timeline_id: "wrong", civil_date: "2030-01-01" } : undefined), /初始基线 generation 与新会话不匹配/);
  await withTauriHost(async (host) => {
    await assert.rejects(host.load({}), /没有递增 generation/);
    assert.equal(host.tick(), 0);
  }, (command) => command === "restore_session" ? { ...catalogWireFixture(snapshot), snapshot, generation: "9007199254740993", timeline_id: "wrong", civil_date: "2030-01-01" } : undefined);
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
    finishRestore({ ...catalogWireFixture({ ...snapshot, tick: 5, mirror_origin: mirrorOrigin(5) }), snapshot: { ...snapshot, tick: 5, mirror_origin: mirrorOrigin(5) }, generation: "2", timeline_id: "late-load", civil_date: "2030-01-01" });
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
    await assert.rejects(host.load({}, () => { restored = true; calls.push("restored-setup"); }), /恢复运行失败/);
    assert.equal(restored, true);
    assert.deepEqual(calls, ["pause_session", "restore_session", "restored-setup", "resume_session"]);
    assert.equal(host.tick(), 2);
  }, command => command === "resume_session" && refuseResume ? Promise.reject(new Error("恢复运行失败")) : undefined);
});

function actualTauriIntentBaseline(generation: string, codes: readonly string[]) {
  const stock_catalog = [...codes].sort().map((code, id) => ({ id, code }));
  return { generation, timeline_id: `actual-${generation}`, civil_date: "2030-01-02", stock_catalog,
    company_catalog: stock_catalog.map(stock => ({ id: stock.id, code: `C-${stock.code}`, name: stock.code, listed_stock: stock.id })),
    snapshot: { ...snapshot, markets: Object.fromEntries(stock_catalog.map(stock => [stock.id, market()])), daily_candles: Object.fromEntries(stock_catalog.map(stock => [stock.id, []])), active_daily_candles: Object.fromEntries(stock_catalog.map(stock => [stock.id, dailyCandle()])) },
  };
}

test("Tauri实际enqueue按restore同code改id，unknown零invoke，旧确认不得转成新代成功", { timeout: 10000 }, async () => {
  const requests: Record<string, unknown>[] = [];
  let hold = false;
  let complete: (() => void) | undefined;
  await withTauriHost(async host => {
    await assert.rejects(host.submitIntent({ Cancel: { code: "000812", id: 7 } }), /000812/);
    assert.equal(requests.length, 0);
    await host.submitIntent({ PlaceLimit: { code: "600101", side: "Buy", price: { Fixed: "1234" }, qty: 100 } });
    assert.deepEqual(requests[0], { sessionId: "session-1", generation: "1", intent: { PlaceLimit: { code: 0, side: "Buy", price: { Fixed: "1234" }, qty: 100 } } });
    hold = true;
    const old = host.submitIntent({ Cancel: { code: "600101", id: 8 } });
    const rejected = assert.rejects(old, /已过期.*generation/);
    await host.load({});
    assert.ok(complete); complete(); await rejected;
    assert.deepEqual(requests[1], { sessionId: "session-1", generation: "1", intent: { Cancel: { code: 0, id: 8 } } });
    hold = false;
    await host.submitIntent({ PlaceMarket: { code: "600101", side: "Sell", qty: 200 } });
    assert.deepEqual(requests[2], { sessionId: "session-1", generation: "2", intent: { PlaceMarket: { code: 1, side: "Sell", qty: 200 } } });
  }, (command, args) => {
    if (command === "engine_baseline") return actualTauriIntentBaseline("1", ["600101"]);
    if (command === "restore_session") return actualTauriIntentBaseline("2", ["002156", "600101"]);
    if (command === "enqueue") {
      requests.push(args as Record<string, unknown>);
      return hold ? new Promise<void>(resolve => { complete = resolve; }) : null;
    }
    return undefined;
  });
});

test("Tauri已提交restore的新timeline事件被过滤时invoke目录fault仍显式失败并禁后续enqueue", { timeout: 10000 }, async () => {
  const failures: unknown[] = [];
  const enqueues: unknown[] = [];
  const failure = { code: "CATALOG_BASELINE_FAILED", where: "desktop.actor.restore", message: "公开公司身份目录构造失败",
    context: { operation: "restore", generation: "2", fault: { code: 3019, context: [7] } }, recoverable: false, recoveryActions: ["重新同步"] };
  await withTauriHost(async host => {
    await host.start(() => {}, value => { failures.push(value); });
    await emit("engine-failure", { ...failure, session_id: "session-1", timeline_id: "timeline-2", events: [] });
    assert.equal(failures.length, 0, "旧timeline listener不能假接新timeline事件");
    await assert.rejects(host.load({}), error => {
      assert.match(String(error), /CompanyId 7/);
      assert.deepEqual((error as Error).cause, failures[0]);
      return true;
    });
    assert.deepEqual((failures[0] as { context: { fault: unknown } }).context.fault, { code: 3019, context: [7] });
    await assert.rejects(host.submitIntent({ Cancel: { code: "600101", id: 7 } }), /baseline.*失效/);
    assert.equal(enqueues.length, 0);
  }, (command, args) => {
    if (command === "restore_session") return Promise.reject(failure);
    if (command === "enqueue") enqueues.push(args);
    return undefined;
  });
});
test("独立review：restore成功回包缺company_catalog后禁止旧generation enqueue", {timeout:10000}, async () => {
  const enqueues: unknown[] = [];
  await withTauriHost(async host => {
    await assert.rejects(host.load({}), /响应字段无效/);
    await Promise.allSettled([host.submitIntent({Cancel:{code:"600101",id:7}})]);
    assert.equal(enqueues.length, 0, `restore已返回新generation但发送旧请求：${JSON.stringify(enqueues)}`);
  }, (command,args) => {
    if(command === "restore_session") return {snapshot, generation:"2",timeline_id:"new-timeline",civil_date:"2030-01-01",stock_catalog:defaultStockCatalogFixture};
    if(command === "enqueue") enqueues.push(args);
    return undefined;
  });
});

test("独立review：restore结构化目录Err的fault不合法后仍须禁enqueue", {timeout:10000}, async () => {
  const enqueues: unknown[] = [];
  const fault = {code:"CATALOG_BASELINE_FAILED",where:"desktop.actor.restore",message:"目录错误",context:{operation:"restore",fault:{code:3019,context:[]}},recoverable:false};
  await withTauriHost(async host => {
    await assert.rejects(host.load({}), /CompanyId/);
    await Promise.allSettled([host.submitIntent({Cancel:{code:"600101",id:7}})]);
    assert.equal(enqueues.length, 0, `目录Err解析失败后发送旧请求：${JSON.stringify(enqueues)}`);
  }, (command,args) => {
    if(command === "restore_session") return Promise.reject(fault);
    if(command === "enqueue") enqueues.push(args);
    return undefined;
  });
});

test("Tauri restore 提交前 ExistingString 拒绝仍可用旧 authority 发送", { timeout: 10000 }, async () => {
  const enqueues: unknown[] = [];
  await withTauriHost(async host => {
    const visible = host.snapshot();
    await assert.rejects(host.load({}), /候选校验失败/);
    assert.equal(host.snapshot(), visible);
    await host.submitIntent({ Cancel: { code: "600101", id: 7 } });
    assert.deepEqual(enqueues, [{ sessionId: "session-1", generation: "1", intent: { Cancel: { code: 3, id: 7 } } }]);
  }, (command, args) => {
    if (command === "restore_session") return Promise.reject("候选校验失败");
    if (command === "enqueue") enqueues.push(args);
    return undefined;
  });
});

test("Tauri restore 坏头部保留可见 baseline，但拒绝晚到的同代写确认、存档及委托查询", { timeout: 10000 }, async () => {
  let finishEnqueue!: (value: unknown) => void;
  let finishSave!: (value: unknown) => void;
  let finishOrders!: (value: unknown) => void;
  const failures: unknown[] = [];
  await withTauriHost(async host => {
    await host.start(() => {}, failure => { failures.push(failure); });
    const visible = host.snapshot();
    const enqueued = assert.rejects(host.submitIntent({ Cancel: { code: "600101", id: 7 } }), /baseline.*失效/);
    const saved = assert.rejects(host.save(), /baseline.*失效/);
    const orders = assert.rejects(host.playerWorkingOrders(), /baseline.*失效/);
    await assert.rejects(host.load({}), /响应字段无效/);
    assert.equal(host.snapshot(), visible);
    assert.equal(failures.length, 1);
    finishEnqueue(null);
    finishSave({ saved: true });
    finishOrders({ generation: "1", value: [] });
    await Promise.all([enqueued, saved, orders]);
    await assert.rejects(host.start(() => {}), /baseline.*失效/);
  }, command => {
    if (command === "restore_session") return { snapshot, generation: "2", timeline_id: "new", civil_date: "2030-01-01", stock_catalog: defaultStockCatalogFixture };
    if (command === "enqueue") return new Promise(resolve => { finishEnqueue = resolve; });
    if (command === "save_session") return new Promise(resolve => { finishSave = resolve; });
    if (command === "player_working_orders") return new Promise(resolve => { finishOrders = resolve; });
    return undefined;
  });
});
test("独立review r3：pending start确认不得在恢复authority已失效后报告成功",{timeout:10000},async()=>{
 let finishResume!:(value:unknown)=>void;
 await withTauriHost(async host=>{
  const start=host.start(()=>{});
  const refused=assert.rejects(start,/baseline.*失效/);
  await assert.rejects(host.load({}),/响应字段无效/);
  finishResume(null);
  await refused;
 },command=>{
  if(command==="resume_session")return new Promise(resolve=>{finishResume=resolve;});
  if(command==="restore_session")return {snapshot,generation:"2",timeline_id:"new",civil_date:"2030-01-01",stock_catalog:defaultStockCatalogFixture};
  return undefined;
 });
});

test("Tauri pending start 遇普通提交前恢复拒绝仍可确认旧 authority 运行", { timeout: 10000 }, async () => {
  let finishResume!: (value: unknown) => void;
  const enqueues: unknown[] = [];
  await withTauriHost(async host => {
    const starting = host.start(() => {});
    await assert.rejects(host.load({}), /候选校验失败/);
    finishResume(null);
    await starting;
    await host.submitIntent({ Cancel: { code: "600101", id: 7 } });
    assert.deepEqual(enqueues, [{ sessionId: "session-1", generation: "1", intent: { Cancel: { code: 3, id: 7 } } }]);
  }, (command, args) => {
    if (command === "resume_session") return new Promise(resolve => { finishResume = resolve; });
    if (command === "restore_session") return Promise.reject("候选校验失败");
    if (command === "enqueue") enqueues.push(args);
    return undefined;
  });
});
