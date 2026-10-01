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
import { normalizePlayerWorkingOrders, type PlayerWorkingOrder } from "./player-working-orders.ts";
import { parseHostFailure } from "./protocol-failure.ts";
import { parseNpcDecisionTrace, type NpcDecisionTraceRecord } from "./npc-decision-trace.ts";
import type { IndicatorInput, IndicatorResults } from "../components/indicator-results.ts";
import { normalizeIndicatorInput, normalizeIndicatorResults } from "./indicator-transport.ts";
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

export function parseWorkerFailure(value: unknown): HostFailure {
  const source = record(value, "Worker failure");
  const allowed = ["code", "where", "message", "cause", "context", "recoverable", "recoveryActions"];
  const fields = Object.fromEntries(Object.entries(source).filter(([key]) => allowed.includes(key)));
  return parseHostFailure(fields, "worker-host.message");
}

export async function readWorkerSpeedMetrics(worker: WorkerRequestPort, requestId: number, currentGeneration: number): Promise<SpeedMetrics> {
  const response = await requestWorker(worker, { type: "speedMetrics", requestId, generation: currentGeneration }, "speedMetrics");
  return parseSpeedMetrics(response.metrics);
}

export async function readWorkerPlayerWorkingOrders(worker: WorkerRequestPort, requestId: number, currentGeneration: number): Promise<readonly PlayerWorkingOrder[]> {
  const response = await requestWorker(worker, { type: "playerWorkingOrders", requestId, generation: currentGeneration }, "playerWorkingOrders");
  return normalizePlayerWorkingOrders(response.orders);
}

export async function requestWorkerIndicators(worker: WorkerRequestPort, requestId: number, currentGeneration: number, input: IndicatorInput): Promise<IndicatorResults> {
  const normalized = normalizeIndicatorInput(input);
  const response = await requestWorker(worker, { type: "calculateIndicators", requestId, generation: currentGeneration, ...normalized }, "indicatorsCalculated");
  return normalizeIndicatorResults(response.result, normalized);
}

export async function stepWorkerOnce(worker: WorkerRequestPort, requestId: number, currentGeneration: number): Promise<number> {
  const response = await requestWorker(worker, { type: "stepOnce", requestId, generation: currentGeneration }, "stepped");
  if (!Number.isSafeInteger(response.tick) || Number(response.tick) < 0) {
    throw new Error("Worker 单步响应的 tick 无效");
  }
  return Number(response.tick);
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

export async function refreshWorkerBaseline(worker: WorkerRequestPort, requestId: number, currentGeneration: number): Promise<Snapshot> {
  const response = await requestWorker(worker, { type: "refreshBaseline", requestId, generation: currentGeneration }, "refreshed");
  return parseProtocolSnapshot(response.snapshot, "Worker refreshed.snapshot");
}

export function workerPausePreferenceRequest(
  requestId: number,
  currentGeneration: number,
  preferences: PausePreferences,
): { readonly type: "setPausePreferences"; readonly requestId: number; readonly generation: number; readonly preferences: PausePreferences } {
  return { type: "setPausePreferences", requestId, generation: currentGeneration, preferences };
}

export interface WorkerE2EHost {
  stepOnceForE2E(): Promise<number>;
}

export function assertWorkerE2EStepAllowed(e2eBuild: boolean, injectedCapability: boolean): void {
  if (!e2eBuild) throw new Error("Worker 受控单步只允许在 E2E 构建中调用");
  if (!injectedCapability) throw new Error("Worker 受控单步 capability 未注入");
}

interface WorkerHostOptions {
  readonly enableE2EStepping?: boolean;
  /** Optional Rayon pool size. It does not cap market requests or tasks. */
  readonly threadCount?: number;
}

export function createWorkerHost(
  setup: SessionSetup,
  seed: bigint,
  options: WorkerHostOptions = {},
): Promise<EngineHost & WorkerE2EHost> {
  return new Promise((resolve, reject) => {
    const worker = new Worker(new URL("./wasm-worker.ts", import.meta.url), { type: "module" });
    const lifecycle = createWorkerLifecycle(worker);
    let callback: ((update: HostUpdate) => void) | null = null;
    let fatalCallback: ((failure: HostFailure) => void) | null = null;
    let cachedBaseline: Extract<HostUpdate, { type: "baseline" }> | null = null;
    let baselineEpoch = 0;
    let deliveredGeneration: string | null = null;
    let initialized = false;
    let disposed = false;
    let currentGeneration = 0;
    let npcDecisionDiagnostics = false;
    let requestSequence = 0;
    let pendingFailure: HostFailure | null = null;
    const timeout = setTimeout(() => {
      if (!initialized) {
        lifecycle.dispose();
        reject(new Error("WASM 多线程初始化超时（10s）。请检查 SharedArrayBuffer、COOP/COEP 和 WASM 线程绑定。"));
      }
    }, 10_000);

    const notifyFailure = (failure: HostFailure) => {
      if (!initialized) {
        clearTimeout(timeout);
        lifecycle.dispose();
        reject(new Error(`${failure.code} @ ${failure.where}: ${failure.message}`));
        return;
      }
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
            {
              const workerCapabilities = record(incoming.capabilities, "Worker capabilities");
              if (Object.keys(workerCapabilities).length !== 1 || typeof workerCapabilities.npcDecisionDiagnostics !== "boolean") {
                throw new Error("Worker capabilities 契约无效");
              }
              npcDecisionDiagnostics = workerCapabilities.npcDecisionDiagnostics;
            }
            return;
          case "baseline": {
            const nextGeneration = generation(incoming.generation, "Worker baseline generation");
            if (nextGeneration < currentGeneration) return;
            currentGeneration = nextGeneration;
            const next = createBaselineUpdate(String(nextGeneration), parseProtocolSnapshot(incoming.snapshot, "Worker baseline.snapshot"));
            if (deliveredGeneration === next.generation) return;
            baselineEpoch += 1;
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
            notifyFailure(parseWorkerFailure(incoming));
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

    worker.postMessage(options.threadCount === undefined
      ? { type: "init" }
      : { type: "init", threads: options.threadCount });

    function host(): EngineHost & WorkerE2EHost {
      return {
        capabilities: {
          deliveryModes: [],
          targetUiHz: UI_TARGET_HZ,
          sharedMemory: true,
          reconnect: false,
          publicCompanyReports: true,
          npcDecisionDiagnostics,
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
        async playerWorkingOrders() {
          if (cachedBaseline === null) throw new Error("Worker 基线尚未就绪，不能查询玩家活动委托");
          const queryGeneration = currentGeneration;
          const queryEpoch = baselineEpoch;
          const orders = await readWorkerPlayerWorkingOrders(worker, ++requestSequence, queryGeneration);
          if (currentGeneration !== queryGeneration || baselineEpoch !== queryEpoch) throw new Error("Worker 玩家活动委托响应属于已过期会话 generation");
          return orders;
        },
        async npcDecisionTrace(account: number): Promise<readonly NpcDecisionTraceRecord[]> {
          if (!npcDecisionDiagnostics) throw new Error("当前 WASM 后端未协商启用 NPC 决策诊断");
          if (!Number.isSafeInteger(account) || account < 0) throw new Error("NPC 账户 ID 必须是非负安全整数");
          const response = await requestWorker(worker, { type: "npcDecisionTrace", requestId: ++requestSequence, generation: currentGeneration, account }, "npcDecisionTrace");
          return parseNpcDecisionTrace(response.records);
        },
        async calculateIndicators(input: IndicatorInput): Promise<IndicatorResults> {
          return requestWorkerIndicators(worker, ++requestSequence, currentGeneration, input);
        },
        async stepOnceForE2E() {
          assertWorkerE2EStepAllowed(import.meta.env.MODE === "e2e", options.enableE2EStepping === true);
          return stepWorkerOnce(worker, ++requestSequence, currentGeneration);
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
        async save(candidate?: { readonly seq: number; readonly settledDate: string }): Promise<unknown> {
          const queryGeneration = currentGeneration;
          const response = await requestWorker(worker, candidate === undefined
            ? { type: "save", requestId: ++requestSequence, generation: queryGeneration }
            : { type: "save", requestId: ++requestSequence, generation: queryGeneration, candidate }, "saved");
          if (disposed || currentGeneration !== queryGeneration) throw new Error("Worker 存档响应属于已过期会话 generation");
          return response.slot;
        },
        async refreshBaseline() {
          const requestedGeneration = currentGeneration;
          const snapshot = await refreshWorkerBaseline(worker, ++requestSequence, requestedGeneration);
          if (currentGeneration !== requestedGeneration) throw new Error("Worker 基线刷新响应属于已过期会话 generation");
          const baseline = createBaselineUpdate(String(requestedGeneration), snapshot);
          baselineEpoch += 1;
          cachedBaseline = baseline;
          callback?.(baseline);
          deliveredGeneration = baseline.generation;
        },
        async load(slot) {
          const parsedSlot = parseSaveSlot(slot);
          const restored = await restoreWorkerSlot(worker, parsedSlot, ++requestSequence, currentGeneration);
          currentGeneration = restored.nextGeneration;
          const baseline = createBaselineUpdate(String(restored.nextGeneration), restored.snapshot);
          baselineEpoch += 1;
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
