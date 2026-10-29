import assert from "node:assert/strict";
import test from "node:test";
import { fixture, drain } from "../host/worker-host-w1.test-support.ts";
import { snapshot, frame, tickBatch } from "../host/protocol-test-fixtures.ts";
import { catalogWorkerFixture } from "../host/catalog-test-fixture.ts";
import { createSessionHostLifecycle } from "./useSessionHostLifecycle.ts";
import type { SessionHostLifecyclePorts } from "./useSessionHostLifecycle.ts";
import type { HostUpdate } from "../host/host-update.ts";
import { WasmDocumentMemoryOwner } from "../host/wasm-document-memory-owner.ts";
import { WasmMemoryStorage } from "../host/wasm-memory-storage.ts";
import { fixture as memoryFixture, MiB } from "../host/wasm-memory-storage.test-support.ts";
import { InitialSaveSource, SessionReplacementGate } from "../save/session-replacement.ts";
import { DayEndPersistence } from "../save/day-end-persistence.ts";
import { PlayerOrderRefreshGate } from "../components/player-orders.ts";
import { DEFAULT_SETUP } from "../config/defaults.ts";

// 使用真实 WorkerHost、EngineSupervisor 和 MessageChannel 验证 JS 关闭编排；fixture 回执不证明 Rust join。
test("真实WorkerHost关闭时drain在途TickBatch且内部ACK最终baseline，UI只消费关闭前数据", { timeout: 10000 }, async () => {
  const f = fixture(), memory = memoryFixture();
  let lifecycle: ReturnType<typeof createSessionHostLifecycle> | undefined;
  try {
    f.launch(); await drain(); f.ingress();
    f.workers[0]!.emit({ ...catalogWorkerFixture(snapshot(0, 0)), type: "baseline", civilDate: "2030-01-02", generation: 1, snapshot: snapshot(0, 0) });
    await drain(); f.ack(f.workers[1]!, 1); await drain();
    const outcome = f.readOutcome(); assert.equal(outcome.status, "fulfilled");
    if (outcome.status !== "fulfilled") throw new Error("fixture宿主未就绪");
    const host = outcome.host, engine = f.workers[0]!, original = engine.onPost;
    const order: string[] = [], delivered: HostUpdate[] = [];
    engine.onPost = (value, transfer) => {
      original?.(value, transfer);
      const types: Record<string, string> = { setSpeed: "speedSet", setPausePreferences: "pausePreferencesSet", start: "started", stop: "stopped", refreshBaseline: "refreshed" };
      const reply = types[String(value.type)];
      if (reply === undefined) return;
      order.push(String(value.type));
      queueMicrotask(() => {
        if (value.type === "stop") engine.emit({ type: "protocol", generation: 1, deliveryId: 2, update: tickBatch([frame(2, 0, [0])], snapshot(2, 2)) });
        engine.emit({ type: reply, generation: 1, requestId: value.requestId,
          ...(value.type === "refreshBaseline" ? { ...catalogWorkerFixture(snapshot(2, 2)), snapshot: snapshot(2, 2), civilDate: "2030-01-02" } : {}) });
      });
    };
    const owner = new WasmDocumentMemoryOwner({ maximumBytes: memory.options.maximumBytes, reclaimBytes: memory.options.reclaimBytes,
      readCapacity: () => ({ kind: "enforced-capacity", initialPages: 1024, maximumPages: 4096, liveUpperBoundBytes: 256 * MiB }),
      createStorage: (liveUpperBoundBytes) => new WasmMemoryStorage({ ...memory.options, liveUpperBoundBytes }) });
    const positions: { tick: number; day: number }[] = [], record = owner.recordCommittedBoundary.bind(owner);
    owner.recordCommittedBoundary = (ticket, position) => { positions.push(position); record(ticket, position); };
    const no = () => {};
    const ports: SessionHostLifecyclePorts = {
      hostRef: { current: null }, hostTransitionRef: owner.hostTransitionRef, wasmMemoryOwner: owner,
      initialSaveSourceRef: { current: new InitialSaveSource() }, dayEndPersistenceRef: { current: new DayEndPersistence() },
      autoOrderMgrRef: { current: null }, sessionReplacementGateRef: { current: new SessionReplacementGate() }, saveSelectionGenerationRef: { current: 0 },
      playerOrderRefreshGateRef: { current: new PlayerOrderRefreshGate() }, hostUpdateRef: { current: (update) => { delivered.push(update); return true; } }, fatalHostErrorRef: { current: no },
      startupTarget: { kind: "wasm" }, sessionSetup: DEFAULT_SETUP, speed: 1, pauseAfterClose: true, pauseBeforeOpen: false, TRADING_E2E_MODE: false,
      malformedProtocolFixture: () => false, setIndicatorCalculator: () => no, connectProtocol: no, disconnectProtocol: no,
      createHost: async (_setup, _seed, _target, _sink, _archive, ticket) => {
        ticket!.registerRelease(Promise.resolve({ kind: "released", finalBytes: 64 * MiB, growthStopped: true, referencesDropped: true }));
        return { ...host, initialAllocation: async () => ({ stocks: [] }) };
      }, onSpawnFailure: no, createSeed: () => 42n, checkWasmEnvironment: no, isDocumentHidden: () => false,
      getBrowserSaveRepository: () => ({ load: async () => null }), setActiveSetup: no, configureMarketTiming: no, setStartDateDraft: no,
      setPriceCageEnabledDraft: no, setFloatAllocationDraft: no, setInitialAllocation: no, setDeliveryModes: no, setDeliveryModeState: no,
      setNotice: no, setReady: no, setError: no, onRunning: no, onAutoTriggered: no,
    };
    lifecycle = createSessionHostLifecycle(ports); await lifecycle.start();
    assert.deepEqual(delivered.map(update => update.type), ["baseline"], "启动应向UI交付缓存baseline");
    engine.emit({ type: "protocol", generation: 1, deliveryId: 1, update: tickBatch([frame(1, 0, [0])], snapshot(1, 1)) });
    assert.deepEqual(delivered.map(update => update.type), ["baseline", "protocol"]);
    assert.equal(host.tick(), 0, "运行protocol不会更新baseline缓存，不能直接当最终位置");
    await lifecycle.dispose();
    assert.deepEqual(positions, [{ tick: 2, day: 0 }]);
    assert.deepEqual(order.slice(-2), ["stop", "refreshBaseline"]);
    assert.deepEqual(delivered.map(update => update.type), ["baseline", "protocol"], "在途tick2与清理baseline只ACK transport，不得交付取消的UI");
  } finally {
    try { await lifecycle?.dispose(); } finally { await f.cleanup(); }
  }
});
