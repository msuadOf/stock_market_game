/**
 * WASM 引擎宿主（主线程简化版 v1，无 Worker）。
 *
 * 职责：
 * - 加载 wasm-pkg（init）。
 * - 在 start() 时 create_session，开启定时器周期性 step。
 * - 把 snapshot 中的 Map（markets / accounts）规整为普通对象，供 RTK 消费。
 * - 速度：1x = 每 600ms 推进一个 tick。
 */
import init, * as wasm from "../../wasm-pkg/web_wasm.js";
import type { EngineEvent, Intent, SessionSetup, Snapshot } from "../types/engine";

export interface EngineHost {
  start(onEvents: (events: EngineEvent[]) => void): void;
  stop(): void;
  setSpeed(x: number): void;
  setFrameRate(fps: number): void;
  submitIntent(intent: Intent): void;
  snapshot(): Snapshot;
  tick(): number;
  day(): number;
}

/** 1x 速度对应的步进间隔（毫秒）。 */
const BASE_INTERVAL_MS = 600;

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

/** 深度规整：递归把所有 JS Map 转为普通 Object（serde-wasm-bindgen 默认产出 Map）。
 *  snapshot.markets / snapshot.accounts / accountSnap.positions 都是 Map → 需深度转。 */
function deepNormalize<T>(obj: unknown): T {
  if (obj instanceof Map) {
    const result: Record<string, unknown> = {};
    for (const [key, value] of obj.entries()) {
      result[String(key)] = deepNormalize(value);
    }
    return result as T;
  }
  if (Array.isArray(obj)) {
    return obj.map(deepNormalize) as T;
  }
  if (obj !== null && typeof obj === 'object') {
    const result: Record<string, unknown> = {};
    for (const [key, value] of Object.entries(obj)) {
      result[key] = deepNormalize(value);
    }
    return result as T;
  }
  return obj as T;
}

/** 读取快照并深度规整 Map 字段。 */
function readSnapshot(handle: number): Snapshot {
  return deepNormalize<Snapshot>(wasm.snapshot(handle));
}

/** 工厂：创建一个绑定到指定 setup/seed 的 EngineHost。 */
export function createWasmHost(setup: SessionSetup, seed: bigint): EngineHost {
  let handle: number | null = null;
  let timer: ReturnType<typeof setInterval> | null = null;
  let speed = 1;
  let onEvents: ((events: EngineEvent[]) => void) | null = null;

  function currentIntervalMs(): number {
    return Math.max(1, Math.round(BASE_INTERVAL_MS / speed));
  }

  function startTimer(): void {
    if (timer !== null) return;
    timer = setInterval(() => {
      if (handle === null) return;
      const events = wasm.step(handle) as EngineEvent[];
      if (events.length > 0 && onEvents) {
        onEvents(events);
      }
    }, currentIntervalMs());
  }

  function stopTimer(): void {
    if (timer !== null) {
      clearInterval(timer);
      timer = null;
    }
  }

  return {
    start(cb) {
      onEvents = cb;
      if (handle === null) {
        handle = wasm.create_session(setup, seed);
      }
      startTimer();
    },
    stop() {
      stopTimer();
    },
    setSpeed(x) {
      if (x <= 0) {
        stopTimer();
        throw new Error(`非法速度倍率：${x}（必须为正数）`);
      }
      speed = x;
      if (timer !== null) {
        stopTimer();
        startTimer();
      }
    },
    setFrameRate(_fps: number) {
      // 主线程 host 不需要帧率控制（同步调用）
    },
    submitIntent(intent) {
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
