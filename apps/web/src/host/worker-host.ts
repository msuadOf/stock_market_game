/**
 * WorkerHost：通过 Web Worker 调用多核 WASM engine。
 *
 * Worker 内部：init wasm → initThreadPool(N核) → create session → 帧循环。
 * 主线程：被动接收 events/snapshot，rAF 渲染。
 *
 * 帧率协商：主线程告诉 Worker 它的渲染帧率（rAF 自然 60fps 或目标 30fps），
 * Worker 按此频率 flush 事件 → 不超频推送。
 */
import type { EngineEvent, Intent, SaveSlot, SessionSetup, Snapshot } from "../types/engine";
import type { EngineHost } from "./engine-host";
import { createWorkerLifecycle, routeWorkerFailure } from "./worker-lifecycle.ts";
import { requestWorker, type WorkerRequestPort } from "./worker-request.ts";

interface WorkerMsg {
  type: string;
  [key: string]: unknown;
}

/**
 * 暂停 Worker 并原子恢复存档；只恢复调用前已经运行的循环。
 *
 * 校验失败或请求超时也必须恢复原运行状态，否则一次失败的读档会意外改变游戏状态。
 */
export async function restoreWorkerSlot(
  worker: WorkerRequestPort,
  slot: SaveSlot,
  requestId: number,
  wasRunning: boolean,
): Promise<Snapshot> {
  worker.postMessage({ type: "stop" });
  try {
    const response = await requestWorker(
      worker,
      { type: "restore", requestId, slot },
      "restored",
    );
    return response.snapshot as Snapshot;
  } finally {
    if (wasRunning) worker.postMessage({ type: "start" });
  }
}

export function createWorkerHost(setup: SessionSetup, seed: bigint): Promise<EngineHost> {
  return new Promise((resolve, reject) => {
    const worker = new Worker(new URL("./wasm-worker.ts", import.meta.url), { type: "module" });
    const lifecycle = createWorkerLifecycle(worker);
    let onEvents: ((events: EngineEvent[]) => void) | null = null;
    let onSnapshot: ((snapshot: Snapshot) => void) | null = null;
    let onFatalError: ((message: string) => void) | null = null;
    let cachedSnapshot: Snapshot | null = null;
    let initialized = false;
    let pendingFatalError: string | null = null;
    let requestSequence = 0;
    let running = false;
    const failInitialization = (message: string) => {
      if (initialized) return;
      clearTimeout(timeout);
      lifecycle.dispose();
      reject(new Error(message));
    };
    const timeout = setTimeout(() => {
      failInitialization(
        "WASM 多线程初始化超时（10s）。请检查 SharedArrayBuffer、COOP/COEP 响应头、wasm atomics 和 worker 脚本加载状态。",
      );
    }, 10000);

    worker.addEventListener("error", (event) => {
      const message = `WASM Worker 脚本加载或执行失败：${event.message || "浏览器未提供具体错误"}`;
      routeWorkerFailure(initialized, message, {
        initialization: failInitialization,
        runtime(runtimeMessage) {
          lifecycle.dispose();
          if (onFatalError) onFatalError(runtimeMessage);
          else pendingFatalError = runtimeMessage;
        },
      });
    });

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
          if (initialized && onSnapshot) onSnapshot(cachedSnapshot);
          if (!initialized) {
            initialized = true;
            clearTimeout(timeout);
            resolve(makeHost());
          }
          break;
        case "events":
          if (onEvents) {
            onEvents(msg.events as EngineEvent[]);
            // 在下一次浏览器绘制前不让 Worker 继续堆积事件。rAF 回调后浏览器会
            // 立即进入布局/绘制，Worker 与绘制可并行继续下一批计算。
            requestAnimationFrame(() => worker.postMessage({ type: "uiFrame" }));
          } else {
            worker.postMessage({ type: "uiFrame" });
          }
          break;
        case "error":
          console.error("[WorkerHost]", msg.message);
          routeWorkerFailure(initialized, String(msg.message), {
            initialization: failInitialization,
            runtime(runtimeMessage) {
              lifecycle.dispose();
              if (onFatalError) onFatalError(runtimeMessage);
              else pendingFatalError = runtimeMessage;
            },
          });
          break;
      }
    });

    // 启动初始化
    worker.postMessage({ type: "init" });

    function makeHost(): EngineHost {
      // 告诉 Worker 当前的渲染帧率（默认 30fps，主线程可改）
      worker.postMessage({ type: "setFrameRate", fps: 30 });

      return {
        start(cb, snapshotCb, fatalCb) {
          if (pendingFatalError) throw new Error(pendingFatalError);
          onEvents = cb;
          if (snapshotCb) onSnapshot = snapshotCb;
          if (fatalCb) onFatalError = fatalCb;
          worker.postMessage({ type: "start" });
          running = true;
        },
        stop() {
          lifecycle.pause();
          running = false;
        },
        dispose() {
          onEvents = null;
          onSnapshot = null;
          onFatalError = null;
          cachedSnapshot = null;
          running = false;
          lifecycle.dispose();
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
        save(): Promise<SaveSlot> {
          const requestId = ++requestSequence;
          return requestWorker(worker, { type: "save", requestId }, "saved")
            .then((response) => response.slot as SaveSlot);
        },
        async load(slot: SaveSlot) {
          const requestId = ++requestSequence;
          cachedSnapshot = await restoreWorkerSlot(worker, slot, requestId, running);
          onSnapshot?.(cachedSnapshot);
        },
      };
    }
  });
}

export const MAX_SPEED = Infinity;
