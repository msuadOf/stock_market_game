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

function fixture(overrides: Partial<SessionHostLifecyclePorts> = {}) {
  const calls: string[] = [];
  let update!: (value: HostUpdate) => void, failure!: (value: HostFailure) => void;
  const host = commandHostFixture({
    start: async (onUpdate: typeof update, onFailure: typeof failure) => { calls.push("start"); update = onUpdate; failure = onFailure; },
    stop: async () => { calls.push("stop"); }, dispose: async () => { calls.push("dispose"); },
    setSpeed: async () => { calls.push("speed"); }, setPausePreferences: async () => { calls.push("preferences"); },
    load: async () => { calls.push("load"); }, submitIntent: async () => {},
    initialAllocation: async () => ({ stocks: [] }),
  });
  const source = new InitialSaveSource<StrictSaveEnvelope>();
  const ports: SessionHostLifecyclePorts = {
    hostRef: { current: null }, initialSaveSourceRef: { current: source }, dayEndPersistenceRef: { current: new DayEndPersistence() },
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
    setError: (value) => { calls.push(String(value)); }, onRunning: (value) => { calls.push(`running:${value}`); }, onAutoTriggered: () => {}, ...overrides,
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
  const restored = fixture();
  restored.source.select(commandDayEndArchiveFixture());
  restored.host.initialAllocation = async () => { throw new Error("读档不能假称初始分配"); };
  await restored.runtime.start();
  assert.ok(restored.calls.includes("load"));
  assert.ok(restored.calls.includes("ready"));
  await restored.runtime.dispose();
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
  const f = fixture(); f.source.select(archive);
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
    const f = fixture();
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
  const f = fixture();
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
    if (mode === "save") f.source.select({ ...commandDayEndArchiveFixture(), seed: "9007199254740993" });
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
  await next.start();
  complete(f.host);
  await starting;
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
