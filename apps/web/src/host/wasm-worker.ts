/**
 * Web Worker：在独立线程跑 WASM engine。
 *
 * 统一帧循环架构（所有速度档 + MAX 共用）：
 *
 * 每 16ms（≈60fps）一个帧周期：
 *   1. 计算本帧应跑多少步
 *      - 固定速度：stepsDue = floor((now - lastStep) / stepInterval)
 *      - MAX：tight loop 跑到 90% 帧时间（14.4ms），自适应步数
 *   2. 跑 stepsDue 步，合并事件（PriceTick Map 去重 O(1)）
 *   3. flush 事件给主线程（~60fps）
 *   4. 每 ~500ms（30帧）推送一次快照
 *   5. yield 剩余时间给事件循环（处理 enqueue/setSpeed）
 *
 * 关于「系统 90% CPU」的重要事实：
 *   一个 Web Worker = 一个线程 = 1 个核心。
 *   在 N 核 CPU 上，Worker 100% = 1/N 系统 CPU。
 *   要真正用 90% 系统 CPU 需要 engine 内部并行（rayon/GPU，已设计未实现）。
 *   当前 MAX = 最大化这一个线程的利用率（~90% 单核）。
 */
import type { EngineEvent, Intent, SessionSetup, Snapshot } from "../types/engine";

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const ctx: any = self;

let wasmModule: typeof import("../../wasm-pkg/web_wasm.js") | null = null;
let handle: number | null = null;
let timer: ReturnType<typeof setTimeout> | null = null;
let speed = 1; // 正数 = 固定倍率；Infinity = MAX
let running = false;

// ── 常量 ──
const TICK_MS = 1000;        // 1x = 1000ms/step
const FRAME_MS = 16;         // 帧周期 ≈60fps（UI 更新频率）
const CPU_RATIO = 0.9;       // MAX 模式：90% 单核
const SNAPSHOT_FRAMES = 30;  // 快照每 30 帧（≈500ms）
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

// ── 事件累积器（Map 去重，O(1) per PriceTick）──
let pendingTicks = new Map<string, EngineEvent>();
let pendingOther: EngineEvent[] = [];

function mergeStep(events: EngineEvent[]): void {
  for (const ev of events) {
    if ("PriceTick" in ev) {
      pendingTicks.set(ev.PriceTick.code, ev);
    } else {
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
}

function stepOnce(): void {
  if (handle !== null && wasmModule) {
    const ev = wasmModule.step(handle) as EngineEvent[];
    mergeStep(ev);
  }
}

// ── 统一帧循环 ──
let lastStepTime = 0;
let frameCount = 0;

function frameLoop(): void {
  if (!running) return;
  const frameStart = performance.now();
  frameCount++;

  if (speed === Infinity) {
    // ── MAX 模式：tight loop 跑到 90% 帧时间 ──
    const budget = FRAME_MS * CPU_RATIO; // 14.4ms
    let steps = 0;
    while (steps < SAFETY_MAX_STEPS) {
      if (performance.now() - frameStart >= budget) break;
      stepOnce();
      steps++;
    }
  } else {
    // ── 固定速度：精确补跑应到步数 ──
    const stepInterval = TICK_MS / speed;
    let stepsThisFrame = 0;
    while (lastStepTime + stepInterval <= performance.now()) {
      lastStepTime += stepInterval;
      stepOnce();
      stepsThisFrame++;
      if (stepsThisFrame > SAFETY_MAX_STEPS) break; // 防失控
      // 如果落后超过 10 秒（标签页休眠恢复），跳到当前
      if (performance.now() - lastStepTime > 10000) {
        lastStepTime = performance.now();
        break;
      }
    }
  }

  // 每帧 flush 事件（~60fps）
  flushEvents();

  // 每 30 帧（~500ms）推送快照
  if (frameCount % SNAPSHOT_FRAMES === 0) {
    if (handle !== null && wasmModule) {
      const raw = wasmModule.snapshot(handle);
      const snap = deepNormalize<Snapshot>(raw);
      ctx.postMessage({ type: "snapshot", snapshot: snap });
    }
  }

  // yield 剩余时间给事件循环
  const elapsed = performance.now() - frameStart;
  const yieldMs = Math.max(1, FRAME_MS - elapsed);
  timer = setTimeout(frameLoop, yieldMs);
}

function startLoop(): void {
  stopLoop();
  running = true;
  lastStepTime = performance.now();
  frameCount = 0;
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
        }
        ctx.postMessage({ type: "ready" });
        break;
      }
      case "create": {
        if (!wasmModule) throw new Error("wasm 未初始化");
        handle = wasmModule.create_session(msg.setup as SessionSetup, msg.seed as bigint);
        ctx.postMessage({ type: "created", handle });
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
        // 切速度时重置步进计时（避免新速度下补跑大量旧步）
        lastStepTime = performance.now();
        break;
      }
      case "snapshot": {
        // 主动拉快照（断线重连/开盘时用）
        if (handle === null || !wasmModule) throw new Error("无会话");
        const raw = wasmModule.snapshot(handle);
        const snap = deepNormalize<Snapshot>(raw);
        ctx.postMessage({ type: "snapshot", snapshot: snap });
        break;
      }
      case "enqueue": {
        if (handle === null || !wasmModule) throw new Error("无会话");
        wasmModule.enqueue(handle, msg.intent as Intent);
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
