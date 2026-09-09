/**
 * WASM 引擎宿主（主线程简化实现，无 Worker）。
 *
 * 职责：
 * - 加载 wasm-pkg（init）。
 * - 在 start() 时 create_session，开启定时器周期性 step。
 * - 把 snapshot 中的 Map（markets / accounts）规整为普通对象，供 RTK 消费。
 * - 速度：1x = 每 1 秒推进一个 tick。
 */
import init, * as wasm from "../../wasm-pkg/web_wasm.js";
import type { SaveSlot, SessionSetup, Snapshot } from "../types/engine";
import type { EngineHost } from "./engine-host";
import type { HostUpdate } from "./host-update.ts";
import {
  UI_TARGET_HZ,
  UI_UPDATE_INTERVAL_MS,
  createBaselineUpdate,
  createDeltaUpdate,
  hostEventSeq,
} from "./host-update.ts";
import { compactFastForwardEvents, normalizeWasmStepEvents } from "./event-buffer";
import { requiresRuntimeSnapshot } from "./runtime-snapshot-policy";
import { normalizeSerdeMaps, prepareSaveForWasm } from "./serde-normalize";
import { HostSpeedMeter, assertValidSpeedMultiplier } from "./speed.ts";

/** 1x 速度对应的步进间隔（毫秒）。 */
const BASE_INTERVAL_MS = 1000;
const FASTEST_SLICE_MS = 8;
const FASTEST_SLICE_MAX_STEPS = 100_000;

let wasmReady: Promise<void> | null = null;

/** 加载并初始化 wasm（幂等，重复调用直接返回已就绪的 Promise）。 */
export function ensureWasmReady(): Promise<void> {
  if (wasmReady) return wasmReady;
  wasmReady = (async () => {
    const resp = await fetch(new URL("../../wasm-pkg/web_wasm_bg.wasm", import.meta.url));
    if (!resp.ok) {
      throw new Error(
        `加载 wasm 二进制失败：HTTP ${resp.status} ${resp.statusText}（路径 web_wasm_bg.wasm）`,
      );
    }
    const buf: ArrayBuffer = await resp.arrayBuffer();
    const bytes = new Uint8Array(buf);
    await init(bytes);
  })();
  return wasmReady;
}

/** 读取快照并深度规整 Map 字段。 */
function readSnapshot(handle: number): Snapshot {
  return normalizeSerdeMaps<Snapshot>(wasm.snapshot(handle));
}

function readRuntimeSnapshot(handle: number): Snapshot {
  return normalizeSerdeMaps<Snapshot>(wasm.runtime_snapshot(handle));
}

/** 工厂：创建一个绑定到指定 setup/seed 的 EngineHost。 */
export function createWasmHost(setup: SessionSetup, seed: bigint): EngineHost {
  let handle: number | null = null;
  let timer: ReturnType<typeof setInterval> | null = null;
  let speed = 1;
  let onUpdate: ((update: HostUpdate) => void) | null = null;
  let baselineDelivered = false;
  let pendingEvents: ReturnType<typeof normalizeWasmStepEvents> = [];
  let pendingFromSeq: number | null = null;
  let pendingToSeq: number | null = null;
  let lastPublishAt = performance.now();
  const speedMeter = new HostSpeedMeter(() => performance.now());

  function currentIntervalMs(): number {
    if (!Number.isFinite(speed)) throw new Error("最快模式不使用逐 tick 定时器");
    return Math.max(1, Math.round(BASE_INTERVAL_MS / speed));
  }

  function stepOnce(): void {
    if (handle === null) return;
    const events = normalizeWasmStepEvents(wasm.step(handle));
    speedMeter.recordTicks();
    for (const event of events) {
      const seq = hostEventSeq(event);
      pendingFromSeq ??= seq;
      pendingToSeq = seq;
    }
    pendingEvents.push(...events);
  }

  function runFastestSlice(): void {
    if (timer === null || handle === null || speed !== Infinity) return;
    const startedAt = performance.now();
    let steps = 0;
    while (performance.now() - startedAt < FASTEST_SLICE_MS && steps < FASTEST_SLICE_MAX_STEPS) {
      stepOnce();
      steps += 1;
    }
    pendingEvents = compactFastForwardEvents(pendingEvents);
    if (performance.now() - lastPublishAt >= UI_UPDATE_INTERVAL_MS) flushPendingEvents();
    timer = setTimeout(runFastestSlice, 0);
  }

  function startTimer(): void {
    if (timer !== null) return;
    if (speed === Infinity) {
      timer = setTimeout(runFastestSlice, 0);
      return;
    }
    timer = setInterval(() => {
      stepOnce();
      if (performance.now() - lastPublishAt >= UI_UPDATE_INTERVAL_MS) flushPendingEvents();
    }, currentIntervalMs());
  }

  function flushPendingEvents(): void {
    if (handle === null || pendingEvents.length === 0) return;
    const events = pendingEvents;
    pendingEvents = [];
    if (pendingFromSeq === null || pendingToSeq === null) throw new Error("WASM 待发布事件缺少原始 seq 覆盖区间");
    const coverage = { fromSeq: pendingFromSeq, toSeq: pendingToSeq };
    pendingFromSeq = null;
    pendingToSeq = null;
    lastPublishAt = performance.now();
    const runtimeSnapshot = requiresRuntimeSnapshot(events) ? readRuntimeSnapshot(handle) : undefined;
    onUpdate?.(createDeltaUpdate(events, runtimeSnapshot, coverage));
  }

  function stopTimer(): void {
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
    flushPendingEvents();
  }

  return {
    capabilities: {
      deliveryModes: [],
      targetUiHz: UI_TARGET_HZ,
      sharedMemory: false,
      reconnect: false,
    },
    start(updateCb) {
      onUpdate = updateCb;
      if (handle === null) {
        handle = wasm.create_session(setup, seed);
      }
      if (!baselineDelivered) {
        onUpdate(createBaselineUpdate(readSnapshot(handle)));
        baselineDelivered = true;
      }
      lastPublishAt = performance.now();
      startTimer();
      speedMeter.setRunning(true);
    },
    stop() {
      stopTimer();
      speedMeter.setRunning(false);
    },
    dispose() {
      onUpdate = null;
      stopTimer();
      if (handle !== null) wasm.drop_session(handle);
      handle = null;
      speedMeter.setRunning(false);
    },
    setSpeed(x) {
      assertValidSpeedMultiplier(x);
      speed = x;
      speedMeter.setSpeed(x);
      if (timer !== null) {
        stopTimer();
        startTimer();
      }
    },
    setFrameRate(_fps: number) {
      // 主线程 host 不需要帧率控制（同步调用）
    },
    async readSpeedMetrics() {
      return speedMeter.read();
    },
    async save() {
      if (handle === null) throw new Error("会话尚未创建");
      return normalizeSerdeMaps<SaveSlot>(wasm.save(handle));
    },
    async load(slot: SaveSlot) {
      flushPendingEvents();
      const restoredHandle = wasm.restore(prepareSaveForWasm(slot) as SaveSlot);
      const restoredSnapshot = readSnapshot(restoredHandle);
      const previousHandle = handle;
      handle = restoredHandle;
      if (previousHandle !== null) wasm.drop_session(previousHandle);
      onUpdate?.(createBaselineUpdate(restoredSnapshot));
      speedMeter.setSpeed(speed);
    },
    async submitIntent(intent) {
      if (handle === null) {
        throw new Error("会话尚未创建，无法提交意图（请先 start）");
      }
      wasm.enqueue(handle, intent);
    },
    snapshot() {
      if (handle === null) {
        throw new Error("会话尚未创建，无法读取快照");
      }
      return readSnapshot(handle);
    },
    tick() {
      if (handle === null) {
        throw new Error("会话尚未创建，无法读取 tick");
      }
      return Number(wasm.tick(handle));
    },
    day() {
      if (handle === null) {
        throw new Error("会话尚未创建，无法读取交易日");
      }
      return wasm.day(handle);
    },
  };
}
