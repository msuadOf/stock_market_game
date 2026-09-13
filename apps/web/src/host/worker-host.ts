/**
 * WorkerHost：通过 Web Worker 调用多核 WASM engine。
 *
 * Worker 内部：init wasm → initThreadPool(N核) → create session → 帧循环。
 * 主线程：被动接收 events/snapshot，rAF 渲染。
 *
 * 帧率协商：主线程告诉 Worker 它的观测目标（16ms，约 62.5Hz），
 * Worker 按此频率 flush 事件 → 不超频推送。
 */
import type {
  Intent,
  PublicReportPage,
  PublicReportQuery,
  PublicReportSummary,
  SaveSlot,
  SessionSetup,
  Snapshot,
} from "../types/engine";
import { parseSaveSlot } from "../save/save-schema.ts";
import type { EngineHost } from "./engine-host";
import type { HostFailure, HostUpdate } from "./host-update.ts";
import { UI_TARGET_HZ, createBaselineUpdate } from "./host-update.ts";
import { createWorkerLifecycle, routeWorkerFailure } from "./worker-lifecycle.ts";
import { requestWorker, type WorkerRequestPort } from "./worker-request.ts";
import { assertValidSpeedMultiplier, parseSpeedMetrics } from "./speed.ts";

interface WorkerMsg {
  type: string;
  [key: string]: unknown;
}

/** 通过 Worker 请求/响应协议读取实际倍率，并在跨线程边界执行统一校验。 */
export async function readWorkerSpeedMetrics(
  worker: WorkerRequestPort,
  requestId: number,
  generation: number,
) {
  const response = await requestWorker(
    worker,
    { type: "speedMetrics", requestId, generation },
    "speedMetrics",
  );
  return parseSpeedMetrics(response.metrics);
}

/**
 * 暂停 Worker 并原子恢复存档；只恢复调用前已经运行的循环。
 *
 * 校验失败或请求超时也必须恢复原运行状态，否则一次失败的读档会意外改变游戏状态。
 */
export async function restoreWorkerSlot(
  worker: WorkerRequestPort,
  slot: unknown,
  requestId: number,
  wasRunning: boolean,
  generation: number,
): Promise<{ snapshot: Snapshot; nextGeneration: number }> {
  worker.postMessage({ type: "stop" });
  try {
    const response = await requestWorker(
      worker,
      { type: "restore", requestId, generation, slot },
      "restored",
    );
    if (!Number.isSafeInteger(response.nextGeneration) || (response.nextGeneration as number) <= generation) {
      throw new Error("Worker 恢复响应缺少递增的新会话 generation");
    }
    return {
      snapshot: response.snapshot as Snapshot,
      nextGeneration: response.nextGeneration as number,
    };
  } finally {
    if (wasRunning) worker.postMessage({ type: "start" });
  }
}

export function createWorkerHost(setup: SessionSetup, seed: bigint): Promise<EngineHost> {
  return new Promise((resolve, reject) => {
    const worker = new Worker(new URL("./wasm-worker.ts", import.meta.url), { type: "module" });
    const lifecycle = createWorkerLifecycle(worker);
    let onUpdate: ((update: HostUpdate) => void) | null = null;
    let onFatalError: ((failure: HostFailure) => void) | null = null;
    let cachedSnapshot: Snapshot | null = null;
    let initialized = false;
    let pendingFatalError: string | null = null;
    let requestSequence = 0;
    let generation = 0;
    let running = false;
    let baselineDelivered = false;
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
          if (onFatalError) onFatalError({ code: "WASM_WORKER_SCRIPT", message: runtimeMessage });
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
          generation = msg.generation as number;
          // 首张快照会在 create 后由 worker 主动推送
          break;
        case "snapshot":
          cachedSnapshot = msg.snapshot as Snapshot;
          if (initialized && onUpdate) onUpdate(createBaselineUpdate(cachedSnapshot));
          if (!initialized) {
            initialized = true;
            clearTimeout(timeout);
            resolve(makeHost());
          }
          break;
        case "hostUpdate":
          if ((msg.update as HostUpdate).type === "delta"
            && (msg.update as Extract<HostUpdate, { type: "delta" }>).runtimeSnapshot) {
            cachedSnapshot = (msg.update as Extract<HostUpdate, { type: "delta" }>).runtimeSnapshot!;
          }
          if (onUpdate) {
            onUpdate(msg.update as HostUpdate);
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
              if (onFatalError) onFatalError({ code: "WASM_WORKER_RUNTIME", message: runtimeMessage });
              else pendingFatalError = runtimeMessage;
            },
          });
          break;
      }
    });

    // 启动初始化
    worker.postMessage({ type: "init" });

    function makeHost(): EngineHost {
      // 三宿主共享 16ms（约 62.5Hz）观测目标；浏览器实际绘制仍由 rAF 决定。
      worker.postMessage({ type: "setFrameRate", fps: UI_TARGET_HZ });

      return {
        capabilities: {
          deliveryModes: [],
          targetUiHz: UI_TARGET_HZ,
          sharedMemory: true,
          reconnect: false,
          publicCompanyReports: true,
          npcDecisionDiagnostics: false,
        },
        start(updateCb, fatalCb) {
          if (pendingFatalError) throw new Error(pendingFatalError);
          onUpdate = updateCb;
          if (fatalCb) onFatalError = fatalCb;
          if (cachedSnapshot && !baselineDelivered) {
            onUpdate(createBaselineUpdate(cachedSnapshot));
            baselineDelivered = true;
          }
          worker.postMessage({ type: "start" });
          running = true;
        },
        stop() {
          lifecycle.pause();
          running = false;
        },
        dispose() {
          onUpdate = null;
          onFatalError = null;
          cachedSnapshot = null;
          running = false;
          lifecycle.dispose();
        },
        setSpeed(x: number) {
          assertValidSpeedMultiplier(x);
          worker.postMessage({ type: "setSpeed", speed: x });
        },
        setFrameRate(fps: number) {
          worker.postMessage({ type: "setFrameRate", fps });
        },
        async readSpeedMetrics() {
          const requestId = ++requestSequence;
          return readWorkerSpeedMetrics(worker, requestId, generation);
        },
        async submitIntent(intent: Intent) {
          const requestId = ++requestSequence;
          await requestWorker(worker, { type: "enqueue", requestId, generation, intent }, "enqueued");
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
        async civilDate(): Promise<string> {
          const requestId = ++requestSequence;
          const response = await requestWorker(
            worker,
            { type: "civilDate", requestId, generation },
            "civilDate",
          );
          if (typeof response.date !== "string") throw new Error("Worker 返回的自然日无效");
          return response.date;
        },
        async endCivilDay(): Promise<void> {
          const requestId = ++requestSequence;
          await requestWorker(
            worker,
            { type: "endCivilDay", requestId, generation },
            "civilDayEnded",
          );
        },
        save(): Promise<SaveSlot> {
          const requestId = ++requestSequence;
          return requestWorker(worker, { type: "save", requestId, generation }, "saved")
            .then((response) => response.slot as SaveSlot);
        },
        async load(slot: unknown) {
          const parsed = parseSaveSlot(slot);
          const requestId = ++requestSequence;
          const restored = await restoreWorkerSlot(worker, parsed, requestId, running, generation);
          cachedSnapshot = restored.snapshot;
          generation = restored.nextGeneration;
          onUpdate?.(createBaselineUpdate(cachedSnapshot));
        },
        async queryPublicReports(query: PublicReportQuery): Promise<PublicReportPage> {
          const requestId = ++requestSequence;
          const response = await requestWorker(
            worker,
            { type: "publicReports", requestId, generation, query },
            "publicReports",
          );
          return response.page as PublicReportPage;
        },
        async publicReportById(id: string): Promise<PublicReportSummary> {
          const requestId = ++requestSequence;
          const response = await requestWorker(
            worker,
            { type: "publicReportById", requestId, generation, id },
            "publicReportById",
          );
          return response.report as PublicReportSummary;
        },
      };
    }
  });
}

export const MAX_SPEED = Infinity;
