/**
 * Web Worker：在独立线程跑 WASM engine。
 *
 * 核心设计：引擎步进速度与 UI 更新频率完全解耦。
 *
 * 速度模型：
 * - 1x = 每 1 秒 1 步。Nx = 每 1000/N ms 1 步。
 * - MAX = 尽力而为：tight while-loop 填满 90% CPU。
 *
 * UI 更新（固定 10fps = 100ms）：
 * - 每 100ms 把累积的事件（PriceTick 取末值 Map 去重、Trade 全保留）flush 给主线程。
 * - 10fps 对分时图足够流畅（Lightweight Charts 内部有动画插值）。
 *
 * 快照刷新（500ms = 2fps）：
 * - 快照（含 bids/asks/positions 全量数据）结构化克隆较重 → 低频刷新。
 * - 实时价格走 PriceTick 事件（轻量），不走快照。
 */
import type { EngineEvent, Intent, SessionSetup, Snapshot } from "../types/engine";

const ctx = self as unknown as { postMessage: (msg: unknown) => void; addEventListener: (type: string, cb: (e: MessageEvent) => void) => void };

let wasmModule: typeof import("../../wasm-pkg/web_wasm.js") | null = null;
let handle: number | null = null;
let timer: ReturnType<typeof setTimeout> | null = null;
let speed = 1;
let running = false;

// ── 常量 ──
const TICK_MS = 1000;
const UI_FLUSH_MS = 100;
const SNAPSHOT_MS = 500;
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

// ── 事件累积器（Map 去重，O(1) per PriceTick）──
let pendingTicks = new Map<string, EngineEvent>(); // code → 最新 PriceTick
let pendingOther: EngineEvent[] = []; // Trade/DayBoundary/IntentRejected/SettlementError/VError

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

// ── 快照刷新计时 ──
let lastSnapshot = 0;
function maybeSnapshot(now: number): void {
  if (now - lastSnapshot >= SNAPSHOT_MS) {
    if (handle !== null && wasmModule) {
      const raw = wasmModule.snapshot(handle);
      const snap = deepNormalize<Snapshot>(raw);
      ctx.postMessage({ type: "snapshot", snapshot: snap });
    }
    lastSnapshot = now;
  }
}

function stepOnce(): void {
  if (handle !== null && wasmModule) {
    const ev = wasmModule.step(handle) as EngineEvent[];
    mergeStep(ev);
  }
}

// ── 固定速度模式 ──
function startFixedSpeed(): void {
  const stepInterval = TICK_MS / speed;
  let lastStep = performance.now();
  let lastFlush = performance.now();
  lastSnapshot = performance.now();

  function tick() {
    if (!running) return;
    const now = performance.now();

    // 补跑所有「应该已经发生」的步（setTimeout 可能延迟）
    while (now - lastStep >= stepInterval) {
      lastStep += stepInterval;
      stepOnce();
      if (now - lastStep > stepInterval * 50) {
        // 落后太多（如标签页休眠后恢复）→ 跳到当前，不补跑
        lastStep = now;
        break;
      }
    }

    if (now - lastFlush >= UI_FLUSH_MS) {
      flushEvents();
      lastFlush = now;
    }
    maybeSnapshot(now);

    const delay = Math.max(1, Math.min(stepInterval - (now - lastStep), UI_FLUSH_MS));
    timer = setTimeout(tick, delay);
  }
  timer = setTimeout(tick, stepInterval);
}

// ── MAX 模式（tight while-loop，90% CPU）──
function startMaxSpeed(): void {
  lastSnapshot = performance.now();

  function cycle() {
    if (!running) return;
    const cycleStart = performance.now();
    const cpuBudget = UI_FLUSH_MS * CPU_RATIO; // 90ms 计算

    // tight loop：尽可能多跑
    let steps = 0;
    while (steps < SAFETY_MAX_STEPS) {
      if (performance.now() - cycleStart >= cpuBudget) break;
      stepOnce();
      steps++;
    }

    // flush + 快照
    const now = performance.now();
    flushEvents();
    maybeSnapshot(now);

    // 让出 ~10ms 给事件循环（处理 enqueue/setSpeed + 浏览器呼吸）
    const elapsed = performance.now() - cycleStart;
    const yield_ms = Math.max(1, UI_FLUSH_MS - elapsed);
    timer = setTimeout(cycle, yield_ms);
  }
  timer = setTimeout(cycle, 0);
}

function startLoop(): void {
  stopLoop();
  running = true;
  if (speed === Infinity) {
    startMaxSpeed();
  } else {
    startFixedSpeed();
  }
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
        if (running) startLoop();
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
