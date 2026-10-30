import assert from "node:assert/strict";
import test from "node:test";
import { createSessionHostLifecycle, type SessionHostLifecyclePorts } from "./useSessionHostLifecycle.ts";
import { InitialSaveSource, SessionReplacementGate } from "../save/session-replacement.ts";
import { DayEndPersistence } from "../save/day-end-persistence.ts";
import { PlayerOrderRefreshGate } from "../components/player-orders.ts";
import { DEFAULT_SETUP } from "../config/defaults.ts";
import type { EngineHost } from "../host/engine-host.ts";
import type { HostFailure, HostUpdate } from "../host/host-update.ts";
import { WasmHostInitializationError } from "../host/wasm-host-initialization-error.ts";
import { fatalWasmInitializationMessage } from "../host/startup-policy.ts";
import type { StrictSaveEnvelope } from "../save/schema/root.ts";
import { commandHostFixture, commandDayEndArchiveFixture } from "./command-host-test-fixture.ts";
import { AutoOrderManager } from "../components/auto-order-manager.ts";
import { createMemoryHandoverSelection } from "./memory-handover-selection.ts";
import type { MemoryHandoverPanelProps } from "./MemoryHandoverPanel.tsx";
import { parseSaveSlot } from "../save/save-schema.ts";
import { store } from "../store/store.ts";

function fixture(overrides: Partial<SessionHostLifecyclePorts> = {}) {
  const calls: string[] = [];
  let update!: (value: HostUpdate) => void, failure!: (value: HostFailure) => void;
  const host = commandHostFixture({
    start: async (onUpdate: typeof update, onFailure: typeof failure) => { calls.push("start"); update = onUpdate; failure = onFailure; },
    stop: async () => { calls.push("stop"); }, refreshBaseline: async () => {}, dispose: async () => { calls.push("dispose"); },
    setSpeed: async () => { calls.push("speed"); }, setPausePreferences: async () => { calls.push("preferences"); },
    load: async () => { calls.push("load"); }, submitIntent: async () => {},
    day: () => 0,
    initialAllocation: async () => ({ stocks: [] }),
  });
  const source = new InitialSaveSource<StrictSaveEnvelope>();
  const ports: SessionHostLifecyclePorts = {
    hostRef: { current: null }, hostTransitionRef: { current: Promise.resolve({ kind: "released" }) }, initialSaveSourceRef: { current: source }, dayEndPersistenceRef: { current: new DayEndPersistence() },
    autoOrderMgrRef: { current: null }, sessionReplacementGateRef: { current: new SessionReplacementGate() }, saveSelectionGenerationRef: { current: 0 },
    playerOrderRefreshGateRef: { current: new PlayerOrderRefreshGate() },
    hostUpdateRef: { current: () => { calls.push("update"); } }, fatalHostErrorRef: { current: () => { calls.push("failure"); } },
    startupTarget: { kind: "wasm" }, sessionSetup: DEFAULT_SETUP, speed: 1, pauseAfterClose: true, pauseBeforeOpen: false,
    TRADING_E2E_MODE: false, malformedProtocolFixture: () => false,
    createSeed: () => 123456789n,
    setIndicatorCalculator: () => { calls.push("register"); return () => { calls.push("unregister"); }; },
    connectProtocol: () => { calls.push("connect"); }, disconnectProtocol: () => { calls.push("disconnect"); },
    createHost: async () => { calls.push("create"); return host; }, checkWasmEnvironment: () => { calls.push("environment"); },
    isDocumentHidden: () => true, getBrowserSaveRepository: () => ({ load: async () => { calls.push("read"); return null; } }),
    setActiveSetup: () => { calls.push("setup"); }, configureMarketTiming: () => { calls.push("timing"); }, setStartDateDraft: () => {}, setPriceCageEnabledDraft: () => {},
    setFloatAllocationDraft: () => {}, setInitialAllocation: () => {},
    setDeliveryModes: () => {}, setDeliveryModeState: () => {}, setNotice: (value) => calls.push(value), setReady: () => { calls.push("ready"); },
    onSpawnFailure: () => { calls.push("spawn-failure"); },
    setError: (value) => { calls.push(typeof value === "string" ? value : `${value.message} ${value.cause instanceof Error ? value.cause.message : String(value.cause)}`); }, onRunning: (value) => { calls.push(`running:${value}`); }, onAutoTriggered: () => {}, ...overrides,
  };
  return { calls, host, source, ports, runtime: createSessionHostLifecycle(ports), update: () => update, failure: () => failure };
}

test("SessionHostLifecycle 先校验环境和读取，再创建/注册/启动；后台补暂停", async () => {
  const f = fixture(); await f.runtime.start();
  assert.deepEqual(f.calls, ["environment", "read", "create", "setup", "timing", "register", "connect", "speed", "preferences", "start", "stop", "running:true", "ready"]);
  assert.equal(f.ports.hostRef.current, f.host);
  let reads = 0; await f.source.read(async () => { reads++; return null; }); assert.equal(reads, 0);
});
test("新局在同一宿主开跑前取得真实分配，读档不查询初始分配", { timeout: 10000 }, async () => {
  const f = fixture({ setInitialAllocation: (value) => { f.calls.push(value === null ? "clear-allocation" : "show-allocation"); } });
  f.host.initialAllocation = async () => { f.calls.push("query-allocation"); return { stocks: [] }; };
  await f.runtime.start();
  assert.ok(f.calls.indexOf("create") < f.calls.indexOf("query-allocation"));
  assert.ok(f.calls.indexOf("query-allocation") < f.calls.indexOf("show-allocation"));
  assert.ok(f.calls.indexOf("show-allocation") < f.calls.indexOf("start"));
  await f.runtime.dispose();
  const restored = fixture({ startupTarget: { kind: "remote", baseUrl: "http://localhost" } });
  restored.source.select(commandDayEndArchiveFixture());
  restored.host.initialAllocation = async () => { throw new Error("读档不能假称初始分配"); };
  await restored.runtime.start();
  assert.ok(restored.calls.includes("load"));
  assert.ok(restored.calls.includes("ready"));
  await restored.runtime.dispose();
});
test("读档成功通知等待恢复、start 与后台 stop 全部确认并完成启动源", { timeout: 10000 }, async () => {
  let confirmStart!: () => void;
  let confirmStop!: () => void;
  let enteredStart!: () => void;
  let enteredStop!: () => void;
  const startingHost = new Promise<void>((resolve) => { confirmStart = resolve; });
  const stoppingHost = new Promise<void>((resolve) => { confirmStop = resolve; });
  const startEntered = new Promise<void>((resolve) => { enteredStart = resolve; });
  const stopEntered = new Promise<void>((resolve) => { enteredStop = resolve; });
  const f = fixture();
  const archive = commandDayEndArchiveFixture();
  f.source.select(archive);
  f.ports.createHost = async (_setup, _seed, _target, _onSpawnFailure, initialArchive) => {
    assert.equal(initialArchive, archive);
    f.calls.push("create-archive");
    return f.host;
  };
  const complete = f.source.complete.bind(f.source);
  f.source.complete = () => { f.calls.push("complete"); complete(); };
  const runtime = createSessionHostLifecycle(f.ports);
  f.host.day = () => 30;
  f.host.start = async () => { f.calls.push("start-pending"); enteredStart(); await startingHost; f.calls.push("start-confirmed"); };
  f.host.stop = async () => { f.calls.push("stop-pending"); enteredStop(); await stoppingHost; f.calls.push("stop-confirmed"); };

  const starting = runtime.start();
  await startEntered;
  assert.equal(f.calls.some((call) => /已读档/.test(call)), false);
  confirmStart();
  await stopEntered;
  assert.equal(f.calls.some((call) => /已读档/.test(call)), false);
  confirmStop();
  await starting;

  const completeIndex = f.calls.indexOf("complete");
  const successNoticeIndex = f.calls.findIndex((call) => /已读档/.test(call));
  assert.ok(completeIndex >= 0);
  assert.ok(f.calls.indexOf("create-archive") >= 0);
  assert.equal(f.calls.includes("load"), false, "WASM 恢复必须通过 factory initialArchive");
  assert.ok(f.calls.indexOf("start-confirmed") < completeIndex);
  assert.ok(f.calls.indexOf("stop-confirmed") < completeIndex);
  assert.ok(completeIndex < successNoticeIndex);
  assert.equal(f.calls[successNoticeIndex], "已读档（第 31 个交易日）");
  await runtime.dispose();
});

test("读档 start 或后台 stop 失败时不发布成功通知", { timeout: 10000 }, async () => {
  for (const stage of ["start", "stop"] as const) {
    const f = fixture();
    f.source.select(commandDayEndArchiveFixture());
    if (stage === "start") {
      f.host.start = async () => { throw new Error("读档 start 失败"); };
    } else {
      f.host.stop = async () => { throw new Error("后台 stop 失败"); };
    }
    await f.runtime.start();
    assert.equal(f.calls.some((call) => /已读档/.test(call)), false);
    assert.equal(f.calls.includes("ready"), false);
  }
});

test("读档启动取消后忽略 start 晚确认，且新局不发布读档成功通知", { timeout: 10000 }, async () => {
  let confirmStart!: () => void;
  let enterStart!: () => void;
  const startEntered = new Promise<void>((resolve) => { enterStart = resolve; });
  const pendingStart = new Promise<void>((resolve) => { confirmStart = resolve; });
  const restored = fixture();
  restored.source.select(commandDayEndArchiveFixture());
  restored.host.start = async () => { restored.calls.push("start-pending"); enterStart(); await pendingStart; };
  const starting = restored.runtime.start();
  await startEntered;
  await restored.runtime.dispose();
  confirmStart();
  await starting;
  assert.equal(restored.calls.some((call) => /已读档/.test(call)), false);

  const fresh = fixture();
  await fresh.runtime.start();
  assert.equal(fresh.calls.some((call) => /已读档/.test(call)), false);
  await fresh.runtime.dispose();
});

test("取消后的初始分配晚响应不写 UI、不启动旧宿主", { timeout: 10000 }, async () => {
  let queried!: () => void;
  let resolve!: (value: { stocks: [] }) => void;
  const entered = new Promise<void>((done) => { queried = done; });
  const f = fixture({ setInitialAllocation: (value) => { if (value !== null) f.calls.push("show-allocation"); } });
  f.host.initialAllocation = async () => { queried(); return new Promise((done) => { resolve = done; }); };
  const starting = f.runtime.start();
  await entered;
  await f.runtime.dispose();
  resolve({ stocks: [] });
  await starting;
  assert.equal(f.calls.includes("show-allocation"), false);
  assert.equal(f.calls.includes("start"), false);
});
test("宿主创建期间进入后台，就绪必须等待暂停确认后再发布 ready", { timeout: 10000 }, async () => {
  let hidden = false;
  let created!: (host: EngineHost) => void;
  let enterCreate!: () => void;
  let confirmStop!: () => void;
  let enterStop!: () => void;
  const creating = new Promise<void>((resolve) => { enterCreate = resolve; });
  const stopping = new Promise<void>((resolve) => { enterStop = resolve; });
  const f = fixture({
    isDocumentHidden: () => hidden,
    createHost: () => new Promise((resolve) => { created = resolve; enterCreate(); }),
  });
  f.host.stop = async () => {
    f.calls.push("stop-pending");
    enterStop();
    await new Promise<void>((resolve) => { confirmStop = resolve; });
    f.calls.push("stop-confirmed");
  };
  const starting = f.runtime.start();
  await creating;
  hidden = true;
  created(f.host);
  await stopping;
  assert.ok(f.calls.indexOf("start") < f.calls.indexOf("stop-pending"));
  assert.equal(f.calls.includes("ready"), false);
  confirmStop();
  await starting;
  assert.ok(f.calls.indexOf("stop-confirmed") < f.calls.indexOf("ready"));
  await f.runtime.dispose();
});
test("停止共享会话先失效令牌再 stop，释放 registration/coordinator/manager/host；晚到消息丢弃", async () => {
  const f = fixture(); await f.runtime.start(); f.calls.length = 0;
  const replacement = f.ports.sessionReplacementGateRef.current.begin()!;
  const refresh = f.ports.playerOrderRefreshGateRef.current.next();
  f.ports.autoOrderMgrRef.current = new AutoOrderManager(async () => {});
  f.ports.autoOrderMgrRef.current.clear = () => { f.calls.push("manager-clear"); };
  await f.runtime.stopCurrentSession();
  assert.deepEqual(f.calls, ["stop", "unregister", "disconnect", "manager-clear", "dispose"]);
  assert.equal(f.ports.hostRef.current, null); assert.equal(f.ports.autoOrderMgrRef.current, null);
  assert.equal(f.ports.sessionReplacementGateRef.current.isCurrent(replacement), false);
  assert.equal(f.ports.playerOrderRefreshGateRef.current.isCurrent(refresh), false); assert.equal(f.ports.saveSelectionGenerationRef.current, 1);
  f.update()({} as HostUpdate); f.failure()({ code: "WORKER_FATAL", message: "旧错误" } as HostFailure);
  assert.equal(f.calls.includes("update"), false); assert.equal(f.calls.includes("failure"), false);
});
test("StrictMode cleanup 后创建完成的宿主只 dispose，不向 Shell 写入 ready/host", async () => {
  let resolve!: (host: EngineHost) => void;
  let entered!: () => void;
  const creating = new Promise<void>((done) => { entered = done; });
  const f = fixture({ createHost: () => new Promise((done) => { resolve = done; entered(); }) });
  const starting = f.runtime.start(); await creating;
  await f.runtime.dispose(); resolve(f.host); await starting;
  assert.equal(f.ports.hostRef.current, null); assert.equal(f.calls.includes("start"), false); assert.equal(f.calls.includes("ready"), false);
  assert.equal(f.calls.filter((value) => value === "dispose").length, 1);
});
test("启动档 setup/seed 优先于默认配置，load 完成后安装时间配置才接线和 start", async () => {
  const archive = { ...commandDayEndArchiveFixture(), setup: { ...DEFAULT_SETUP, start_date: "2031-02-03", ticks_per_day: 30, auction_ticks: 9, closing_auction_ticks: 3 }, seed: "9007199254740993" };
  let seenSeed: bigint | null = null;
  const f = fixture({ startupTarget: { kind: "remote", baseUrl: "http://localhost" } }); f.source.select(archive);
  f.ports.createHost = async (setup, seed) => { assert.equal(setup, archive.setup); seenSeed = seed; return f.host; };
  f.ports.configureMarketTiming = (setup) => { assert.equal(setup, archive.setup); f.calls.push("timing"); };
  const runtime = createSessionHostLifecycle(f.ports); await runtime.start();
  assert.equal(seenSeed, 9007199254740993n); assert.ok(f.calls.indexOf("load") < f.calls.indexOf("connect"));
  assert.ok(f.calls.indexOf("load") < f.calls.indexOf("timing"));
  assert.ok(f.calls.indexOf("timing") < f.calls.indexOf("connect"));
});
test("初始化失败释放已创建资源并显式报告当前宿主错误", async () => {
  const f = fixture(); f.host.setPausePreferences = async () => { throw new Error("偏好 IPC 失败"); };
  await f.runtime.start(); assert.equal(f.ports.hostRef.current, null);
  assert.equal(f.calls.includes("dispose"), true); assert.equal(f.calls.includes("ready"), false);
  assert.ok(f.calls.some((value) => /偏好 IPC 失败/.test(value)));
});

test("初始化 load 或偏好确认尚未完成时 cleanup，晚到确认不能重新注册或启动宿主", async () => {
  for (const stage of ["load", "preferences"] as const) {
    let enter!: () => void, complete!: () => void;
    const entered = new Promise<void>((resolve) => { enter = resolve; });
    const pending = new Promise<void>((resolve) => { complete = resolve; });
    const f = fixture({ startupTarget: { kind: "remote", baseUrl: "http://localhost" } });
    const archive = commandDayEndArchiveFixture();
    if (stage === "load") {
      f.source.select(archive);
      f.host.load = async (slot) => { assert.equal(slot, archive); f.calls.push("load-pending"); enter(); await pending; };
    } else {
      f.host.setPausePreferences = async () => { f.calls.push("preferences-pending"); enter(); await pending; };
    }

    const starting = f.runtime.start();
    await entered;
    await f.runtime.dispose();
    const callsAfterCleanup = [...f.calls];
    complete();
    await starting;

    assert.deepEqual(f.calls, callsAfterCleanup);
    assert.equal(f.ports.hostRef.current, null);
    assert.equal(f.ports.autoOrderMgrRef.current, null);
    assert.equal(f.calls.filter((value) => value === "dispose").length, 1);
    assert.equal(f.calls.includes("start"), false);
    assert.equal(f.calls.includes("ready"), false);
    if (stage === "load") {
      assert.equal(f.calls.includes("register"), false);
      assert.equal(await f.source.read(async () => { throw new Error("已选择的启动档不应重新读取"); }), archive);
    } else {
      assert.equal(f.calls.filter((value) => value === "unregister").length, 1);
    }
  }
});

test("初始存档恢复失败显式释放宿主并保留启动源，重试仍恢复同一日终档", async () => {
  const f = fixture({ startupTarget: { kind: "remote", baseUrl: "http://localhost" } });
  const archive = commandDayEndArchiveFixture();
  f.source.select(archive);
  f.host.load = async (slot) => { assert.equal(slot, archive); throw new Error("初始日终存档恢复失败"); };

  await f.runtime.start();

  assert.equal(f.ports.hostRef.current, null);
  assert.equal(f.calls.filter((value) => value === "dispose").length, 1);
  assert.equal(f.calls.includes("register"), false);
  assert.equal(f.calls.includes("ready"), false);
  assert.ok(f.calls.some((value) => /初始日终存档恢复失败/.test(value)));

  const retryHost = commandHostFixture({ ...f.host,
    load: async (slot) => { assert.equal(slot, archive); f.calls.push("load-retried"); },
  });
  f.ports.createHost = async () => retryHost;
  f.calls.length = 0;
  await createSessionHostLifecycle(f.ports).start();
  assert.equal(f.ports.hostRef.current, retryHost);
  assert.equal(f.calls.filter((value) => value === "load-retried").length, 1);
  assert.equal(f.calls.filter((value) => value === "register").length, 1);
  assert.ok(f.calls.indexOf("load-retried") < f.calls.indexOf("register"));
  assert.equal(f.calls.includes("ready"), true);
});

test("stop 抛错仍释放 indicator、协议与宿主资源并保留原错误", async () => {
  const f = fixture();
  await f.runtime.start();
  const stopError = new Error("宿主 stop 失败");
  f.host.stop = async () => { f.calls.push("stop-failed"); throw stopError; };
  f.calls.length = 0;

  await assert.rejects(f.runtime.stopCurrentSession(), (error) => error === stopError);

  assert.deepEqual(f.calls, ["stop-failed", "unregister", "disconnect", "dispose"]);
  assert.equal(f.ports.hostRef.current, null);
  assert.equal(f.ports.autoOrderMgrRef.current, null);
});

test("G20：无存档新局使用熵端口，E2E固定seed与读档seed不消耗熵", { timeout: 10000 }, async () => {
  for (const mode of ["new", "e2e", "save"] as const) {
    let seeds = 0; let seen: bigint | null = null;
    const f = fixture({ createSeed: () => { seeds++; return 9007199254740999n; }, TRADING_E2E_MODE: mode === "e2e" });
    if (mode === "save") { f.ports.startupTarget = { kind: "remote", baseUrl: "http://localhost" }; f.source.select({ ...commandDayEndArchiveFixture(), seed: "9007199254740993" }); }
    f.ports.createHost = async (_setup, seed) => { seen = seed; return f.host; };
    if (mode === "e2e") {
      Object.assign(f.host, { stepOnceForE2E: async () => 0 });
      f.host.tick = () => 0;
      Object.assign(globalThis, { window: {} });
    }
    await createSessionHostLifecycle(f.ports).start();
    assert.equal(seeds, mode === "new" ? 1 : 0);
    assert.equal(seen, mode === "new" ? 9007199254740999n : mode === "save" ? 9007199254740993n : 42n);
  }
});

test("E2E snapshot bridge读取Redux当前快照，不读取宿主缓存快照", { timeout: 10_000 }, async () => {
  const f = fixture({ TRADING_E2E_MODE: true });
  Object.assign(f.host, { stepOnceForE2E: async () => 0 });
  f.host.tick = () => 0;
  const previousWindow = (globalThis as typeof globalThis & { window?: unknown }).window;
  Object.assign(globalThis, { window: {} });
  const getState = store.getState.bind(store);
  try {
    await f.runtime.start();
    const live = { ...f.host.snapshot(), seq: 9, tick: 4, day: 2 };
    store.getState = (() => ({ ...getState(), snapshot: { ...getState().snapshot, snapshot: live } })) as typeof store.getState;
    const controls = (globalThis as typeof globalThis & { window: { __STOCK_GAME_E2E__?: { snapshot(): unknown } } }).window.__STOCK_GAME_E2E__;
    assert.ok(controls);
    assert.equal(controls.snapshot(), live);
    assert.notEqual(controls.snapshot(), f.host.snapshot());
    store.getState = (() => ({ ...getState(), snapshot: { ...getState().snapshot, snapshot: null } })) as typeof store.getState;
    assert.throws(() => controls.snapshot(), /E2E 实时快照尚未由当前会话交付/);
  } finally {
    try { await f.runtime.dispose(); }
    finally {
      store.getState = getState;
      if (previousWindow === undefined) Reflect.deleteProperty(globalThis, "window");
      else Object.assign(globalThis, { window: previousWindow });
    }
  }
});

test("G40：speed/start异步确认前不得ready，dispose失败不能消失", { timeout: 10000 }, async () => {
  let confirm!: () => void; let entered!: () => void;
  const waiting = new Promise<void>((resolve) => { entered = resolve; });
  const f = fixture();
  f.host.setSpeed = async () => { entered(); await new Promise<void>((resolve) => { confirm = resolve; }); };
  const starting = f.runtime.start(); await waiting;
  assert.equal(f.calls.includes("start"), false); assert.equal(f.calls.includes("ready"), false);
  confirm(); await starting;
  f.host.dispose = async () => { throw new Error("释放 IPC 失败"); };
  await assert.rejects(f.runtime.dispose(), /释放 IPC 失败/);
});

test("G20：0 seed无损进入host，熵失败显错且不创建或ready", { timeout: 10000 }, async () => {
  const zero = fixture({ createSeed: () => 0n });
  let seen: bigint | null = null;
  zero.ports.createHost = async (_setup, seed) => { seen = seed; return zero.host; };
  await createSessionHostLifecycle(zero.ports).start();
  assert.equal(seen, 0n); assert.equal(zero.calls.includes("ready"), true);
  const failed = fixture({ createSeed: () => { throw new Error("crypto 熵获取被拒绝"); } });
  await failed.runtime.start();
  assert.equal(failed.calls.includes("create"), false); assert.equal(failed.calls.includes("ready"), false);
  assert.ok(failed.calls.some((call) => call.includes("crypto 熵获取被拒绝")));
});

test("宿主callback同步传回协议失败，不将false变成void或继续消费旧会话", { timeout: 10000 }, async () => {
  const f = fixture({ hostUpdateRef: { current: () => false } });
  await f.runtime.start();
  assert.equal(f.update()({} as HostUpdate), false);
  await f.runtime.dispose();
  assert.equal(f.update()({} as HostUpdate), false);
});


for (const stage of ["construction", "running", "paused", "stop"] as const) {
  test(`spawn failure 在 ${stage} 立即交付原始结构且不走 fatal 或修改运行状态`, { timeout: 10000 }, async () => {
    const reports: HostFailure[] = [];
    const original: HostFailure = { code: "WORKER_SPAWN_FAILED", where: "wasm.spawn", message: stage,
      cause: { name: "Error", message: "资源不足" }, context: { requestId: 17, phase: "Continuous" } };
    let emit!: (value: HostFailure) => void;
    const f = fixture({ isDocumentHidden: () => false, onSpawnFailure: (value) => reports.push(value) });
    f.ports.createHost = async (...args: unknown[]) => {
      assert.equal(typeof args[3], "function", "构造前必须安装 spawn failure sink");
      emit = args[3] as typeof emit;
      if (stage === "construction") {
        assert.equal(f.ports.hostRef.current, null);
        emit(original);
        assert.equal(reports[0], original, "createHost resolve 前立即交付");
      }
      return f.host;
    };
    const runtime = createSessionHostLifecycle(f.ports);
    await runtime.start();
    assert.equal(typeof emit, "function", "启动必须将 sink 交给 createHost");
    f.calls.length = 0;
    if (stage === "paused") await f.host.stop();
    if (stage === "stop") {
      f.host.stop = async () => { emit(original); assert.equal(reports[0], original); };
      await f.host.stop();
    } else if (stage !== "construction") emit(original);
    assert.equal(reports.length, 1);
    assert.equal(reports[0], original);
    assert.equal(reports[0].cause, original.cause);
    assert.equal(reports[0].context, original.context);
    assert.equal(f.calls.includes("failure"), false);
    assert.equal(f.calls.some((value) => value.startsWith("running:")), false);
    await runtime.dispose();
    emit({ ...original, message: "旧生命周期晚到 failure" });
    assert.equal(reports.length, 1, "cancel 后旧 sink 不得污染当前报告");
  });
}


test("构造尚未 resolve 就可报告；取消旧 sink 后新 lifecycle 追加且保留未确认详情", { timeout: 10000 }, async () => {
  const reports: HostFailure[] = [];
  const original: HostFailure = { code: "WORKER_SPAWN_FAILED", where: "wasm.spawn", message: "构造期失败", cause: { message: "原cause" }, context: { requestId: 1 } };
  let oldSink!: (failure: HostFailure) => void;
  let complete!: (host: EngineHost) => void;
  let entered!: () => void;
  const creating = new Promise<void>((resolve) => { entered = resolve; });
  const f = fixture({ onSpawnFailure: (failure) => reports.push(failure), createHost: async (_setup, _seed, _target, sink) => {
    oldSink = sink;
    entered();
    return new Promise((resolve) => { complete = resolve; });
  } });
  const starting = f.runtime.start();
  await creating;
  oldSink(original);
  assert.deepEqual(reports, [original]);
  assert.equal(f.ports.hostRef.current, null);
  await f.runtime.stopCurrentSession();
  f.ports.createHost = async (_setup, _seed, _target, sink) => {
    oldSink({ ...original, message: "旧错误不得追加" });
    sink({ ...original, message: "新生命周期失败" });
    return commandHostFixture({ ...f.host });
  };
  const next = createSessionHostLifecycle(f.ports);
  const nextStarting = next.start();
  await settleTasks();
  assert.equal(reports.length, 1, "旧create未完成前不能启动下一组");
  complete(f.host);
  await Promise.all([starting, nextStarting]);
  assert.equal(reports.length, 2);
  assert.equal(reports[0], original);
  assert.equal(reports[1].message, "新生命周期失败");
  await next.dispose();
});


test("WASM初始化结构化错误保留HostFailure，普通String和Error沿用阶段文本", { timeout: 10000 }, async () => {
  const failure: HostFailure = {
    code: "WASM_RESTORE_PREFIX", where: "facade-engine-initialize.restore-prefix",
    message: "日终存档前缀恢复失败", cause: { code: 1037, phase: "prefix" },
    context: { operation: "restore", stockCount: 12 }, recoverable: true, recoveryActions: ["保留存档"],
  };
  let received: string | HostFailure | null = null;
  const f = fixture({ setError: (value) => { received = value; }, createHost: async (_setup, _seed, _target, onSpawnFailure) => {
    onSpawnFailure(failure);
    throw new WasmHostInitializationError(failure);
  } });
  await f.runtime.start();
  assert.equal(received, failure, "结构化初始化错误应以同一HostFailure交付给setError");
  assert.ok(f.calls.includes("spawn-failure"), "createHost构造期spawn sink必须保留");
  assert.equal(f.calls.includes("ready"), false, "初始化失败不能发布ready");
  assert.equal(f.ports.hostRef.current, null, "初始化失败不能安装host");

  for (const ordinary of ["WASM wrapper 原始字符串", new Error("WASM wrapper 普通错误")]) {
    let ordinaryReceived: string | HostFailure | null = null;
    const plain = fixture({ setError: (value) => { ordinaryReceived = value; }, createHost: async () => { throw ordinary; } });
    await plain.runtime.start();
    assert.equal(ordinaryReceived, fatalWasmInitializationMessage(ordinary));
  }
});

test("WASM 启动档作为同一第五参数交给 createHost，构造前不调用 load", { timeout: 10000 }, async () => {
  const archive = commandDayEndArchiveFixture();
  const f = fixture();
  f.source.select(archive);
  f.ports.createHost = async (...args: unknown[]) => {
    assert.equal(args.length, 5);
    assert.equal(args[3] instanceof Function, true, "第四参数仍是 spawn failure sink");
    assert.equal(args[4], archive, "完整原档作为第五参数传递");
    return f.host;
  };
  await createSessionHostLifecycle(f.ports).start();
  assert.equal(f.calls.includes("load"), false, "WASM 不能先空建宿主再恢复");
  assert.equal(f.calls.includes("ready"), true);
});

function deferred<Value>() {
  let resolve!: (value: Value) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<Value>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}
async function settleTasks() { for (let index = 0; index < 8; index++) await Promise.resolve(); }

test("共享transition：下一create等待旧异步create和晚到host释放均完成", async () => {
  const created = deferred<EngineHost>(), creating = deferred<void>();
  const disposed = deferred<void>(), disposing = deferred<void>();
  const f = fixture({ createHost: async () => { creating.resolve(); return created.promise; } });
  f.host.dispose = async () => { disposing.resolve(); await disposed.promise; };
  const oldStarting = f.runtime.start(); await creating.promise;
  await f.runtime.dispose();
  let nextCreates = 0;
  const nextHost = commandHostFixture({ ...f.host, dispose: async () => {} });
  const next = createSessionHostLifecycle({ ...f.ports, createHost: async () => { nextCreates++; return nextHost; } });
  const nextStarting = next.start(); await settleTasks();
  assert.equal(nextCreates, 0, "旧create未定时禁止第二组");
  created.resolve(f.host); await disposing.promise; await settleTasks();
  assert.equal(nextCreates, 0, "旧晚到host释放未定时禁止第二组");
  disposed.resolve(); await Promise.all([oldStarting, nextStarting]);
  assert.equal(nextCreates, 1); assert.equal(f.ports.hostRef.current, nextHost);
  await next.dispose();
});

test("共享transition：旧已就绪host释放确认前新create为0", async () => {
  const f = fixture(); await f.runtime.start();
  const disposed = deferred<void>(), disposing = deferred<void>(); let nextCreates = 0;
  f.host.dispose = async () => { disposing.resolve(); await disposed.promise; };
  const closing = f.runtime.dispose(); await disposing.promise;
  const next = createSessionHostLifecycle({ ...f.ports, createHost: async () => { nextCreates++; return commandHostFixture(); } });
  const nextStarting = next.start(); await settleTasks(); assert.equal(nextCreates, 0);
  disposed.resolve(); await Promise.all([closing, nextStarting]); assert.equal(nextCreates, 1);
  await next.dispose();
});

test("共享transition：释放失败显式阻止下一create并保原error事实", async () => {
  const f = fixture(); await f.runtime.start();
  const cause = new Error("旧实例释放未获确认"); let nextCreates = 0;
  f.host.dispose = async () => { throw cause; };
  await assert.rejects(f.runtime.dispose(), (error) => error === cause);
  const next = createSessionHostLifecycle({ ...f.ports, createHost: async () => { nextCreates++; return commandHostFixture(); } });
  await next.start(); assert.equal(nextCreates, 0);
  const outcome = await f.ports.hostTransitionRef.current;
  assert.equal(outcome.kind, "failed"); if (outcome.kind === "failed") assert.equal(outcome.error, cause);
  assert.ok(f.calls.some((value) => value.includes(cause.message)));
});

test("共享transition：同步dispose抛错同样锁存，下一UI保原cause", async () => {
  const f = fixture(); await f.runtime.start();
  const cause = new Error("dispose同步失败"); f.host.dispose = () => { throw cause; };
  await assert.rejects(f.runtime.dispose(), (error) => error === cause);
  let failure: string | HostFailure | null = null; let creates = 0;
  await createSessionHostLifecycle({ ...f.ports, setError: (value) => { failure = value; },
    createHost: async () => { creates++; return commandHostFixture(); } }).start();
  assert.equal(creates, 0); assert.ok(failure !== null && typeof failure === "object");
  assert.equal((failure as HostFailure).cause, cause);
});

test("共享transition：尚未取得前序资源owner的排队cleanup不清当前协议或manager", async () => {
  const f = fixture(); await f.runtime.start(); const before = [...f.calls];
  const selection = f.ports.saveSelectionGenerationRef.current;
  const epoch = f.ports.sessionReplacementGateRef.current.begin()!;
  const queued = createSessionHostLifecycle(f.ports); const starting = queued.start();
  await queued.dispose(); assert.deepEqual(f.calls, before); assert.equal(f.ports.hostRef.current, f.host);
  assert.equal(f.ports.saveSelectionGenerationRef.current, selection);
  assert.equal(f.ports.sessionReplacementGateRef.current.isCurrent(epoch), true);
  await f.runtime.dispose(); await starting;
});

test("共享transition：旧查询挂起但host释放成功后不阻塞下一create", async () => {
  const queried = deferred<void>(), allocation = deferred<{ stocks: [] }>();
  const f = fixture();
  f.host.initialAllocation = async () => { queried.resolve(); return allocation.promise; };
  const oldStarting = f.runtime.start(); await queried.promise; await f.runtime.dispose();
  let nextCreates = 0;
  const next = createSessionHostLifecycle({ ...f.ports, createHost: async () => { nextCreates++; return commandHostFixture(); } });
  await next.start(); assert.equal(nextCreates, 1, "不等待已销毁host的旧查询");
  const current = f.ports.hostRef.current;
  allocation.resolve({ stocks: [] }); await oldStarting;
  assert.equal(f.ports.hostRef.current, current); await next.dispose();
});

test("共享transition：排队effect取消不创建，后继仍等待最旧释放", async () => {
  const f = fixture(); await f.runtime.start();
  const disposed = deferred<void>(); f.host.dispose = () => disposed.promise;
  const closing = f.runtime.dispose(); let cancelledCreates = 0; let finalCreates = 0;
  const cancelled = createSessionHostLifecycle({ ...f.ports, createHost: async () => { cancelledCreates++; return commandHostFixture(); } });
  const cancelledStarting = cancelled.start(); await cancelled.dispose();
  const final = createSessionHostLifecycle({ ...f.ports, createHost: async () => { finalCreates++; return commandHostFixture(); } });
  const finalStarting = final.start(); await settleTasks(); assert.equal(finalCreates, 0);
  disposed.resolve(); await Promise.all([closing, cancelledStarting, finalStarting]);
  assert.equal(cancelledCreates, 0); assert.equal(finalCreates, 1); await final.dispose();
});

test("共享transition：未start的effect不占位，旧dispose重复不触及新资源", async () => {
  const f = fixture(); const before = f.ports.hostTransitionRef.current;
  await createSessionHostLifecycle(f.ports).dispose(); assert.equal(f.ports.hostTransitionRef.current, before);
  await f.runtime.start(); await f.runtime.dispose();
  const next = createSessionHostLifecycle(f.ports); await next.start(); const calls = [...f.calls];
  await f.runtime.dispose(); assert.deepEqual(f.calls, calls); assert.equal(f.ports.hostRef.current, f.host);
  await next.dispose();
});

test("共享transition：重复start拒绝且不重新占尾，stop抛undefined也显式失败", async () => {
  const f = fixture(); await f.runtime.start(); const tail = f.ports.hostTransitionRef.current;
  await assert.rejects(f.runtime.start(), /只允许 start 一次/); assert.equal(f.ports.hostTransitionRef.current, tail);
  f.host.stop = async () => { throw undefined; };
  await assert.rejects(f.runtime.stopCurrentSession(), (error) => error === undefined);
  const outcome = await tail;
  assert.equal(outcome.kind, "failed"); if (outcome.kind === "failed") assert.equal(outcome.error, undefined);
});

test("共享transition：取消后create拒绝无late host，后继可继续且不丢诊断", async () => {
  const created = deferred<EngineHost>(), entered = deferred<void>();
  const f = fixture({ createHost: async () => { entered.resolve(); return created.promise; } });
  const oldStarting = f.runtime.start(); await entered.promise; await f.runtime.dispose();
  let creates = 0;
  const next = createSessionHostLifecycle({ ...f.ports, createHost: async () => { creates++; return commandHostFixture(); } });
  const nextStarting = next.start(); await settleTasks(); assert.equal(creates, 0);
  created.reject(new Error("旧create失败，已清资源")); await Promise.all([oldStarting, nextStarting]);
  assert.equal(creates, 1); assert.ok(f.calls.some((value) => value.includes("旧create失败，已清资源")));
  await next.dispose();
});


import { WasmDocumentMemoryOwner } from "../host/wasm-document-memory-owner.ts";
import type { WasmFinalRelease } from "../host/wasm-document-memory-owner.ts";
import { WasmMemoryStorage } from "../host/wasm-memory-storage.ts";
import { fixture as memoryFixture, MiB, SNAPSHOT_KEY } from "../host/wasm-memory-storage.test-support.ts";
function memoryOwnerFixture() {
  const memory = memoryFixture();
  const owner = new WasmDocumentMemoryOwner({ maximumBytes: memory.options.maximumBytes, reclaimBytes: memory.options.reclaimBytes,
    readCapacity: () => ({ kind: "enforced-capacity", initialPages: 1024, maximumPages: 4096, liveUpperBoundBytes: 256 * MiB }),
    restoreCapabilities: { forArchive: () => ({ kind: "verified-package", wasmSha256: "a".repeat(64), glueSha256: "b".repeat(64), sourceSha256: "c".repeat(64), buildManifestSha256: "d".repeat(64), exports: ["restore_json", "facade_slot_init_for_restore_json"] }) },
    createStorage: (liveUpperBoundBytes) => new WasmMemoryStorage({ ...memory.options, liveUpperBoundBytes }),
    randomNonce: () => "1234567890abcdef1234567890abcdef" });
  return { memory, owner };
}

test("document owner真实release回执晚于dispose时，下一effect不能分配且drop只计一次", { timeout: 10000 }, async () => {
  const m = memoryOwnerFixture(), release = deferred<WasmFinalRelease>(); let creates = 0;
  const f = fixture({ wasmMemoryOwner: m.owner, hostTransitionRef: m.owner.hostTransitionRef });
  f.host.tick = () => 4; f.host.day = () => 1;
  f.ports.createHost = async (_setup, _seed, _target, _sink, _archive, ticket) => {
    creates++; assert.ok(ticket); ticket.registerRelease(release.promise); return f.host;
  };
  const first = createSessionHostLifecycle(f.ports); await first.start();
  const closing = first.dispose(); await settleTasks();
  const next = createSessionHostLifecycle({ ...f.ports, createHost: async (_setup, _seed, _target, _sink, _archive, ticket) => {
    creates++; ticket!.registerRelease(Promise.resolve({ kind: "not-allocated" })); return commandHostFixture({ ...f.host, tick: () => 0, day: () => 0 });
  } });
  const starting = next.start(); await settleTasks(); assert.equal(creates, 1);
  release.resolve({ kind: "released", finalBytes: 64 * MiB, growthStopped: true, referencesDropped: true });
  await Promise.all([closing, starting]); assert.equal(creates, 2);
  await first.dispose();
  const snapshot = JSON.parse(m.memory.session.data.get(SNAPSHOT_KEY)!);
  assert.equal(snapshot.sessionResident, 64 * MiB); assert.equal(snapshot.liveUpperBound, 256 * MiB);
  await next.dispose();
});

test("pending完整X先获用户确认再传同一目标，只restore不读quickslot或空new+load", { timeout: 10000 }, async () => {
  const m = memoryOwnerFixture(), archive = commandDayEndArchiveFixture();
  for (let index = 0; index < 2; index++) {
    const admitted = m.owner.admit({ kind: "new", setup: DEFAULT_SETUP, seed: "42" });
    assert.equal(admitted.kind, "admitted"); if (admitted.kind !== "admitted") throw new Error("预期admitted");
    m.owner.recordCommittedBoundary(admitted.ticket, { tick: 6, day: 2 });
    admitted.ticket.registerRelease(Promise.resolve({ kind: "released", finalBytes: 256 * MiB, growthStopped: true, referencesDropped: true }));
    await m.owner.finish(admitted.ticket);
  }
  assert.equal(m.owner.admit({ kind: "load", archive }).kind, "reload");
  const nextOwner = new WasmDocumentMemoryOwner({ maximumBytes: m.memory.options.maximumBytes, reclaimBytes: m.memory.options.reclaimBytes,
    readCapacity: () => ({ kind: "enforced-capacity", initialPages: 1024, maximumPages: 4096, liveUpperBoundBytes: 256 * MiB }),
    restoreCapabilities: { forArchive: () => ({ kind: "verified-package", wasmSha256: "a".repeat(64), glueSha256: "b".repeat(64), sourceSha256: "c".repeat(64), buildManifestSha256: "d".repeat(64), exports: ["restore_json", "facade_slot_init_for_restore_json"] }) },
    createStorage: (liveUpperBoundBytes) => new WasmMemoryStorage({ ...m.memory.options, liveUpperBoundBytes }) });
  const confirmation = deferred<void>(); let confirms = 0, creates = 0;
  const f = fixture({ wasmMemoryOwner: nextOwner, hostTransitionRef: nextOwner.hostTransitionRef,
    confirmMemoryHandover: async (handover) => {
      confirms++; await confirmation.promise;
      assert.equal(handover.kind, "frozen"); return handover.kind === "frozen" ? handover.pendingIntent : null;
    }, getBrowserSaveRepository: () => { throw new Error("pending确认不能重读quickslot"); } });
  f.host.tick = () => 6; f.host.day = () => 2;
  f.ports.createHost = async (_setup, seed, _target, sink, x, ticket) => {
    creates++; assert.equal(seed, BigInt(archive.seed)); assert.equal(typeof sink, "function");
    assert.deepEqual(x, archive); assert.ok(ticket);
    assert.equal(JSON.parse(m.memory.session.data.get(SNAPSHOT_KEY)!).handover, undefined);
    ticket.registerRelease(Promise.resolve({ kind: "not-allocated" })); return f.host;
  };
  const current = createSessionHostLifecycle(f.ports), starting = current.start(); await settleTasks();
  assert.equal(confirms, 1); assert.equal(creates, 0);
  confirmation.resolve(); await starting;
  assert.equal(creates, 1); assert.equal(f.calls.includes("load"), false); assert.equal(f.calls.includes("ready"), true);
  await current.dispose();
});

test("reload待确认X时用户明确改选Y，先确认Y再单次消费；不重读快速槽或复活X", { timeout: 10000 }, async () => {
  const m = memoryOwnerFixture(), archive = commandDayEndArchiveFixture();
  for (let index = 0; index < 2; index++) {
    const result = m.owner.admit({ kind: "new", setup: DEFAULT_SETUP, seed: "42" });
    if (result.kind !== "admitted") throw new Error("预期admitted");
    m.owner.recordCommittedBoundary(result.ticket, { tick: 6, day: 2 });
    result.ticket.registerRelease(Promise.resolve({ kind: "released", finalBytes: 256 * MiB, growthStopped: true, referencesDropped: true }));
    await m.owner.finish(result.ticket);
  }
  m.owner.admit({ kind: "load", archive });
  const next = new WasmDocumentMemoryOwner({ maximumBytes: m.memory.options.maximumBytes, reclaimBytes: m.memory.options.reclaimBytes,
    readCapacity: () => ({ kind: "enforced-capacity", initialPages: 1024, maximumPages: 4096, liveUpperBoundBytes: 256 * MiB }),
    restoreCapabilities: { forArchive: () => ({ kind: "verified-package", wasmSha256: "a".repeat(64), glueSha256: "b".repeat(64), sourceSha256: "c".repeat(64), buildManifestSha256: "d".repeat(64), exports: ["restore_json", "facade_slot_init_for_restore_json"] }) },
    createStorage: (liveUpperBoundBytes) => new WasmMemoryStorage({ ...m.memory.options, liveUpperBoundBytes }) });
  const y = { ...archive, seed: "99" }; let confirmed = 0, created = 0;
  const f = fixture({ wasmMemoryOwner: next, hostTransitionRef: next.hostTransitionRef,
    confirmMemoryHandover: async (old, explicit) => {
      confirmed++; assert.equal(old.kind, "frozen");
      assert.equal(explicit?.kind, "load"); if (explicit?.kind === "load") assert.equal(explicit.archive, y);
      assert.equal(JSON.parse(m.memory.session.data.get(SNAPSHOT_KEY)!).handover.pendingIntent.archive.seed, archive.seed);
      return explicit!;
    }, getBrowserSaveRepository: () => { throw new Error("不得重读快速槽"); } });
  f.source.select(y); f.host.tick = () => 6; f.host.day = () => 2;
  f.ports.createHost = async (_setup, seed, _target, _sink, selected, ticket) => {
    created++; assert.equal(seed, 99n); assert.deepEqual(selected, y);
    assert.equal(JSON.parse(m.memory.session.data.get(SNAPSHOT_KEY)!).handover, undefined);
    ticket!.registerRelease(Promise.resolve({ kind: "not-allocated" })); return f.host;
  };
  const current = createSessionHostLifecycle(f.ports); await current.start();
  assert.equal(confirmed, 1); assert.equal(created, 1); assert.equal(f.calls.includes("load"), false);
  await current.dispose();
});

test("面板实际解析文件Y后才确认恢复，carrier仍绑定X且旧快速槽X不能覆盖Y", { timeout: 10000 }, async () => {
  const m = memoryOwnerFixture(), archive = commandDayEndArchiveFixture();
  for (let index = 0; index < 2; index++) {
    const admitted = m.owner.admit({ kind: "new", setup: DEFAULT_SETUP, seed: "42" });
    if (admitted.kind !== "admitted") throw new Error("fixture必须admitted");
    m.owner.recordCommittedBoundary(admitted.ticket, { tick: 6, day: 2 });
    admitted.ticket.registerRelease(Promise.resolve({ kind: "released", finalBytes: 256 * MiB, growthStopped: true, referencesDropped: true }));
    await m.owner.finish(admitted.ticket);
  }
  assert.equal(m.owner.admit({ kind: "load", archive }).kind, "reload");
  const originalCarrier = JSON.parse(m.memory.session.data.get(SNAPSHOT_KEY)!);
  const next = new WasmDocumentMemoryOwner({ maximumBytes: m.memory.options.maximumBytes, reclaimBytes: m.memory.options.reclaimBytes,
    readCapacity: () => ({ kind: "enforced-capacity", initialPages: 1024, maximumPages: 4096, liveUpperBoundBytes: 256 * MiB }),
    restoreCapabilities: { forArchive: () => ({ kind: "verified-package", wasmSha256: "a".repeat(64), glueSha256: "b".repeat(64), sourceSha256: "c".repeat(64), buildManifestSha256: "d".repeat(64), exports: ["restore_json", "facade_slot_init_for_restore_json"] }) },
    createStorage: (liveUpperBoundBytes) => new WasmMemoryStorage({ ...m.memory.options, liveUpperBoundBytes }) });
  const y = { ...archive, seed: "99" };
  let panel: MemoryHandoverPanelProps | null = null, fileReads = 0, quickslotReads = 0, creates = 0;
  const view = (): MemoryHandoverPanelProps => { assert.ok(panel); return panel; };
  const selection = createMemoryHandoverSelection({
    loadFromFile: async () => { fileReads++; return parseSaveSlot(JSON.parse(JSON.stringify(y))); },
    createNewIntent: () => { throw new Error("选择文件不得生成新局seed"); },
    onChange: value => { panel = value; },
  });
  const f = fixture({ wasmMemoryOwner: next, hostTransitionRef: next.hostTransitionRef,
    confirmMemoryHandover: selection.request,
    getBrowserSaveRepository: () => ({ load: async () => { quickslotReads++; return archive; } }),
    prepareWasmRestoreCapability: async selected => { assert.deepEqual(selected, y); },
  });
  f.host.tick = () => 6; f.host.day = () => 2;
  f.ports.createHost = async (_setup, seed, _target, _sink, selected, ticket) => {
    creates++; assert.equal(seed, 99n); assert.deepEqual(selected, y); assert.ok(ticket);
    assert.equal(JSON.parse(m.memory.session.data.get(SNAPSHOT_KEY)!).handover, undefined);
    ticket.registerRelease(Promise.resolve({ kind: "not-allocated" })); return f.host;
  };
  const current = createSessionHostLifecycle(f.ports), starting = current.start();
  await settleTasks(); assert.deepEqual(view().selectedIntent, { kind: "load", archive });
  const carrier = m.memory.session.data.get(SNAPSHOT_KEY);
  assert.deepEqual(JSON.parse(carrier!).handover, originalCarrier.handover);
  assert.equal(JSON.parse(carrier!).chainId, originalCarrier.chainId);
  view().onSelectFile(); await settleTasks();
  assert.equal(fileReads, 1); assert.deepEqual(view().selectedIntent, { kind: "load", archive: y });
  assert.equal(m.memory.session.data.get(SNAPSHOT_KEY), carrier); assert.equal(creates, 0);
  view().onContinue(); await starting;
  assert.equal(creates, 1); assert.equal(quickslotReads, 0); assert.deepEqual(archive.seed, "42");
  assert.equal(f.calls.includes("load"), false); assert.equal(f.calls.includes("ready"), true);
  await current.dispose(); selection.dispose();
});

test("用户取消冻结目标确认正常返回启动选择，原carrier与明确Y均保留且零新分配", { timeout: 10000 }, async () => {
  const m = memoryOwnerFixture(), archive = commandDayEndArchiveFixture();
  for (let index = 0; index < 2; index++) {
    const a = m.owner.admit({ kind: "new", setup: DEFAULT_SETUP, seed: "42" });
    if (a.kind !== "admitted") throw new Error("fixture必须admitted");
    m.owner.recordCommittedBoundary(a.ticket, { tick: 6, day: 2 });
    a.ticket.registerRelease(Promise.resolve({ kind: "released", finalBytes: 256 * MiB, growthStopped: true, referencesDropped: true })); await m.owner.finish(a.ticket);
  }
  m.owner.admit({ kind: "load", archive });
  const before = m.memory.session.data.get(SNAPSHOT_KEY); let returned = 0, created = 0;
  const errors: unknown[] = [], y = { ...archive, seed: "99" };
  const f = fixture({ wasmMemoryOwner: m.owner, hostTransitionRef: m.owner.hostTransitionRef,
    confirmMemoryHandover: async () => null, onMemoryHandoverCancelled: async () => { returned++; },
    createHost: async () => { created++; throw new Error("取消不得分配"); },
    getBrowserSaveRepository: () => { throw new Error("取消不得读快速槽"); }, setError: error => errors.push(error) });
  f.source.select(y); await f.runtime.start();
  assert.equal(returned, 1); assert.equal(created, 0); assert.deepEqual(errors, []);
  assert.equal(m.memory.session.data.get(SNAPSHOT_KEY), before); assert.equal(await f.source.read(async () => null), y);
  assert.deepEqual(await m.owner.hostTransitionRef.current, { kind: "released" }); await f.runtime.dispose();
});

test("原冻结交接重试由用户取消也正常返回启动选择，不reload不分配不报宿主错误", { timeout: 10000 }, async () => {
  const m = memoryOwnerFixture();
  for (let index = 0; index < 2; index++) {
    const a = m.owner.admit({ kind: "new", setup: DEFAULT_SETUP, seed: "42" });
    if (a.kind !== "admitted") throw new Error("fixture必须admitted");
    m.owner.recordCommittedBoundary(a.ticket, { tick: 4, day: 1 });
    a.ticket.registerRelease(Promise.resolve({ kind: "released", finalBytes: 256 * MiB, growthStopped: true, referencesDropped: true })); await m.owner.finish(a.ticket);
  }
  m.memory.session.onCall(call => { if (call.operation === "set" && call.key === SNAPSHOT_KEY) throw new Error("quota"); });
  assert.throws(() => m.owner.admit({ kind: "new", setup: DEFAULT_SETUP, seed: "42" }));m.memory.session.onCall(null);
  let returned = 0; const errors: unknown[] = [], retry = m.owner.pendingReloadRetry();
  const f = fixture({ wasmMemoryOwner: m.owner, hostTransitionRef: m.owner.hostTransitionRef,
    confirmMemoryReloadRetry: async () => false, onMemoryHandoverCancelled: async () => { returned++; },
    createHost: async () => { throw new Error("取消不得分配"); }, reloadAfterMemoryNotice: async () => { throw new Error("取消不得reload"); }, setError: error => errors.push(error) });
  await f.runtime.start(); assert.equal(returned, 1); assert.deepEqual(errors, []);assert.deepEqual(m.owner.pendingReloadRetry(), retry);
  await f.runtime.dispose();
});

test("旧create拒绝后仍等待已登记Memory回执，不能将create失败当零残留", { timeout: 10000 }, async () => {
  const m = memoryOwnerFixture(), completion = deferred<WasmFinalRelease>(), creating = deferred<void>(), rejectCreate = deferred<EngineHost>();
  const f = fixture({ wasmMemoryOwner: m.owner, hostTransitionRef: m.owner.hostTransitionRef });
  f.ports.createHost = async (_setup, _seed, _target, _sink, _x, ticket) => {
    ticket!.registerRelease(completion.promise); creating.resolve(); return rejectCreate.promise;
  };
  const first = createSessionHostLifecycle(f.ports), starting = first.start(); await creating.promise; await first.dispose();
  let nextCreates = 0;
  const next = createSessionHostLifecycle({ ...f.ports, createHost: async (_setup, _seed, _target, _sink, _x, ticket) => {
    nextCreates++; ticket!.registerRelease(Promise.resolve({ kind: "not-allocated" })); return commandHostFixture({ ...f.host, tick: () => 0, day: () => 0 });
  } });
  const nextStarting = next.start(); rejectCreate.reject(new Error("构造失败，资源仍在终结")); await settleTasks();
  assert.equal(nextCreates, 0); assert.equal(JSON.parse(m.memory.session.data.get(SNAPSHOT_KEY)!).liveUpperBound, 256 * MiB);
  completion.resolve({ kind: "released", finalBytes: 64 * MiB, growthStopped: true, referencesDropped: true });
  await Promise.all([starting, nextStarting]); assert.equal(nextCreates, 1);
  assert.equal(JSON.parse(m.memory.session.data.get(SNAPSHOT_KEY)!).sessionResident, 64 * MiB);
  await next.dispose();
});

test("协议registration清理抛错仍须dispose与真实receipt终结，保原失败并禁止后继", { timeout: 10000 }, async () => {
  const m = memoryOwnerFixture(); const cause = new Error("协议解绑失败"); let disposed = 0;
  const f = fixture({ wasmMemoryOwner: m.owner, hostTransitionRef: m.owner.hostTransitionRef, disconnectProtocol: () => { throw cause; } });
  f.host.tick = () => 1; f.host.day = () => 0; f.host.dispose = async () => { disposed++; };
  f.ports.createHost = async (_setup, _seed, _target, _sink, _x, ticket) => {
    ticket!.registerRelease(Promise.resolve({ kind: "released", finalBytes: 64 * MiB, growthStopped: true, referencesDropped: true })); return f.host;
  };
  const current = createSessionHostLifecycle(f.ports); await current.start();
  await assert.rejects(current.dispose(), (error) => error === cause);
  assert.equal(disposed, 1); assert.equal(JSON.parse(m.memory.session.data.get(SNAPSHOT_KEY)!).liveUpperBound, 0);
  let creates = 0; await createSessionHostLifecycle({ ...f.ports, createHost: async () => { creates++; return f.host; } }).start();
  assert.equal(creates, 0);
});

test("真实factory坏runtime query在Worker前登记确定not-allocated，保原错且无残留计费", { timeout: 10000 }, async () => {
  const module = await import("./useSessionHostLifecycle.ts");
  const s = memoryOwnerFixture(), admitted = s.owner.admit({ kind: "new", setup: DEFAULT_SETUP, seed: "42" });
  if (admitted.kind !== "admitted") throw new Error("预期admitted");
  const oldWindow = globalThis.window;
  Object.assign(globalThis, { window: { location: { search: "?wasmRuntime=bad" } } });
  try {
    assert.throws(() => module.createSessionHost(DEFAULT_SETUP, 42n, { kind: "wasm" }, false, () => {}, null, admitted.ticket), /wasmRuntime/);
    await s.owner.finish(admitted.ticket);
    assert.equal(JSON.parse(s.memory.session.data.get(SNAPSHOT_KEY)!).sessionResident, 0);
    assert.equal(s.owner.admit({ kind: "new", setup: DEFAULT_SETUP, seed: "42" }).kind, "admitted");
  } finally { Object.assign(globalThis, { window: oldWindow }); }
});

test("失败交接重试须确认原X，不读quickslot/抽新seed/受理Y，提示后才reload", { timeout: 10000 }, async () => {
  const m = memoryOwnerFixture();
  for (let index = 0; index < 2; index++) {
    const a = m.owner.admit({ kind: "new", setup: DEFAULT_SETUP, seed: "42" });
    if (a.kind !== "admitted") throw new Error("预期admitted");
    m.owner.recordCommittedBoundary(a.ticket, { tick: 4, day: 1 });
    a.ticket.registerRelease(Promise.resolve({ kind: "released", finalBytes: 256 * MiB, growthStopped: true, referencesDropped: true })); await m.owner.finish(a.ticket);
  }
  m.memory.session.onCall((call) => { if (call.operation === "set" && call.key === SNAPSHOT_KEY) throw new Error("quota"); });
  assert.throws(() => m.owner.admit({ kind: "new", setup: DEFAULT_SETUP, seed: "42" }));
  m.memory.session.onCall(null);
  const confirmed = deferred<boolean>(); let prompts = 0, creates = 0, reloads = 0;
  const f = fixture({ wasmMemoryOwner: m.owner, hostTransitionRef: m.owner.hostTransitionRef,
    createSeed: () => { throw new Error("同事件重试不得再抽seed"); }, getBrowserSaveRepository: () => { throw new Error("同事件重试不得读quickslot"); },
    createHost: async () => { creates++; throw new Error("reload前不得分配"); },
    confirmMemoryReloadRetry: async (handover) => { prompts++; assert.equal(handover.pendingIntent?.kind, "new"); if (handover.pendingIntent?.kind === "new") assert.equal(handover.pendingIntent.seed, "42"); return confirmed.promise; },
    reloadAfterMemoryNotice: async () => { reloads++; assert.ok(f.calls.some((call) => /已冻结本次选择/.test(call))); },
  });
  f.source.select({ ...commandDayEndArchiveFixture(), seed: "99" });
  const current = createSessionHostLifecycle(f.ports), starting = current.start(); await settleTasks();
  assert.equal(prompts, 1); assert.equal(reloads, 0); assert.equal(creates, 0);
  confirmed.resolve(true); await starting;
  assert.equal(reloads, 1); assert.equal(creates, 0);
  assert.equal(JSON.parse(m.memory.session.data.get(SNAPSHOT_KEY)!).handover.pendingIntent.seed, "42");
  assert.equal((await f.source.read(async () => null))!.seed, "99", "本次Y未被静默消费或覆盖");
});

test("坏query与回执登记同时失败，用户可见文本保留两个原始原因", { timeout: 10000 }, async () => {
  const { createSessionHost } = await import("./useSessionHostLifecycle.ts");
  const old = Object.getOwnPropertyDescriptor(globalThis, "window"), registration = new Error("登记端口拒绝");
  Object.defineProperty(globalThis, "window", { configurable: true, value: { location: { search: "?wasmRuntime=bad" } } });
  const admitted = memoryOwnerFixture().owner.admit({ kind: "new", setup: DEFAULT_SETUP, seed: "42" });
  if (admitted.kind !== "admitted") throw new Error("预期admitted");
  try {
    assert.throws(() => createSessionHost(DEFAULT_SETUP, 42n, { kind: "wasm" }, false, () => {}, null,
      { ...admitted.ticket, registerRelease: () => { throw registration; } }), (error: unknown) => {
        assert.ok(error instanceof AggregateError); assert.ok(error.errors.includes(registration));
        assert.match(fatalWasmInitializationMessage(error), /wasmRuntime.*登记端口拒绝/); return true;
      });
  } finally { if (old === undefined) Reflect.deleteProperty(globalThis, "window"); else Object.defineProperty(globalThis, "window", old); }
});

test("健康WASM释放先stop确认和refreshBaseline，再读取最终commit边界后dispose", { timeout: 10000 }, async () => {
  const m = memoryOwnerFixture(); const order: string[] = []; let cachedTick = 0, paused = false;
  const f = fixture({ wasmMemoryOwner: m.owner, hostTransitionRef: m.owner.hostTransitionRef });
  f.host.tick = () => { order.push("tick"); return cachedTick; }; f.host.day = () => { order.push("day"); return 2; };
  f.host.stop = async () => { order.push("stop"); paused = true; };
  f.host.refreshBaseline = async () => { assert.equal(paused, true); order.push("baseline"); cachedTick = 9; };
  f.host.dispose = async () => { order.push("dispose"); };
  f.ports.createHost = async (_setup, _seed, _target, _sink, _x, ticket) => {
    ticket!.registerRelease(Promise.resolve({ kind: "released", finalBytes: 64 * MiB, growthStopped: true, referencesDropped: true })); return f.host;
  };
  const captured: { tick: number; day: number }[] = [], record = m.owner.recordCommittedBoundary.bind(m.owner);
  m.owner.recordCommittedBoundary = (ticket, boundary) => { captured.push(boundary); record(ticket, boundary); };
  const current = createSessionHostLifecycle(f.ports); await current.start(); order.length = 0; paused = false;
  await current.dispose();
  assert.deepEqual(order, ["stop", "baseline", "tick", "day", "dispose"]);
  assert.deepEqual(captured, [{ tick: 9, day: 2 }]);
});

test("WASM stop或baseline失败仍dispose与finish，不把旧cached值记为最终边界", { timeout: 10000 }, async () => {
  for (const stage of ["stop", "baseline"] as const) {
    const m = memoryOwnerFixture(), cause = new Error(`${stage}失败`); let stops = 0, disposed = 0, reads = 0, refreshes = 0;
    const f = fixture({ wasmMemoryOwner: m.owner, hostTransitionRef: m.owner.hostTransitionRef, isDocumentHidden: () => false });
    f.host.tick = () => { reads++; return 0; }; f.host.day = () => { reads++; return 0; };
    f.host.stop = async () => { stops++; if (stage === "stop") throw cause; };
    f.host.refreshBaseline = async () => { refreshes++; if (stage === "baseline") throw cause; };
    f.host.dispose = async () => { disposed++; };
    f.ports.createHost = async (_setup, _seed, _target, _sink, _x, ticket) => {
      ticket!.registerRelease(Promise.resolve({ kind: "released", finalBytes: 64 * MiB, growthStopped: true, referencesDropped: true })); return f.host;
    };
    const current = createSessionHostLifecycle(f.ports); await current.start();
    await assert.rejects(current.stopCurrentSession(), (error: unknown) => error === cause);
    assert.equal(stops, 1); assert.equal(refreshes, stage === "stop" ? 0 : 1); assert.equal(reads, 0); assert.equal(disposed, 1);
    assert.equal(JSON.parse(m.memory.session.data.get(SNAPSHOT_KEY)!).sessionResident, 64 * MiB);
    const outcome = await m.owner.hostTransitionRef.current; assert.equal(outcome.kind, "failed");
  }
});

test("释放清理相位仅内部ACK同宿主transport，取消UI不消费且相位结束仍拒绝", { timeout: 10000 }, async () => {
  const m = memoryOwnerFixture(), f = fixture({ wasmMemoryOwner: m.owner, hostTransitionRef: m.owner.hostTransitionRef });
  f.host.tick = () => 9; f.host.day = () => 2;
  f.ports.createHost = async (_setup, _seed, _target, _sink, _x, ticket) => {
    ticket!.registerRelease(Promise.resolve({ kind: "released", finalBytes: 64 * MiB, growthStopped: true, referencesDropped: true })); return f.host;
  };
  let baselineAck: unknown, protocolAck: unknown;
  f.host.refreshBaseline = async () => {
    baselineAck = f.update()({ type: "baseline" } as HostUpdate);
    protocolAck = f.update()({ type: "protocol" } as HostUpdate);
    if (baselineAck === false) throw new Error("Worker消费者拒绝baseline");
  };
  const current = createSessionHostLifecycle(f.ports); await current.start(); f.calls.length = 0;
  await current.dispose();
  assert.equal(baselineAck, true); assert.equal(protocolAck, true);
  assert.equal(f.calls.includes("update"), false, "清理baseline不可消费到已取消UI");
  assert.equal(f.update()({ type: "baseline" } as HostUpdate), false, "清理相位结束后旧baseline不再ACK");
  assert.equal(f.update()({ type: "protocol" } as HostUpdate), false, "清理相位结束后旧protocol不再ACK");
});

test("正式 WASM 初始 X 在 metadata 准备完成之前不得 admit 或创建 Worker", { timeout: 10000 }, async () => {
  let ready!: () => void;
  const waiting = new Promise<void>((resolve) => { ready = resolve; });
  const calls: string[] = [];
  const owner = { pendingReloadRetry: () => undefined, pendingHandover: () => undefined, admit() { calls.push("admit"); throw new Error("fixture 已确认准入排序"); } } as unknown as NonNullable<SessionHostLifecyclePorts["wasmMemoryOwner"]>;
  const f = fixture({ wasmMemoryOwner: owner, prepareWasmRestoreCapability: async (archive) => { assert.deepEqual(archive, expected); calls.push("prepare"); await waiting; }, createHost: async () => { calls.push("create"); throw new Error("不可创建"); } });
  const expected = commandDayEndArchiveFixture(); f.source.select(expected);
  const starting = f.runtime.start();
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(calls, ["prepare"], "PREPARE_MUST_PRECEDE_ADMISSION_AND_CARRIER_CONSUME");
  ready(); await starting;
  assert.deepEqual(calls, ["prepare", "admit"]);
});

test("普通 WASM 新局不请求 restore metadata 或静默等待其网络", { timeout: 10000 }, async () => {
  const f = fixture({ prepareWasmRestoreCapability: async () => { throw new Error("新局不可读取 restore metadata"); } });
  await f.runtime.start();
  assert.ok(f.calls.includes("create")); assert.ok(f.calls.includes("ready"));
  await f.runtime.dispose();
});
