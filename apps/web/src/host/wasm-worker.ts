/**
 * Web Worker：在独立线程跑 WASM engine（多核）。
 *
 * 架构：
 *   主线程（UI/React）
 *     ↕ postMessage（事件/快照）
 *   Worker 线程（本文件）
 *     ├─ WASM engine（rayon 内部自动 spawn N 个子 Worker 共享 SharedArrayBuffer）
 *     ├─ 子 Worker 1 ← SharedArrayBuffer
 *     ├─ 子 Worker 2 ← SharedArrayBuffer
 *     └─ ...（= navigator.hardwareConcurrency）
 *
 * 初始化流程：
 *   1. import wasm-pkg（含 rayon workerHelpers.js snippets）
 *   2. init(wasm_bytes) → 加载 WASM
 *   3. initThreadPool(cores) → 启动 N 个 rayon 子 Worker
 *   4. create_session → 开始模拟
 *
 * 速度模型（尽力而为）：
 *   统一帧循环（16ms≈60fps）。
 *   固定速度：精确补跑应到步数（stepInterval=1000/N ms）。
 *   MAX：tight while-loop 填满 90% 帧时间（14.4ms）。
 *   若 engine 跑不到目标速度 → 尽力而为（少跑几步，不报错）。
 *
 * 通信协议：
 *   主线程发帧率 → Worker 调整 flush 间隔。
 *   快照：每交易日(DayBoundary)推送一次 + 断线重连时推送。
 */
import type { EngineEvent, Intent, SessionSetup, Snapshot } from "../types/engine";

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const ctx: any = self;

let wasmModule: typeof import("../../wasm-pkg/web_wasm.js") | null = null;
let handle: number | null = null;
let timer: ReturnType<typeof setTimeout> | null = null;
let speed = 1;
let running = false;
let flushMs = 1000 / 30; // 默认 30fps（主线程发 setFrameRate 后覆盖）

// ── 常量 ──
const TICK_MS = 1000;
const FRAME_MS = 16;
const CPU_RATIO = 0.9;
const SAFETY_MAX_STEPS = 100000;

// ── 深度规整 ──
function deepNormalize<T>(obj: unknown): T {
  if (obj instanceof Map) {
    const r: Record<string, unknown> = {};
    for (const [k, v] of obj.entries()) r[String(k)] = deepNormalize(v);
    return r as T;
  }
  if (Array.isArray(obj)) return obj.map(deepNormalize) as T;
  if (obj !== null && typeof obj === "object") {
    const r: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(obj)) r[k] = deepNormalize(v);
    return r as T;
  }
  return obj as T;
}

// ── 事件累积（Map 去重 O(1)）──
let pendingTicks = new Map<string, EngineEvent>();
let pendingOther: EngineEvent[] = [];
let hadDayBoundary = false; // 日界标记（触发快照推送）

function mergeStep(events: EngineEvent[]): void {
  for (const ev of events) {
    if ("PriceTick" in ev) {
      pendingTicks.set(ev.PriceTick.code, ev);
    } else {
      if ("DayBoundary" in ev) hadDayBoundary = true;
      pendingOther.push(ev);
    }
  }
}

function flushEvents(): void {
  if (pendingTicks.size === 0 && pendingOther.length === 0) return;
  const all = [...pendingTicks.values(), ...pendingOther];
  ctx.postMessage({ type: "events", events: all });
  pendingTicks.clear();
  pendingOther = [];

  // 日界 → 推送快照
  if (hadDayBoundary) {
    hadDayBoundary = false;
    pushSnapshot();
  }
}

function pushSnapshot(): void {
  if (handle !== null && wasmModule) {
    const raw = wasmModule.snapshot(handle);
    const snap = deepNormalize<Snapshot>(raw);
    ctx.postMessage({ type: "snapshot", snapshot: snap });
  }
}

function stepOnce(): void {
  if (handle !== null && wasmModule) {
    const ev = wasmModule.step(handle) as EngineEvent[];
    mergeStep(ev);
  }
}

// ── 统一帧循环 ──
let lastStepTime = 0;
let lastFlush = 0;

function frameLoop(): void {
  if (!running) return;
  const now = performance.now();

  if (speed === Infinity) {
    // MAX：tight while-loop 填满 90% 帧时间
    const budget = FRAME_MS * CPU_RATIO;
    let steps = 0;
    while (steps < SAFETY_MAX_STEPS) {
      if (performance.now() - now >= budget) break;
      stepOnce();
      steps++;
    }
  } else {
    // 固定速度：精确补跑应到步数（尽力而为）
    const stepInterval = TICK_MS / speed;
    let stepsThisFrame = 0;
    while (lastStepTime + stepInterval <= performance.now()) {
      lastStepTime += stepInterval;
      stepOnce();
      stepsThisFrame++;
      if (stepsThisFrame > SAFETY_MAX_STEPS) break;
      if (performance.now() - lastStepTime > 10000) {
        lastStepTime = performance.now();
        break;
      }
    }
  }

  // 按主线程请求的帧率 flush
  if (now - lastFlush >= flushMs) {
    flushEvents();
    lastFlush = now;
  }

  const elapsed = performance.now() - now;
  const yieldMs = Math.max(1, Math.min(FRAME_MS, flushMs) - elapsed);
  timer = setTimeout(frameLoop, yieldMs);
}

function startLoop(): void {
  stopLoop();
  running = true;
  lastStepTime = performance.now();
  lastFlush = performance.now();
  timer = setTimeout(frameLoop, FRAME_MS);
}

function stopLoop(): void {
  running = false;
  if (timer !== null) {
    clearTimeout(timer);
    timer = null;
  }
}

// ── 消息处理 ──
ctx.addEventListener("message", async (e: MessageEvent) => {
  const msg = e.data;
  try {
    switch (msg.type) {
      case "init": {
        if (!wasmModule) {
          wasmModule = await import("../../wasm-pkg/web_wasm.js");
          const resp = await fetch(new URL("../../wasm-pkg/web_wasm_bg.wasm", import.meta.url));
          const buf = await resp.arrayBuffer();
          await wasmModule.default(new Uint8Array(buf));

          // 初始化 rayon 多线程池（用满浏览器所有核心）。
          // initThreadPool 内部 spawn 子 Worker + postMessage SharedArrayBuffer →
          // 需要 crossOriginIsolated（COOP/COEP），已在 vite.config.ts plugin 里设。
          // 若 initThreadPool 失败（如 dev 模式 Worker URL 解析问题），回退单线程。
          const cores = (navigator as any).hardwareConcurrency || 4;
          if (wasmModule.initThreadPool) {
            try {
              await wasmModule.initThreadPool(cores);
              ctx.postMessage({ type: "ready", cores });
            } catch (initErr) {
              console.warn("[Worker] rayon initThreadPool 失败，回退单线程:", initErr);
              ctx.postMessage({ type: "ready", cores: 1 });
            }
          } else {
            ctx.postMessage({ type: "ready", cores: 1 });
          }
        } else {
          ctx.postMessage({ type: "ready", cores: 1 });
        }
        break;
      }
      case "create": {
        if (!wasmModule) throw new Error("wasm 未初始化");
        handle = wasmModule.create_session(msg.setup as SessionSetup, msg.seed as bigint);
        ctx.postMessage({ type: "created", handle });
        pushSnapshot(); // 首张快照
        break;
      }
      case "start": {
        startLoop();
        break;
      }
      case "stop": {
        stopLoop();
        break;
      }
      case "setSpeed": {
        const s = msg.speed as number;
        if (s <= 0) throw new Error(`非法速度：${s}`);
        speed = s;
        lastStepTime = performance.now();
        break;
      }
      case "setFrameRate": {
        // 主线程告知它的刷新率 → Worker 调整 flush 间隔
        const fps = msg.fps as number;
        flushMs = fps > 0 ? 1000 / fps : 1000 / 30;
        break;
      }
      case "snapshot": {
        // 主动拉快照（断线重连/开盘）
        pushSnapshot();
        break;
      }
      case "enqueue": {
        if (handle === null || !wasmModule) throw new Error("无会话");
        wasmModule.enqueue(handle, msg.intent as Intent);
        break;
      }
      case "save": {
        if (handle === null || !wasmModule) throw new Error("无会话");
        const slot = wasmModule.save(handle);
        ctx.postMessage({ type: "saved", slot });
        break;
      }
      case "restore": {
        if (!wasmModule) throw new Error("wasm 未初始化");
        // 销毁旧会话，从存档恢复
        if (handle !== null) {
          wasmModule.drop_session(handle);
          handle = null;
        }
        handle = wasmModule.restore(msg.slot);
        // 推送新快照让主线程更新
        pushSnapshot();
        break;
      }
      case "drop": {
        if (handle !== null && wasmModule) {
          wasmModule.drop_session(handle);
          handle = null;
        }
        stopLoop();
        break;
      }
    }
  } catch (err) {
    ctx.postMessage({ type: "error", message: err instanceof Error ? err.message : String(err) });
  }
});
