/**
 * WorkerHost：通过 Web Worker 间接调用 WASM engine。
 *
 * 主线程不直接碰 wasm；所有调用经 postMessage 发给 Worker，异步等回包。
 * 不阻塞 UI 渲染（720x 高速时关键）。
 */
import type { EngineEvent, Intent, SessionSetup, Snapshot } from "../types/engine";
import type { EngineHost } from "./wasm-host";

interface WorkerMsg {
  type: string;
  [key: string]: unknown;
}

export function createWorkerHost(setup: SessionSetup, seed: bigint): EngineHost {
  const worker = new Worker(new URL("./wasm-worker.ts", import.meta.url), { type: "module" });
  let onEvents: ((events: EngineEvent[]) => void) | null = null;
  let cachedSnapshot: Snapshot | null = null;

  // 消息路由
  worker.addEventListener("message", (e: MessageEvent) => {
    const msg = e.data as WorkerMsg;
    switch (msg.type) {
      case "ready":
        // 自动 create
        worker.postMessage({ type: "create", setup, seed });
        break;
      case "created":
        // 拉一张初始快照缓存
        worker.postMessage({ type: "snapshot" });
        break;
      case "events":
        if (onEvents) onEvents(msg.events as EngineEvent[]);
        break;
      case "snapshot":
        cachedSnapshot = msg.snapshot as Snapshot;
        break;
      case "error":
        console.error("[WorkerHost]", msg.message);
        break;
    }
  });

  // 初始化 Worker
  worker.postMessage({ type: "init" });

  /** 同步返回缓存快照；若尚未收到则抛（UI 应在 ready 后才调用）。 */
  function requireSnapshot(): Snapshot {
    if (!cachedSnapshot) throw new Error("快照尚未就绪");
    return cachedSnapshot;
  }

  return {
    start(cb) {
      onEvents = cb;
      // 轮询：定期拉快照刷新缓存（主线程读快照是同步的，需缓存）
      worker.postMessage({ type: "start" });
    },
    stop() {
      worker.postMessage({ type: "stop" });
    },
    setSpeed(x: number) {
      if (x <= 0) throw new Error(`非法速度倍率：${x}`);
      worker.postMessage({ type: "setSpeed", speed: x });
      // 速度变更后拉一次新快照
      setTimeout(() => worker.postMessage({ type: "snapshot" }), 100);
    },
    submitIntent(intent: Intent) {
      worker.postMessage({ type: "enqueue", intent });
    },
    snapshot(): Snapshot {
      // 异步触发刷新；返回上次缓存
      worker.postMessage({ type: "snapshot" });
      return requireSnapshot();
    },
    tick(): number {
      return requireSnapshot().tick;
    },
    day(): number {
      return requireSnapshot().day;
    },
  };
}

/** 等待 Worker 就绪的 Promise 包装（App.tsx 初始化用）。 */
export function createWorkerHostAsync(setup: SessionSetup, seed: bigint): Promise<EngineHost> {
  return new Promise((resolve, reject) => {
    const host = createWorkerHost(setup, seed);
    // 轮询就绪状态（Worker 异步 init + create）
    let attempts = 0;
    const check = setInterval(() => {
      attempts++;
      if (attempts > 50) {
        clearInterval(check);
        reject(new Error("Worker 初始化超时（5s）"));
      }
      try {
        // 如果 snapshot 不抛，说明 created + 至少一张快照到了
        host.snapshot();
        clearInterval(check);
        resolve(host);
      } catch {
        // 尚未就绪，继续等
      }
    }, 100);
  });
}
