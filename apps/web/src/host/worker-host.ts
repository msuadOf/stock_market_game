/**
 * WorkerHost：通过 Web Worker 调用多核 WASM engine。
 *
 * Worker 内部：init wasm → initThreadPool(N核) → create session → 帧循环。
 * 主线程：被动接收 events/snapshot，rAF 渲染。
 *
 * 帧率协商：主线程告诉 Worker 它的渲染帧率（rAF 自然 60fps 或目标 30fps），
 * Worker 按此频率 flush 事件 → 不超频推送。
 */
import type { EngineEvent, Intent, SessionSetup, Snapshot } from "../types/engine";
import type { EngineHost } from "./wasm-host";

interface WorkerMsg {
  type: string;
  [key: string]: unknown;
}

export function createWorkerHost(setup: SessionSetup, seed: bigint): Promise<EngineHost> {
  return new Promise((resolve, reject) => {
    const worker = new Worker(new URL("./wasm-worker.ts", import.meta.url), { type: "module" });
    let onEvents: ((events: EngineEvent[]) => void) | null = null;
    let cachedSnapshot: Snapshot | null = null;
    let initialized = false;
    const timeout = setTimeout(() => {
      if (!initialized) {
        worker.terminate();
        reject(new Error("Worker 初始化超时（10s）— 检查 SharedArrayBuffer/COOP-COEP 头"));
      }
    }, 10000);

    worker.addEventListener("message", (e: MessageEvent) => {
      const msg = e.data as WorkerMsg;
      switch (msg.type) {
        case "ready":
          console.log(`[WorkerHost] WASM 就绪，rayon 线程池：${msg.cores} 核`);
          worker.postMessage({ type: "create", setup, seed });
          break;
        case "created":
          // 首张快照会在 create 后由 worker 主动推送
          break;
        case "snapshot":
          cachedSnapshot = msg.snapshot as Snapshot;
          if (!initialized) {
            initialized = true;
            clearTimeout(timeout);
            resolve(makeHost());
          }
          break;
        case "events":
          if (onEvents) onEvents(msg.events as EngineEvent[]);
          break;
        case "error":
          console.error("[WorkerHost]", msg.message);
          if (!initialized) {
            clearTimeout(timeout);
            worker.terminate();
            reject(new Error(String(msg.message)));
          }
          break;
      }
    });

    // 启动初始化
    worker.postMessage({ type: "init" });

    function makeHost(): EngineHost {
      // 告诉 Worker 当前的渲染帧率（默认 30fps，主线程可改）
      worker.postMessage({ type: "setFrameRate", fps: 30 });

      return {
        start(cb) {
          onEvents = cb;
          worker.postMessage({ type: "start" });
        },
        stop() {
          worker.postMessage({ type: "stop" });
        },
        setSpeed(x: number) {
          if (x <= 0) throw new Error(`非法速度倍率：${x}`);
          worker.postMessage({ type: "setSpeed", speed: x });
        },
        setFrameRate(fps: number) {
          worker.postMessage({ type: "setFrameRate", fps });
        },
        submitIntent(intent: Intent) {
          worker.postMessage({ type: "enqueue", intent });
        },
        snapshot(): Snapshot {
          if (!cachedSnapshot) throw new Error("快照尚未就绪");
          return cachedSnapshot;
        },
        tick(): number {
          if (!cachedSnapshot) return 0;
          return cachedSnapshot.tick;
        },
        day(): number {
          if (!cachedSnapshot) return 0;
          return cachedSnapshot.day;
        },
        save(): Promise<unknown> {
          return new Promise((resolve, reject) => {
            const handler = (e: MessageEvent) => {
              const msg = e.data as WorkerMsg;
              if (msg.type === "saved") {
                worker.removeEventListener("message", handler);
                resolve(msg.slot);
              } else if (msg.type === "error" && String(msg.message).includes("save")) {
                worker.removeEventListener("message", handler);
                reject(new Error(String(msg.message)));
              }
            };
            worker.addEventListener("message", handler);
            worker.postMessage({ type: "save" });
          });
        },
        async load(slot: unknown) {
          // 停当前循环，发 restore，等新快照
          worker.postMessage({ type: "stop" });
          return new Promise<void>((resolve, reject) => {
            const handler = (e: MessageEvent) => {
              const msg = e.data as WorkerMsg;
              if (msg.type === "snapshot") {
                cachedSnapshot = msg.snapshot as Snapshot;
                worker.removeEventListener("message", handler);
                worker.postMessage({ type: "start" });
                resolve();
              } else if (msg.type === "error" && String(msg.message).includes("restore")) {
                worker.removeEventListener("message", handler);
                reject(new Error(String(msg.message)));
              }
            };
            worker.addEventListener("message", handler);
            worker.postMessage({ type: "restore", slot });
          });
        },
      };
    }
  });
}

export const MAX_SPEED = Infinity;
