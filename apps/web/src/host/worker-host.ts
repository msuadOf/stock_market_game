/**
 * WorkerHost：通过 Web Worker 间接调用 WASM engine。
 *
 * 主线程不直接碰 wasm；所有调用经 postMessage 发给 Worker，异步等回包。
 * 不阻塞 UI 渲染（720x 高速时关键）。
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
          break;
        case "events":
          if (onEvents) onEvents(msg.events as EngineEvent[]);
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

    /** 定期刷新快照缓存（主线程 snapshot() 是同步的，需异步维护缓存）。 */
    function startSnapshotRefresh(): void {
      setInterval(() => {
        if (initialized) worker.postMessage({ type: "snapshot" });
      }, 500);
    }

    function makeHost(): EngineHost {
      startSnapshotRefresh();
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
