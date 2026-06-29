/**
 * Web Worker：在独立线程跑 WASM engine。
 *
 * 速度模型（用户定）：
 * - 1x = 每 1 秒 1 步 step。2x = 每 0.5 秒 1 步。Nx = 每 1000/N ms 1 步。
 * - MAX = 尽力而为：尽可能多跑步，受 90% CPU 占用上限约束。
 *
 * UI 通知频率与引擎速度解耦：
 * - 主线程每 1 秒收到一次合并事件（不管引擎跑了多少步）。
 * - 事件合并：同一股票 PriceTick 取末值、Trade 全保留、DayBoundary 全保留。
 *
 * 性能保护（MAX 档）：
 * - 每轮循环先测耗时，动态调整步数，使单轮≤目标帧时间（留 10% 余量给系统）。
 * - 到达 CPU 上限时不再加步数 → 尽力而为的加速比。
 */
import type { EngineEvent, Intent, SessionSetup, Snapshot } from "../types/engine";

const ctx = self as unknown as { postMessage: (msg: unknown) => void; addEventListener: (type: string, cb: (e: MessageEvent) => void) => void };

let wasmModule: typeof import("../../wasm-pkg/web_wasm.js") | null = null;
let handle: number | null = null;
let timer: ReturnType<typeof setTimeout> | null = null;
let speed = 1; // 正数=固定倍率；Infinity=MAX
let running = false;

// ── 常量 ──
const TICK_MS = 1000; // 1x = 1000ms/step
const UI_NOTIFY_MS = 1000; // 主线程 UI 更新间隔（固定 1s）
const CPU_BUDGET_RATIO = 0.9; // 最多用 90% CPU
const MAX_MEASURED_STEPS = 500; // 单轮测量上限

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

// ── 事件累积器（在两次 UI 通知之间累积）──
let pendingEvents: EngineEvent[] = [];

/** 把新事件合并进 pending（同股票 PriceTick 取末值）。 */
function mergeEvents(incoming: EngineEvent[]): void {
  for (const ev of incoming) {
    if ("PriceTick" in ev) {
      // 找已有的同 code PriceTick，替换；没有则追加
      const idx = pendingEvents.findIndex(
        (e) => "PriceTick" in e && e.PriceTick.code === ev.PriceTick.code,
      );
      if (idx >= 0) {
        pendingEvents[idx] = ev; // 替换为更新的
      } else {
        pendingEvents.push(ev);
      }
    } else {
      // Trade / DayBoundary / IntentRejected / SettlementError / VError：全保留
      pendingEvents.push(ev);
    }
  }
}

/** flush：把 pending 发给主线程，清空。 */
function flushEvents(): void {
  if (pendingEvents.length > 0) {
    ctx.postMessage({ type: "events", events: pendingEvents });
    pendingEvents = [];
  }
}

// ── 固定速度模式（1x/2x/5x/10x 等）──
//
// 引擎步进间隔 = TICK_MS / speed。
// UI 通知间隔 = UI_NOTIFY_MS（固定 1s）。
// 两者独立：引擎按自己的节奏跑，UI 按自己的节奏收。

function startFixedSpeed(): void {
  const stepInterval = Math.max(1, TICK_MS / speed);
  let lastNotify = performance.now();

  function tick() {
    if (!running) return;
    const t0 = performance.now();

    // 跑一步
    if (handle !== null && wasmModule) {
      const ev = wasmModule.step(handle) as EngineEvent[];
      mergeEvents(ev);
    }

    // UI 通知（固定 1s）
    const now = performance.now();
    if (now - lastNotify >= UI_NOTIFY_MS) {
      flushEvents();
      lastNotify = now;
    }

    timer = setTimeout(tick, Math.max(0, stepInterval - (performance.now() - t0)));
  }
  timer = setTimeout(tick, stepInterval);
}

// ── MAX 模式（尽力而为，90% CPU 上限）──
//
// 每轮跑一批步，测耗时，动态调步数。
// 目标：每轮总耗时 ≈ UI_NOTIFY_MS（1s），其中计算占 90%（900ms）。
// 若一步很快（<1ms），一轮可以跑几百步。
// 若一步很慢（大 NPC 数），一轮可能只跑几步。
// 无论如何，每 1s flush 一次 UI 通知。

function startMaxSpeed(): void {
  let stepsPerRound = 10; // 初始猜测
  let lastNotify = performance.now();

  function round() {
    if (!running) return;
    const t0 = performance.now();
    const cpuBudget = UI_NOTIFY_MS * CPU_BUDGET_RATIO; // 900ms

    let actualSteps = 0;
    for (let i = 0; i < stepsPerRound; i++) {
      if (handle === null || !wasmModule) break;
      const ev = wasmModule.step(handle) as EngineEvent[];
      mergeEvents(ev);
      actualSteps++;

      // 检查是否超出 CPU 预算
      if (performance.now() - t0 >= cpuBudget) break;
    }

    // 测量单步耗时，调整下一轮步数
    const elapsed = performance.now() - t0;
    if (actualSteps > 0 && elapsed > 0) {
      const msPerStep = elapsed / actualSteps;
      // 目标步数 = cpuBudget / msPerStep（留 10% 余量）
      const targetSteps = Math.max(1, Math.floor(cpuBudget / (msPerStep * 1.1)));
      stepsPerRound = Math.min(targetSteps, MAX_MEASURED_STEPS);
    }

    // UI 通知（固定 1s）
    const now = performance.now();
    if (now - lastNotify >= UI_NOTIFY_MS) {
      flushEvents();
      lastNotify = now;
    }

    // 立即开始下一轮（不等待）—— MAX 尽力而为
    timer = setTimeout(round, 0);
  }
  timer = setTimeout(round, 0);
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
        if (running) startLoop(); // 重启循环以应用新速度
        break;
      }
      case "snapshot": {
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
