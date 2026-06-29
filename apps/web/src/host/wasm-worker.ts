/**
 * Web Worker：在独立线程跑 WASM engine，不阻塞主线程渲染。
 *
 * 主线程 → Worker 命令（postMessage）：create / step / snapshot / enqueue / drop / setSpeed。
 * Worker → 主线程（postMessage）：events / snapshot / ready / error。
 *
 * 720x 高频：Worker 内 rAF 节流推送（PriceTick 合并取末值、Trade 全保留）。
 */
import type { EngineEvent, Intent, SessionSetup, Snapshot } from "../types/engine";

/// <reference lib="webworker" />

const ctx = self as unknown as { postMessage: (msg: unknown) => void; addEventListener: (type: string, cb: (e: MessageEvent) => void) => void };

let wasmModule: typeof import("../../wasm-pkg/web_wasm.js") | null = null;
let handle: number | null = null;
let timer: ReturnType<typeof setInterval> | null = null;
let speed = 1;
// 性能保护：主线程通知下限 ~16fps（MIN_NOTIFY_MS），Worker 内部批量跑步但不过载。

/** 深度规整 Map → Object（serde-wasm-bindgen 默认产出 Map）。 */
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

function stopLoop(): void {
  if (timer !== null) {
    clearInterval(timer);
    timer = null;
  }
}

/** 高速模式：批量推进 N 步后只推一次合并事件（PriceTick 取末值，Trade 全保留）。
 *  性能保护：无论 speed 多高，主线程最多每 MIN_NOTIFY_MS 收到一次通知（~16fps），
 *  Worker 内部可以跑很多步但不过载 postMessage/结构化克隆。
 */
const MIN_NOTIFY_MS = 60; // 主线程通知下限：~16fps，保 UI 流畅
const MAX_BATCH_STEPS = 50; // 单次通知最多推进步数（防 Worker 跑满 CPU）

function startLoopBatched(): void {
  if (timer !== null) return;
  timer = setInterval(() => {
    if (handle === null || !wasmModule) return;
    try {
      const allEvents: EngineEvent[] = [];
      // 步数 = speed 的合理映射，但有上限
      const steps = Math.min(Math.max(1, Math.round(speed)), MAX_BATCH_STEPS);
      for (let i = 0; i < steps; i++) {
        const ev = wasmModule.step(handle) as EngineEvent[];
        allEvents.push(...ev);
      }
      if (allEvents.length > 0) {
        // 高速合并：同一股票的 PriceTick 只保留最后一个
        const deduped: EngineEvent[] = [];
        const seenPriceTick = new Set<string>();
        for (let i = allEvents.length - 1; i >= 0; i--) {
          const e = allEvents[i];
          if ("PriceTick" in e) {
            const code = e.PriceTick.code;
            if (seenPriceTick.has(code)) continue;
            seenPriceTick.add(code);
          }
          deduped.unshift(e);
        }
        ctx.postMessage({ type: "events", events: deduped });
      }
    } catch (e) {
      ctx.postMessage({ type: "error", message: String(e) });
    }
  }, MIN_NOTIFY_MS);
}

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
        startLoopBatched();
        break;
      }
      case "stop": {
        stopLoop();
        break;
      }
      case "setSpeed": {
        speed = msg.speed as number;
        if (speed <= 0) throw new Error(`非法速度：${speed}`);
        if (timer !== null) { stopLoop(); startLoopBatched(); }
        break;
      }
      case "step": {
        if (handle === null || !wasmModule) throw new Error("无会话");
        const events = wasmModule.step(handle) as EngineEvent[];
        ctx.postMessage({ type: "events", events });
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
