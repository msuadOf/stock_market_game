/**
 * WorkerHost：通过 Web Worker 间接调用 WASM engine。
 *
 * 速度模型（与 wasm-worker.ts 对齐）：
 * - 正数 speed = 固定倍率（1x=1秒/步, 2x=0.5秒/步...）。
 * - Infinity = MAX（尽力而为，90% CPU）。
 * - UI 通知固定每 1 秒一次（Worker 内部控制，不由这里轮询）。
 *
 * 初始化流程：init → ready → create → created → 首张快照 → 就绪。
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
        reject(new Error("Worker 初始化超时（5s）"));
      }
    }, 5000);

    worker.addEventListener("message", (e: MessageEvent) => {
      const msg = e.data as WorkerMsg;
      switch (msg.type) {
        case "ready":
          worker.postMessage({ type: "create", setup, seed });
          break;
        case "created":
          worker.postMessage({ type: "snapshot" });
          break;
        case "snapshot":
          cachedSnapshot = msg.snapshot as Snapshot;
          if (!initialized) {
            initialized = true;
            clearTimeout(timeout);
            resolve(makeHost());
          }
          // 后续 snapshot 也更新缓存
          break;
        case "events":
          if (onEvents) onEvents(msg.events as EngineEvent[]);
          // 每次收到事件后也拉一张快照（保持缓存新鲜，Worker 内 1s 通知不会太频繁）
          worker.postMessage({ type: "snapshot" });
          break;
        case "error":
          if (!initialized) {
            clearTimeout(timeout);
            worker.terminate();
            reject(new Error(String(msg.message)));
          }
          break;
      }
    });

    worker.postMessage({ type: "init" });

    function makeHost(): EngineHost {
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
      };
    }
  });
}

/** MAX 档的速度值（传给 worker → Infinity → 尽力而为模式）。 */
export const MAX_SPEED = Infinity;
