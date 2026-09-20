import type {
  PublicReportPage,
  PublicReportQuery,
  PublicReportSummary,
  SessionSetup,
  Snapshot,
} from "../types/engine.ts";
import type { PausePreferences } from "../types/generated/PausePreferences.ts";
import { parseSaveSlot } from "../save/save-schema.ts";
import type { EngineHost, SpeedMetrics } from "./engine-host.ts";
import { createBaselineUpdate, createProtocolUpdate, type HostFailure, type HostUpdate, UI_TARGET_HZ } from "./host-update.ts";
import { parseProtocolSnapshot } from "./protocol/index.ts";
import { normalizePublicReportById, normalizePublicReportPage } from "./serde-normalize.ts";
import { assertValidSpeedMultiplier, parseSpeedMetrics } from "./speed.ts";
import { createWorkerLifecycle } from "./worker-lifecycle.ts";
import { requestWorker, type WorkerRequestPort } from "./worker-request.ts";

type WorkerMessage = {
  readonly type: string;
  readonly [key: string]: unknown;
};

function record(value: unknown, where: string): Readonly<Record<string, unknown>> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${where} 必须是对象`);
  }
  return value as Readonly<Record<string, unknown>>;
}

function message(value: unknown): WorkerMessage {
  const parsed = record(value, "Worker 消息");
  if (typeof parsed.type !== "string") throw new Error("Worker 消息缺少 type");
  return parsed as WorkerMessage;
}

function generation(value: unknown, where: string): number {
  if (!Number.isSafeInteger(value) || Number(value) < 1) {
    throw new Error(`${where} 必须是正安全整数`);
  }
  return Number(value);
}

function workerFailure(value: WorkerMessage): HostFailure {
  if (typeof value.code !== "string" || typeof value.where !== "string" || typeof value.message !== "string") {
    throw new Error("Worker failure 载荷无效");
  }
  return { code: value.code, where: value.where, message: value.message };
}

export async function readWorkerSpeedMetrics(worker: WorkerRequestPort, requestId: number, currentGeneration: number): Promise<SpeedMetrics> {
  const response = await requestWorker(worker, { type: "speedMetrics", requestId, generation: currentGeneration }, "speedMetrics");
  return parseSpeedMetrics(response.metrics);
}

export async function restoreWorkerSlot(
  worker: WorkerRequestPort,
  slot: unknown,
  requestId: number,
  currentGeneration: number,
): Promise<{ readonly snapshot: Snapshot; readonly nextGeneration: number }> {
  const response = await requestWorker(worker, {
    type: "restore",
    requestId,
    generation: currentGeneration,
    slot,
  }, "restored");
  return {
    snapshot: parseProtocolSnapshot(response.snapshot, "Worker restored.snapshot"),
    nextGeneration: generation(response.nextGeneration, "Worker restore generation"),
  };
}

export function workerPausePreferenceRequest(
  requestId: number,
  currentGeneration: number,
  preferences: PausePreferences,
): { readonly type: "setPausePreferences"; readonly requestId: number; readonly generation: number; readonly preferences: PausePreferences } {
  return { type: "setPausePreferences", requestId, generation: currentGeneration, preferences };
}

export function createWorkerHost(setup: SessionSetup, seed: bigint): Promise<EngineHost> {
  return new Promise((resolve, reject) => {
    const worker = new Worker(new URL("./wasm-worker.ts", import.meta.url), { type: "module" });
    const lifecycle = createWorkerLifecycle(worker);
    let callback: ((update: HostUpdate) => void) | null = null;
    let fatalCallback: ((failure: HostFailure) => void) | null = null;
    let cachedBaseline: Extract<HostUpdate, { type: "baseline" }> | null = null;
    let deliveredGeneration: string | null = null;
    let initialized = false;
    let disposed = false;
    let currentGeneration = 0;
    let requestSequence = 0;
    let pendingFailure: HostFailure | null = null;
    const timeout = setTimeout(() => {
      if (!initialized) {
        lifecycle.dispose();
        reject(new Error("WASM 多线程初始化超时（10s）。请检查 SharedArrayBuffer、COOP/COEP 和 WASM 线程绑定。"));
      }
    }, 10_000);

    const notifyFailure = (failure: HostFailure) => {
      lifecycle.dispose();
      if (fatalCallback !== null) fatalCallback(failure);
      else pendingFailure = failure;
    };

    worker.addEventListener("error", (event) => {
      const failure = {
        code: "WASM_WORKER_SCRIPT",
        where: "worker-host",
        message: `WASM Worker 脚本加载或执行失败：${event.message || "浏览器未提供具体错误"}`,
      };
      if (!initialized) {
        clearTimeout(timeout);
        lifecycle.dispose();
        reject(new Error(failure.message));
        return;
      }
      notifyFailure(failure);
    });

    worker.addEventListener("message", (event: MessageEvent<unknown>) => {
      try {
        const incoming = message(event.data);
        switch (incoming.type) {
          case "ready":
            worker.postMessage({ type: "create", setup, seed });
            return;
          case "created":
            currentGeneration = generation(incoming.generation, "Worker created generation");
            return;
          case "baseline": {
            const nextGeneration = generation(incoming.generation, "Worker baseline generation");
            if (nextGeneration < currentGeneration) return;
            currentGeneration = nextGeneration;
            const next = createBaselineUpdate(String(nextGeneration), parseProtocolSnapshot(incoming.snapshot, "Worker baseline.snapshot"));
            cachedBaseline = next;
            if (!initialized) {
              initialized = true;
              clearTimeout(timeout);
              resolve(host());
            }
            if (callback !== null && deliveredGeneration !== next.generation) {
              callback(next);
              deliveredGeneration = next.generation;
            }
            return;
          }
          case "protocol": {
            const updateGeneration = generation(incoming.generation, "Worker protocol generation");
            if (updateGeneration !== currentGeneration || callback === null) return;
            callback(createProtocolUpdate(String(updateGeneration), incoming.update, {
              civilDate: typeof incoming.civilDate === "string" ? incoming.civilDate : null,
              revision: typeof incoming.revision === "string" ? incoming.revision : null,
            }));
            return;
          }
          case "failure":
            notifyFailure(workerFailure(incoming));
            return;
          case "barrierPaused":
            generation(incoming.generation, "Worker barrier generation");
            return;
          default:
            return;
        }
      } catch (error) {
        notifyFailure({
          code: "WASM_WORKER_TRANSPORT",
          where: "worker-host.message",
          message: error instanceof Error ? error.message : String(error),
        });
      }
    });

    worker.postMessage({ type: "init" });

    function host(): EngineHost {
      return {
        capabilities: {
          deliveryModes: [],
          targetUiHz: UI_TARGET_HZ,
          sharedMemory: true,
          reconnect: false,
          publicCompanyReports: true,
          npcDecisionDiagnostics: false,
        },
        start(onUpdate, onFatalError) {
          if (disposed) throw new Error("WASM Worker 已被销毁");
          if (pendingFailure !== null) throw new Error(`${pendingFailure.code}: ${pendingFailure.message}`);
          callback = onUpdate;
          fatalCallback = onFatalError ?? null;
          if (cachedBaseline !== null && deliveredGeneration !== cachedBaseline.generation) {
            callback(cachedBaseline);
            deliveredGeneration = cachedBaseline.generation;
          }
          worker.postMessage({ type: "start" });
        },
        stop() {
          lifecycle.pause();
        },
        dispose() {
          if (disposed) return;
          disposed = true;
          callback = null;
          fatalCallback = null;
          cachedBaseline = null;
          lifecycle.dispose();
        },
        setSpeed(multiplier) {
          assertValidSpeedMultiplier(multiplier);
          worker.postMessage({ type: "setSpeed", speed: multiplier });
        },
        async setPausePreferences(preferences: PausePreferences) {
          const requestId = ++requestSequence;
          await requestWorker(worker, workerPausePreferenceRequest(requestId, currentGeneration, preferences), "pausePreferencesSet");
        },
        setFrameRate(fps) {
          worker.postMessage({ type: "setFrameRate", fps });
        },
        async readSpeedMetrics() {
          return readWorkerSpeedMetrics(worker, ++requestSequence, currentGeneration);
        },
        async submitIntent(intent) {
          await requestWorker(worker, { type: "enqueue", requestId: ++requestSequence, generation: currentGeneration, intent }, "enqueued");
        },
        snapshot() {
          if (cachedBaseline === null) throw new Error("快照尚未就绪");
          return cachedBaseline.snapshot;
        },
        tick() {
          if (cachedBaseline === null) throw new Error("快照尚未就绪");
          return cachedBaseline.snapshot.tick;
        },
        day() {
          if (cachedBaseline === null) throw new Error("快照尚未就绪");
          return cachedBaseline.snapshot.day;
        },
        async civilDate() {
          const response = await requestWorker(worker, { type: "civilDate", requestId: ++requestSequence, generation: currentGeneration }, "civilDate");
          if (typeof response.date !== "string") throw new Error("Worker 返回的自然日无效");
          return response.date;
        },
        async endCivilDay() {
          await requestWorker(worker, { type: "endCivilDay", requestId: ++requestSequence, generation: currentGeneration }, "civilDayEnded");
        },
        async save(): Promise<unknown> {
          const response = await requestWorker(worker, { type: "save", requestId: ++requestSequence, generation: currentGeneration }, "saved");
          return response.slot;
        },
        async load(slot) {
          const restored = await restoreWorkerSlot(worker, parseSaveSlot(slot), ++requestSequence, currentGeneration);
          currentGeneration = restored.nextGeneration;
          const baseline = createBaselineUpdate(String(restored.nextGeneration), restored.snapshot);
          cachedBaseline = baseline;
          if (callback !== null && deliveredGeneration !== baseline.generation) {
            callback(baseline);
            deliveredGeneration = baseline.generation;
          }
        },
        async queryPublicReports(query: PublicReportQuery): Promise<PublicReportPage> {
          const response = await requestWorker(worker, { type: "publicReports", requestId: ++requestSequence, generation: currentGeneration, query }, "publicReports");
          return normalizePublicReportPage(response.page);
        },
        async publicReportById(id: string): Promise<PublicReportSummary> {
          const response = await requestWorker(worker, { type: "publicReportById", requestId: ++requestSequence, generation: currentGeneration, id }, "publicReportById");
          return normalizePublicReportById(response.report);
        },
      };
    }
  });
}

export const MAX_SPEED = Infinity;
