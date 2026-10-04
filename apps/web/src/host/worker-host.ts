import type {
  PublicReportPage,
  PublicReportQuery,
  PublicReportSummary,
  SessionSetup,
  Snapshot,
  StockCode,
  HistoricalStockData,
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
import { WorkerRequestScope } from "./worker-request.ts";
import { parseHistoricalStockData } from "./stock-history.ts";
import { parseInitialAllocation } from "./initial-allocation.ts";

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

export async function readWorkerSpeedMetrics(requests: WorkerRequestScope, requestId: number, currentGeneration: number): Promise<SpeedMetrics> {
  const response = await requests.request({ type: "speedMetrics", requestId, generation: currentGeneration }, "speedMetrics");
  return parseSpeedMetrics(response.metrics);
}

export async function readWorkerPlayerWorkingOrders(requests: WorkerRequestScope, requestId: number, currentGeneration: number): Promise<readonly PlayerWorkingOrder[]> {
  const response = await requests.request({ type: "playerWorkingOrders", requestId, generation: currentGeneration }, "playerWorkingOrders");
  return normalizePlayerWorkingOrders(response.orders);
}

export async function readWorkerStockHistory(requests: WorkerRequestScope, requestId: number, currentGeneration: number, code: StockCode): Promise<HistoricalStockData> {
  const response = await requests.request({ type: "stockHistory", requestId, generation: currentGeneration, code }, "stockHistory");
  return parseHistoricalStockData(response.data, code);
}

export async function requestWorkerIndicators(requests: WorkerRequestScope, requestId: number, currentGeneration: number, input: IndicatorInput): Promise<IndicatorResults> {
  const normalized = normalizeIndicatorInput(input);
  const response = await requests.request({ type: "calculateIndicators", requestId, generation: currentGeneration, ...normalized }, "indicatorsCalculated");
  return normalizeIndicatorResults(response.result, normalized);
}

export async function stepWorkerOnce(requests: WorkerRequestScope, requestId: number, currentGeneration: number): Promise<number> {
  const response = await requests.request({ type: "stepOnce", requestId, generation: currentGeneration }, "stepped");
  if (!Number.isSafeInteger(response.tick) || Number(response.tick) < 0) {
    throw new Error("Worker 单步响应的 tick 无效");
  }
  return Number(response.tick);
}

export async function restoreWorkerSlot(
  requests: WorkerRequestScope,
  slot: unknown,
  requestId: number,
  currentGeneration: number,
): Promise<{ readonly snapshot: Snapshot; readonly nextGeneration: number }> {
  const response = await requests.request({
    type: "restore",
    requestId,
    generation: currentGeneration,
    slot,
  }, "restored");
  const nextGeneration = generation(response.nextGeneration, "Worker restore generation");
  if (nextGeneration <= currentGeneration) {
    throw new Error(`Worker restore generation 必须大于当前 generation ${currentGeneration}，实际收到 ${nextGeneration}`);
  }
  return {
    snapshot: parseProtocolSnapshot(response.snapshot, "Worker restored.snapshot"),
    nextGeneration,
  };
}

export async function refreshWorkerBaseline(requests: WorkerRequestScope, requestId: number, currentGeneration: number): Promise<Snapshot> {
  const response = await requests.request({ type: "refreshBaseline", requestId, generation: currentGeneration }, "refreshed");
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
  /** 可选 Rayon pool 大小，不限制市场请求或任务数量。 */
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
    const requests = new WorkerRequestScope(worker);
    let callback: ((update: HostUpdate) => void | boolean) | null = null;
    let fatalCallback: ((failure: HostFailure) => void) | null = null;
    let cachedBaseline: Extract<HostUpdate, { type: "baseline" }> | null = null;
    let baselineEpoch = 0;
    let deliveredGeneration: string | null = null;
    let initialized = false;
    let disposed = false;
    let currentGeneration = 0;
    let npcDecisionDiagnostics = false;
    let pendingFailure: HostFailure | null = null;
    const timeout = setTimeout(() => {
      if (!initialized) {
        lifecycle.dispose();
        reject(new Error("WASM 多线程初始化超时（10s）。请检查 SharedArrayBuffer、COOP/COEP 和 WASM 线程绑定。"));
      }
    }, 10_000);

    const notifyFailure = (failure: HostFailure) => {
      if (disposed) return;
      disposed = true;
      callback = null;
      cachedBaseline = null;
      const reportFailure = fatalCallback;
      fatalCallback = null;
      requests.close(new Error(`${failure.code} @ ${failure.where}: ${failure.message}`));
      if (!initialized) {
        clearTimeout(timeout);
        lifecycle.dispose();
        reject(new Error(`${failure.code} @ ${failure.where}: ${failure.message}`));
        return;
      }
      lifecycle.dispose();
      if (reportFailure !== null) reportFailure(failure);
      else pendingFailure = failure;
    };

    const deliverLiveBaseline = (baseline: Extract<HostUpdate, { type: "baseline" }>): void => {
      deliveredGeneration = null;
      if (callback === null) return;
      try {
        if (callback(baseline) === false) throw new Error("Worker 消费者拒绝 baseline，会话已停止");
      } catch (error) {
        notifyFailure({ code: "WASM_WORKER_BASELINE_REJECTED", where: "worker-host.baseline", message: error instanceof Error ? error.message : String(error) });
        throw error;
      }
      deliveredGeneration = baseline.generation;
    };

    worker.addEventListener("error", (event) => {
      const failure = {
        code: "WASM_WORKER_SCRIPT",
        where: "worker-host",
        message: `WASM Worker 脚本加载或执行失败：${event.message || "浏览器未提供具体错误"}`,
      };
      notifyFailure(failure);
    });

    worker.addEventListener("message", (event: MessageEvent<unknown>) => {
      if (disposed) return;
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
              deliverLiveBaseline(next);
            }
            return;
          }
          case "protocol":
          case "protocolBatch": {
            const updateGeneration = generation(incoming.generation, "Worker protocol generation");
            if (updateGeneration !== currentGeneration || callback === null) return;
            const deliveryId = generation(incoming.deliveryId, "Worker protocol deliveryId");
            const updates = incoming.type === "protocol" ? [incoming.update] : incoming.updates;
            if (!Array.isArray(updates) || updates.length < 1 || updates.length > 64) throw new Error("Worker protocolBatch 必须包含 1–64 个完整提交");
            for (const update of updates) {
              const accepted = callback(createProtocolUpdate(String(updateGeneration), update, {
                civilDate: typeof incoming.civilDate === "string" ? incoming.civilDate : null,
                revision: typeof incoming.revision === "string" ? incoming.revision : null,
              }));
              if (accepted === false) {
                requests.close(new Error("Worker 消费者拒绝协议更新，会话已停止"));
                lifecycle.dispose();
                disposed = true;
                callback = null;
                return;
              }
            }
            worker.postMessage({ type: "uiFrame", generation: updateGeneration, deliveryId });
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

    async function control(type: string, successType: string, payload: Record<string, unknown> = {}): Promise<void> {
      const requestedGeneration = currentGeneration;
      await requests.request({ type, ...payload, requestId: requests.nextRequestId(), generation: requestedGeneration }, successType);
      if (disposed || currentGeneration !== requestedGeneration) throw new Error(`Worker ${type} 响应属于已过期 generation`);
    }

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
        async start(onUpdate, onFatalError) {
          if (disposed) throw new Error("WASM Worker 已被销毁");
          if (pendingFailure !== null) throw new Error(`${pendingFailure.code}: ${pendingFailure.message}`);
          callback = onUpdate;
          fatalCallback = onFatalError ?? null;
          if (cachedBaseline !== null && deliveredGeneration !== cachedBaseline.generation) {
            if (callback(cachedBaseline) === false) {
              callback = null;
              throw new Error("Worker 消费者拒绝 cached baseline，未启动 Worker loop");
            }
            deliveredGeneration = cachedBaseline.generation;
          }
          await control("start", "started");
        },
        async stop() {
          await control("stop", "stopped");
        },
        async dispose() {
          if (disposed) return;
          disposed = true;
          requests.close(new Error("WASM Worker 已被销毁，操作已取消"));
          callback = null;
          fatalCallback = null;
          cachedBaseline = null;
          lifecycle.dispose();
        },
        async setSpeed(multiplier) {
          assertValidSpeedMultiplier(multiplier);
          await control("setSpeed", "speedSet", { speed: multiplier });
        },
        async setPausePreferences(preferences: PausePreferences) {
          await control("setPausePreferences", "pausePreferencesSet", { preferences });
        },
        setFrameRate(fps) {
          worker.postMessage({ type: "setFrameRate", fps });
        },
        async readSpeedMetrics() {
          return readWorkerSpeedMetrics(requests, requests.nextRequestId(), currentGeneration);
        },
        async playerWorkingOrders() {
          if (cachedBaseline === null) throw new Error("Worker 基线尚未就绪，不能查询玩家活动委托");
          const queryGeneration = currentGeneration;
          const queryEpoch = baselineEpoch;
          const orders = await readWorkerPlayerWorkingOrders(requests, requests.nextRequestId(), queryGeneration);
          if (currentGeneration !== queryGeneration || baselineEpoch !== queryEpoch) throw new Error("Worker 玩家活动委托响应属于已过期会话 generation");
          return orders;
        },
        async queryStockHistory(code: StockCode): Promise<HistoricalStockData> {
          if (cachedBaseline === null) throw new Error("Worker 基线尚未就绪，不能查询股票历史");
          const queryGeneration = currentGeneration;
          const queryEpoch = baselineEpoch;
          const data = await readWorkerStockHistory(requests, requests.nextRequestId(), queryGeneration, code);
          if (currentGeneration !== queryGeneration || baselineEpoch !== queryEpoch) throw new Error("Worker 股票历史响应属于已过期会话 generation");
          return data;
        },
        async initialAllocation() {
          if (cachedBaseline === null) throw new Error("Worker 基线尚未就绪，不能查询初始分配");
          const queryGeneration = currentGeneration;
          const queryEpoch = baselineEpoch;
          const response = await requests.request({ type: "initialAllocation", requestId: requests.nextRequestId(), generation: queryGeneration }, "initialAllocation");
          if (disposed || currentGeneration !== queryGeneration || baselineEpoch !== queryEpoch) throw new Error("Worker 初始分配响应属于已过期会话 generation");
          return parseInitialAllocation(response.data, setup.stocks.map((stock) => stock.code));
        },
        async npcDecisionTrace(account: number): Promise<readonly NpcDecisionTraceRecord[]> {
          if (!npcDecisionDiagnostics) throw new Error("当前 WASM 后端未协商启用 NPC 决策诊断");
          if (!Number.isSafeInteger(account) || account < 0) throw new Error("NPC 账户 ID 必须是非负安全整数");
          const response = await requests.request({ type: "npcDecisionTrace", requestId: requests.nextRequestId(), generation: currentGeneration, account }, "npcDecisionTrace");
          return parseNpcDecisionTrace(response.records);
        },
        async calculateIndicators(input: IndicatorInput): Promise<IndicatorResults> {
          return requestWorkerIndicators(requests, requests.nextRequestId(), currentGeneration, input);
        },
        async stepOnceForE2E() {
          assertWorkerE2EStepAllowed(import.meta.env.MODE === "e2e", options.enableE2EStepping === true);
          return stepWorkerOnce(requests, requests.nextRequestId(), currentGeneration);
        },
        async submitIntent(intent) {
          await requests.request({ type: "enqueue", requestId: requests.nextRequestId(), generation: currentGeneration, intent }, "enqueued");
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
          const response = await requests.request({ type: "civilDate", requestId: requests.nextRequestId(), generation: currentGeneration }, "civilDate");
          if (typeof response.date !== "string") throw new Error("Worker 返回的自然日无效");
          return response.date;
        },
        async endCivilDay() {
          await requests.request({ type: "endCivilDay", requestId: requests.nextRequestId(), generation: currentGeneration }, "civilDayEnded");
        },
        async save(candidate?: { readonly seq: number; readonly settledDate: string }): Promise<unknown> {
          const queryGeneration = currentGeneration;
          const response = await requests.request(candidate === undefined
            ? { type: "save", requestId: requests.nextRequestId(), generation: queryGeneration }
            : { type: "save", requestId: requests.nextRequestId(), generation: queryGeneration, candidate }, "saved");
          if (disposed || currentGeneration !== queryGeneration) throw new Error("Worker 存档响应属于已过期会话 generation");
          return response.slot;
        },
        async refreshBaseline() {
          const requestedGeneration = currentGeneration;
          const snapshot = await refreshWorkerBaseline(requests, requests.nextRequestId(), requestedGeneration);
          if (disposed || currentGeneration !== requestedGeneration) throw new Error("Worker 基线刷新响应属于已过期会话 generation");
          const baseline = createBaselineUpdate(String(requestedGeneration), snapshot);
          baselineEpoch += 1;
          cachedBaseline = baseline;
          deliverLiveBaseline(baseline);
        },
        async load(slot) {
          const parsedSlot = parseSaveSlot(slot);
          const requestedGeneration = currentGeneration;
          const restored = await restoreWorkerSlot(requests, parsedSlot, requests.nextRequestId(), requestedGeneration);
          if (disposed || currentGeneration !== requestedGeneration && currentGeneration !== restored.nextGeneration) throw new Error("Worker restore 响应属于已过期 generation");
          currentGeneration = restored.nextGeneration;
          const baseline = createBaselineUpdate(String(restored.nextGeneration), restored.snapshot);
          baselineEpoch += 1;
          cachedBaseline = baseline;
          if (callback !== null && deliveredGeneration !== baseline.generation) {
            deliverLiveBaseline(baseline);
          }
        },
        async queryPublicReports(query: PublicReportQuery): Promise<PublicReportPage> {
          const response = await requests.request({ type: "publicReports", requestId: requests.nextRequestId(), generation: currentGeneration, query }, "publicReports");
          return normalizePublicReportPage(response.page);
        },
        async publicReportById(id: string): Promise<PublicReportSummary> {
          const response = await requests.request({ type: "publicReportById", requestId: requests.nextRequestId(), generation: currentGeneration, id }, "publicReportById");
          return normalizePublicReportById(response.report);
        },
      };
    }
  });
}

export const MAX_SPEED = Infinity;
