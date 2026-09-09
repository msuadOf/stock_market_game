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
 *   快照：在成交、挂撤单、集合竞价结束、跨日等权威账户状态变化后推送，重连时全量推送。
 */
import type { EngineEvent, Intent, SaveSlot, SessionSetup, Snapshot } from "../types/engine";
import { HostSpeedMeter, assertValidSpeedMultiplier } from "./speed.ts";
import { requiresRuntimeSnapshot } from "./runtime-snapshot-policy";
import {
  compactFastForwardEvents,
  normalizeWasmStepEvents,
  uiBackpressurePolicy,
} from "./event-buffer";
import { normalizeSerdeMaps, prepareSaveForWasm } from "./serde-normalize";
import { postWorkerFlush, shouldFlushWorkerEvents } from "./worker-flush";
import { UI_TARGET_HZ } from "./host-update.ts";
import { hostEventSeq } from "./host-update.ts";

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const ctx: any = self;

let wasmModule: typeof import("../../wasm-pkg/web_wasm.js") | null = null;
let handle: number | null = null;
let timer: ReturnType<typeof setTimeout> | null = null;
let speed = 1;
let running = false;
let flushMs = 1000 / UI_TARGET_HZ;
let awaitingUiFrame = false;
const speedMeter = new HostSpeedMeter(() => performance.now());

// ── 常量 ──
const TICK_MS = 1000;
const FRAME_MS = 16;
const CPU_RATIO = 0.9;
const SAFETY_MAX_STEPS = 100000;

// ── 事件累积（保留引擎顺序）──
// 分时图需以 60 个逐秒 PriceTick 聚合成一分钟；不能按刷新帧去重，否则会丢失
// 游戏时间，导致一分钟量柱提前或永远无法闭合。
let pendingEvents: EngineEvent[] = [];
let needsRuntimeSnapshot = false;
let pendingFromSeq: number | null = null;
let pendingToSeq: number | null = null;

function mergeStep(events: EngineEvent[]): void {
  for (const ev of events) {
    const seq = hostEventSeq(ev);
    pendingFromSeq ??= seq;
    pendingToSeq = seq;
    if (requiresRuntimeSnapshot([ev])) needsRuntimeSnapshot = true;
    pendingEvents.push(ev);
  }
}

function flushEvents(force = false): void {
  if (!shouldFlushWorkerEvents(pendingEvents.length, awaitingUiFrame, force)) return;
  // 720x / 最快时，浏览器不可能逐个绘制每个中间 tick。引擎仍完整推进，
  // UI 帧只接收足以精确恢复所有收盘日 K 与当前日 K 的权威事件，避免主线程积压。
  const eventsForUi = speed >= 720 ? compactFastForwardEvents(pendingEvents) : pendingEvents;
  // 账户或交易阶段变化后的轻量快照描述的是这一批事件作用后的状态，必须排在事件之后发送，
  // 否则主线程会先渲染新账户状态，再被旧事件批次短暂覆盖。
  const runtimeSnapshot = needsRuntimeSnapshot ? readSnapshot(false) : undefined;
  needsRuntimeSnapshot = false;
  if (pendingFromSeq === null || pendingToSeq === null) throw new Error("Worker 待发布事件缺少原始 seq 覆盖区间");
  postWorkerFlush(ctx, eventsForUi, runtimeSnapshot, { fromSeq: pendingFromSeq, toSeq: pendingToSeq });
  awaitingUiFrame = true;
  pendingEvents = [];
  pendingFromSeq = null;
  pendingToSeq = null;
}

function readSnapshot(includeDailyCandles = true): Snapshot | undefined {
  if (handle !== null && wasmModule) {
    const raw = includeDailyCandles ? wasmModule.snapshot(handle) : wasmModule.runtime_snapshot(handle);
    return normalizeSerdeMaps<Snapshot>(raw);
  }
}

function pushSnapshot(includeDailyCandles = true): void {
  const snapshot = readSnapshot(includeDailyCandles);
  if (snapshot !== undefined) ctx.postMessage({ type: "snapshot", snapshot });
}

function stepOnce(): void {
  if (handle !== null && wasmModule) {
    const ev = normalizeWasmStepEvents(wasmModule.step(handle));
    speedMeter.recordTicks();
    mergeStep(ev);
  }
}

// ── 统一帧循环 ──
let lastStepTime = 0;
let lastFlush = 0;

function frameLoop(): void {
  if (!running) return;
  const policy = uiBackpressurePolicy(awaitingUiFrame);
  const now = performance.now();

  if (policy.stepEngine && speed === Infinity) {
    // MAX：tight while-loop 填满 90% 帧时间
    const budget = FRAME_MS * CPU_RATIO;
    let steps = 0;
    while (steps < SAFETY_MAX_STEPS) {
      if (performance.now() - now >= budget) break;
      stepOnce();
      steps++;
    }
  } else if (policy.stepEngine) {
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

  // UI 尚未确认上一批时，引擎照常跑；只把不可见的中间事件压缩到有界集合，
  // 避免“最快”被 rAF 限速，也避免 postMessage 队列和内存无界增长。
  if (!policy.flushUi && speed >= 720 && pendingEvents.length > 0) {
    pendingEvents = compactFastForwardEvents(pendingEvents);
  }

  // 按主线程请求的帧率 flush
  if (policy.flushUi && now - lastFlush >= flushMs) {
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
  speedMeter.setRunning(true);
  awaitingUiFrame = false;
  lastStepTime = performance.now();
  lastFlush = performance.now();
  timer = setTimeout(frameLoop, FRAME_MS);
}

function stopLoop(): void {
  running = false;
  speedMeter.setRunning(false);
  if (timer !== null) {
    clearTimeout(timer);
    timer = null;
  }
  // stop/restore 是时间线屏障。即使上一帧尚未确认，也必须借助 Worker
  // postMessage 的 FIFO 顺序先发布旧时间线尾批，再交付 restored baseline。
  flushEvents(true);
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

          // 初始化 rayon 多线程池。浏览器同时需要 1 个引擎调度 Worker 和 1 个 UI
          // 主线程，因此计算池必须扣除这两个线程；只减 1 会实际占满所有逻辑核，
          // 导致“最快”仍在运算但图表无法获得渲染时间片。
          const logicalCores = (navigator as any).hardwareConcurrency || 4;
          const cores = Math.max(1, logicalCores - 2);
          if (wasmModule.initThreadPool) {
            try {
              await wasmModule.initThreadPool(cores);
              ctx.postMessage({ type: "ready", cores });
            } catch (initErr) {
              // 绝不静默 fallback（铁律二）：多核初始化失败 = 严重问题，必须上报。
              ctx.postMessage({
                type: "error",
                message: `WASM 多核初始化失败（${cores} 核）：${initErr instanceof Error ? initErr.message : String(initErr)}。` +
                  `可能原因：1) 浏览器不支持 SharedArrayBuffer（需 COOP/COEP 头）；` +
                  `2) wasm-bindgen 与 nightly TLS 模型不兼容；` +
                  `3) WASM 二进制未用 atomics 编译。` +
                  `engine 将无法利用多核并行。`,
              });
            }
          } else {
            ctx.postMessage({
              type: "error",
              message: "WASM 二进制缺少 initThreadPool 导出——未用 wasm-bindgen-rayon + atomics 编译。",
            });
          }
        } else {
          // 重复 init：之前已成功，不再重复。
          break;
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
        assertValidSpeedMultiplier(s);
        speed = s;
        speedMeter.setSpeed(s);
        lastStepTime = performance.now();
        break;
      }
      case "speedMetrics": {
        const requestId = msg.requestId as number;
        ctx.postMessage({ type: "speedMetrics", requestId, metrics: speedMeter.read() });
        break;
      }
      case "setFrameRate": {
        // 主线程告知它的刷新率 → Worker 调整 flush 间隔
        const fps = msg.fps as number;
        flushMs = fps > 0 ? 1000 / fps : 1000 / 30;
        break;
      }
      case "uiFrame": {
        awaitingUiFrame = false;
        break;
      }
      case "snapshot": {
        // 主动拉快照（断线重连/开盘）
        pushSnapshot();
        break;
      }
      case "enqueue": {
        const requestId = msg.requestId as number;
        try {
          if (handle === null || !wasmModule) throw new Error("无会话");
          wasmModule.enqueue(handle, msg.intent as Intent);
          ctx.postMessage({ type: "enqueued", requestId });
        } catch (error) {
          ctx.postMessage({
            type: "operationError",
            requestId,
            message: error instanceof Error ? error.message : String(error),
          });
        }
        break;
      }
      case "save": {
        const requestId = msg.requestId as number;
        try {
          if (handle === null || !wasmModule) throw new Error("无会话");
          const slot = normalizeSerdeMaps<SaveSlot>(wasmModule.save(handle));
          ctx.postMessage({ type: "saved", requestId, slot });
        } catch (error) {
          ctx.postMessage({
            type: "operationError",
            requestId,
            message: error instanceof Error ? error.message : String(error),
          });
        }
        break;
      }
      case "restore": {
        const requestId = msg.requestId as number;
        try {
          if (!wasmModule) throw new Error("wasm 未初始化");
          // 先完整恢复并读取快照，成功后再原子替换旧会话。
          const restoredHandle = wasmModule.restore(
            prepareSaveForWasm(msg.slot as SaveSlot) as SaveSlot,
          );
          const snapshot = normalizeSerdeMaps<Snapshot>(wasmModule.snapshot(restoredHandle));
          const previousHandle = handle;
          handle = restoredHandle;
          if (previousHandle !== null) {
            wasmModule.drop_session(previousHandle);
          }
          ctx.postMessage({ type: "restored", requestId, snapshot });
        } catch (error) {
          ctx.postMessage({
            type: "operationError",
            requestId,
            message: error instanceof Error ? error.message : String(error),
          });
        }
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
