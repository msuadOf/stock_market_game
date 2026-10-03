import assert from "node:assert/strict";
import test from "node:test";
import { createSessionHostLifecycle, type SessionHostLifecyclePorts } from "./useSessionHostLifecycle.ts";
import { InitialSaveSource, SessionReplacementGate } from "../save/session-replacement.ts";
import { DayEndPersistence } from "../save/day-end-persistence.ts";
import { PlayerOrderRefreshGate } from "../components/player-orders.ts";
import { DEFAULT_SETUP } from "../config/defaults.ts";
import type { EngineHost } from "../host/engine-host.ts";
import type { HostFailure, HostUpdate } from "../host/host-update.ts";
import type { StrictSaveEnvelope } from "../save/schema/root.ts";
import { commandHostFixture, commandDayEndArchiveFixture } from "./command-host-test-fixture.ts";
import { AutoOrderManager } from "../components/auto-order-manager.ts";

function fixture(overrides: Partial<SessionHostLifecyclePorts> = {}) {
  const calls: string[] = [];
  let update!: (value: HostUpdate) => void, failure!: (value: HostFailure) => void;
  const host = commandHostFixture({
    start: (onUpdate: typeof update, onFailure: typeof failure) => { calls.push("start"); update = onUpdate; failure = onFailure; },
    stop: () => { calls.push("stop"); }, dispose: () => { calls.push("dispose"); },
    setSpeed: () => { calls.push("speed"); }, setPausePreferences: async () => { calls.push("preferences"); },
    load: async () => { calls.push("load"); }, submitIntent: async () => {},
  });
  const source = new InitialSaveSource<StrictSaveEnvelope>();
  const ports: SessionHostLifecyclePorts = {
    hostRef: { current: null }, initialSaveSourceRef: { current: source }, dayEndPersistenceRef: { current: new DayEndPersistence() },
    autoOrderMgrRef: { current: null }, sessionReplacementGateRef: { current: new SessionReplacementGate() }, saveSelectionGenerationRef: { current: 0 },
    playerOrderRefreshGateRef: { current: new PlayerOrderRefreshGate() },
    hostUpdateRef: { current: () => { calls.push("update"); } }, fatalHostErrorRef: { current: () => { calls.push("failure"); } },
    startupTarget: { kind: "wasm" }, sessionSetup: DEFAULT_SETUP, speed: 1, pauseAfterClose: true, pauseBeforeOpen: false,
    TRADING_E2E_MODE: false, malformedProtocolFixture: () => false,
    setIndicatorCalculator: () => { calls.push("register"); return () => { calls.push("unregister"); }; },
    connectProtocol: () => { calls.push("connect"); }, disconnectProtocol: () => { calls.push("disconnect"); },
    createHost: async () => { calls.push("create"); return host; }, checkWasmEnvironment: () => { calls.push("environment"); },
    isDocumentHidden: () => true, getBrowserSaveRepository: () => ({ load: async () => { calls.push("read"); return null; } }),
    setActiveSetup: () => { calls.push("setup"); }, setStartDateDraft: () => {}, setPriceCageEnabledDraft: () => {},
    setDeliveryModes: () => {}, setDeliveryModeState: () => {}, setNotice: (value) => calls.push(value), setReady: () => { calls.push("ready"); },
    setError: (value) => { calls.push(String(value)); }, onRunning: (value) => { calls.push(`running:${value}`); }, onAutoTriggered: () => {}, ...overrides,
  };
  return { calls, host, source, ports, runtime: createSessionHostLifecycle(ports), update: () => update, failure: () => failure };
}

test("SessionHostLifecycle 先校验环境和读取，再创建/注册/启动；后台补暂停", async () => {
  const f = fixture(); await f.runtime.start();
  assert.deepEqual(f.calls, ["environment", "read", "create", "setup", "register", "connect", "speed", "preferences", "start", "stop", "running:true", "ready"]);
  assert.equal(f.ports.hostRef.current, f.host);
  let reads = 0; await f.source.read(async () => { reads++; return null; }); assert.equal(reads, 0);
});
test("停止共享会话先失效令牌再 stop，释放 registration/coordinator/manager/host；晚到消息丢弃", async () => {
  const f = fixture(); await f.runtime.start(); f.calls.length = 0;
  const replacement = f.ports.sessionReplacementGateRef.current.begin()!;
  const refresh = f.ports.playerOrderRefreshGateRef.current.next();
  f.ports.autoOrderMgrRef.current = new AutoOrderManager(async () => {});
  f.ports.autoOrderMgrRef.current.clear = () => { f.calls.push("manager-clear"); };
  f.runtime.stopCurrentSession();
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
  f.runtime.dispose(); resolve(f.host); await starting;
  assert.equal(f.ports.hostRef.current, null); assert.equal(f.calls.includes("start"), false); assert.equal(f.calls.includes("ready"), false);
  assert.equal(f.calls.filter((value) => value === "dispose").length, 1);
});
test("启动档 setup/seed 优先于默认配置，load 完成后才能接线和 start", async () => {
  const archive = { ...commandDayEndArchiveFixture(), setup: { ...DEFAULT_SETUP, start_date: "2031-02-03" }, seed: "9007199254740993" };
  let seenSeed: bigint | null = null;
  const f = fixture(); f.source.select(archive);
  f.ports.createHost = async (setup, seed) => { assert.equal(setup, archive.setup); seenSeed = seed; return f.host; };
  const runtime = createSessionHostLifecycle(f.ports); await runtime.start();
  assert.equal(seenSeed, 9007199254740993n); assert.ok(f.calls.indexOf("load") < f.calls.indexOf("connect"));
});
test("初始化失败释放已创建资源并显式报告当前宿主错误", async () => {
  const f = fixture(); f.host.setPausePreferences = async () => { throw new Error("偏好 IPC 失败"); };
  await f.runtime.start(); assert.equal(f.ports.hostRef.current, null);
  assert.equal(f.calls.includes("dispose"), true); assert.equal(f.calls.includes("ready"), false);
  assert.ok(f.calls.some((value) => /偏好 IPC 失败/.test(value)));
});
