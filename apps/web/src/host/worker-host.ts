import type {
  PublicReportPage,
  PublicReportAvailability,
  PublicReportAvailabilityQuery,
  PublicReportQuery,
  PublicReportSummary,
  SessionSetup,
  Snapshot,
  StockCode,
  HistoricalStockData,
} from "../types/engine.ts";
import type { PausePreferences } from "../types/generated/PausePreferences.ts";
import { parseSaveSlot } from "../save/save-schema.ts";
import type { EngineHost, IndicatorCapabilities, IntradayAverageCurveInput, IntradayAverageInput, IntradayAverageResult, PersonalTradeConfirmation, SpeedMetrics } from "./engine-host.ts";
import { createBaselineUpdate, createProtocolUpdate, type HostFailure, type HostUpdate, UI_TARGET_HZ } from "./host-update.ts";
import { parseProtocolSnapshot } from "./protocol/index.ts";
import { parseIsoDate } from "./protocol/wire-values.ts";
import { normalizePublicReportAvailability, normalizePublicReportAvailabilityQuery, normalizePublicReportById, normalizePublicReportPage } from "./serde-normalize.ts";
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
import { parseReportCorrectionStatus } from "./report-corrections.ts";
import { parseCompanyReportCorrection } from "../save/schema/company/report-corrections.ts";
import { exact } from "./protocol/guards.ts";
import { normalizeConfirmationCursor, normalizeIntradayAverageCurveInput, normalizeIntradayAverageCurveResult, normalizeIntradayAverageInput, normalizeIntradayAverageResult, normalizePersonalTradeConfirmations } from "./intraday-average.ts";
import { normalizePersonalTradeHistoryRequest, normalizePersonalTradeHistoryPage } from "./personal-trade-history.ts";
import type { PersonalTradeHistoryRequest, PersonalTradeHistoryPage } from "./engine-host.ts";
import { parseAccountDividendTaxStatusView, parseDividendTaxOutstandingViews } from "./dividend-tax.ts";
import { parseCompanyPreferenceRejections, parseRejectedRightsSubscriptions } from "./corporate-action-views.ts";
import {
  parseCompanyCapabilities,
  parseFlatWithholdingReceipts,
  parseOwnerRightsOfferings,
  parsePeriodChangeExplanation,
} from "./company-contract-views.ts";
import { normalizeMarketHistoryRequest, normalizeMarketHistoryPage, type MarketHistoryRequest, type MarketHistoryPage } from "./market-history.ts";
import { normalizeCurrentMinuteHistoryRequest, normalizeCurrentMinuteHistoryResponse, type CurrentMinuteHistoryRequest, type CurrentMinuteHistoryResponse } from "./current-minute-history.ts";

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

export async function requestWorkerIntradayAverage(requests: WorkerRequestScope, requestId: number, currentGeneration: number, input: IntradayAverageInput): Promise<IntradayAverageResult | null> {
  const normalized = normalizeIntradayAverageInput(input);
  const response = await requests.request({ type: "calculateIntradayAverage", requestId, generation: currentGeneration, ...normalized }, "intradayAverageCalculated");
  return normalizeIntradayAverageResult(response.result);
}

export async function requestWorkerIntradayAverageCurve(requests: WorkerRequestScope, requestId: number, currentGeneration: number, input: IntradayAverageCurveInput): Promise<readonly (IntradayAverageResult | null)[]> {
  const normalized = normalizeIntradayAverageCurveInput(input);
  const response = await requests.request({ type: "calculateIntradayAverageCurve", requestId, generation: currentGeneration, seriesKey: normalized.seriesKey, samples: normalized.samples }, "intradayAverageCurveCalculated");
  if (response.seriesKey !== normalized.seriesKey) throw new Error("VWAP curve 响应 seriesKey 与请求不匹配");
  return normalizeIntradayAverageCurveResult(response.results, normalized.samples.length, normalized.samples);
}

export async function requestWorkerPersonalTradeConfirmations(requests: WorkerRequestScope, requestId: number, currentGeneration: number, beforeReceipt: string | null = null): Promise<readonly PersonalTradeConfirmation[]> {
  const response = await requests.request({ type: "personalTradeConfirmations", requestId, generation: currentGeneration, beforeReceipt: normalizeConfirmationCursor(beforeReceipt) }, "personalTradeConfirmations");
  return normalizePersonalTradeConfirmations(response.confirmations);
}

export async function requestWorkerPersonalTradeHistory(requests: WorkerRequestScope, requestId: number, currentGeneration: number, request: PersonalTradeHistoryRequest): Promise<PersonalTradeHistoryPage> {
  const normalized = normalizePersonalTradeHistoryRequest(request);
  const response = await requests.request({ type: "personalTradeHistory", requestId, generation: currentGeneration, query: normalized }, "personalTradeHistory");
  return normalizePersonalTradeHistoryPage(response.page, normalized);
}

export async function requestWorkerMarketHistory(requests: WorkerRequestScope, requestId: number, currentGeneration: number, request: MarketHistoryRequest): Promise<MarketHistoryPage> {
  const normalized = normalizeMarketHistoryRequest(request);
  const response = await requests.request({ type: "marketHistory", requestId, generation: currentGeneration, query: normalized }, "marketHistory");
  return normalizeMarketHistoryPage(response.page, normalized);
}

export async function requestWorkerCurrentMinuteHistory(requests: WorkerRequestScope, requestId: number, currentGeneration: number, request: CurrentMinuteHistoryRequest): Promise<CurrentMinuteHistoryResponse> {
  const normalized = normalizeCurrentMinuteHistoryRequest(request);
  const response = await requests.request({ type: "currentMinuteHistory", requestId, generation: currentGeneration, query: normalized }, "currentMinuteHistory");
  return normalizeCurrentMinuteHistoryResponse(response.response, normalized);
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
): Promise<{ readonly snapshot: Snapshot; readonly civilDate: string; readonly nextGeneration: number }> {
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
    civilDate: parseIsoDate(response.civilDate, "Worker restored.civilDate"),
    nextGeneration,
  };
}

export async function refreshWorkerBaseline(requests: WorkerRequestScope, requestId: number, currentGeneration: number): Promise<{ readonly snapshot: Snapshot; readonly civilDate: string }> {
  const response = await requests.request({ type: "refreshBaseline", requestId, generation: currentGeneration }, "refreshed");
  return {
    snapshot: parseProtocolSnapshot(response.snapshot, "Worker refreshed.snapshot"),
    civilDate: parseIsoDate(response.civilDate, "Worker refreshed.civilDate"),
  };
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
    let ingressWorker: Worker | null = null;
    let ingressRequests: WorkerRequestScope | null = null;
    let ingressReady: Promise<unknown> | null = null;
    let ingressGeneration = 0;
    const closeIngress = (error: Error) => {
      ingressRequests?.close(error);
      ingressWorker?.terminate();
      ingressWorker = null;
      ingressRequests = null;
    };
    let callback: ((update: HostUpdate) => void | boolean) | null = null;
    let fatalCallback: ((failure: HostFailure) => void) | null = null;
    let cachedBaseline: Extract<HostUpdate, { type: "baseline" }> | null = null;
    let baselineEpoch = 0;
    let deliveredGeneration: string | null = null;
    let initialized = false;
    let disposed = false;
    let currentGeneration = 0;
    let npcDecisionDiagnostics = false;
    let indicatorCapabilities: IndicatorCapabilities = { intradayAverage: false, macd: false, priceKdj: false, candleKdj: false };
    let personalTradeHistory = false;
    let pendingFailure: HostFailure | null = null;
    const timeout = setTimeout(() => {
      if (!initialized) {
        notifyFailure({ code: "WASM_WORKER_INIT_TIMEOUT", where: "worker-host.initialize", message: "WASM 多线程初始化超时（10s）。请检查 SharedArrayBuffer、COOP/COEP 和 WASM 线程绑定。" });
      }
    }, 10_000);

    const notifyFailure = (failure: HostFailure) => {
      if (disposed) return;
      if (initialized && failure.code === "REPORT_CORRECTION_REJECTED" && failure.recoverable === true) {
        if (fatalCallback !== null) fatalCallback(failure);
        else pendingFailure = failure;
        return;
      }
      disposed = true;
      callback = null;
      cachedBaseline = null;
      const reportFailure = fatalCallback;
      fatalCallback = null;
      requests.close(new Error(`${failure.code} @ ${failure.where}: ${failure.message}`));
      closeIngress(new Error(`${failure.code}: ${failure.message}`));
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
          case "ingress": {
            const nextGeneration = generation(incoming.generation, "Browser ingress generation");
            if (nextGeneration <= ingressGeneration) throw new Error("Browser ingress generation 必须严格推进");
            if (ingressWorker === null) {
              ingressWorker = new Worker(new URL("./wasm-ingress-worker.ts", import.meta.url), { type: "module" });
              ingressRequests = new WorkerRequestScope(ingressWorker);
              ingressWorker.addEventListener("error", (event) => {
                notifyFailure({ code: "WASM_INGRESS_WORKER", where: "worker-host.ingress", message: event.message || "Browser ingress worker 执行失败" });
              });
            }
            if (ingressRequests === null) throw new Error("Browser ingress request scope 尚未初始化");
            ingressGeneration = nextGeneration;
            ingressReady = ingressRequests.request({
              ...incoming, type: "bindIngress", requestId: ingressRequests.nextRequestId(), generation: nextGeneration,
            }, "ingressBound");
            void ingressReady.catch((error: unknown) => {
              notifyFailure({ code: "WASM_INGRESS_BIND", where: "worker-host.ingress", message: error instanceof Error ? error.message : String(error) });
            });
            return;
          }
          case "ready":
            worker.postMessage({ type: "create", setup, seed });
            return;
          case "created":
            currentGeneration = generation(incoming.generation, "Worker created generation");
            {
              const workerCapabilities = record(incoming.capabilities, "Worker capabilities");
              const indicators = workerCapabilities.indicatorCapabilities;
              if (Object.keys(workerCapabilities).length !== 3 || typeof workerCapabilities.npcDecisionDiagnostics !== "boolean"
                || indicators === null || typeof indicators !== "object" || Array.isArray(indicators)
                || Object.keys(indicators as Record<string, unknown>).length !== 4
                || !["intradayAverage", "macd", "priceKdj", "candleKdj"].every((key) => typeof (indicators as Record<string, unknown>)[key] === "boolean")
                || typeof workerCapabilities.personalTradeHistory !== "boolean") {
                throw new Error("Worker capabilities 契约无效");
              }
              npcDecisionDiagnostics = workerCapabilities.npcDecisionDiagnostics;
              indicatorCapabilities = indicators as IndicatorCapabilities;
              personalTradeHistory = workerCapabilities.personalTradeHistory;
            }
            return;
          case "baseline": {
            const nextGeneration = generation(incoming.generation, "Worker baseline generation");
            if (nextGeneration < currentGeneration) return;
            currentGeneration = nextGeneration;
            const next = createBaselineUpdate(String(nextGeneration), parseProtocolSnapshot(incoming.snapshot, "Worker baseline.snapshot"), {
              civilDate: parseIsoDate(incoming.civilDate, "Worker baseline.civilDate"), revision: null,
            });
            if (deliveredGeneration === next.generation) return;
            baselineEpoch += 1;
            cachedBaseline = next;
            if (!initialized) {
              const finishInitialization = () => {
                if (disposed || initialized) return;
                initialized = true;
                clearTimeout(timeout);
                resolve(host());
              };
              if (ingressReady === null) finishInitialization();
              else void ingressReady.then(finishInitialization).catch((error: unknown) => {
                notifyFailure({ code: "WASM_INGRESS_BIND", where: "worker-host.ingress", message: error instanceof Error ? error.message : String(error) });
              });
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
                closeIngress(new Error("Worker 消费者拒绝协议更新，会话已停止"));
                lifecycle.dispose();
                disposed = true;
                callback = null;
                return;
              }
            }
            worker.postMessage({ type: "uiFrame", generation: updateGeneration, deliveryId });
            return;
          }
          case "failure": {
            if (!Number.isSafeInteger(incoming.generation) || Number(incoming.generation) < 0) throw new Error("Worker failure generation 必须是非负安全整数");
            if (initialized && Number(incoming.generation) < currentGeneration) return;
            if (initialized && incoming.generation !== currentGeneration) throw new Error("Worker failure 来自尚未安装的 generation");
            notifyFailure(parseWorkerFailure(incoming));
            return;
          }
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

    async function reportCorrectionControl(type: string, successType: string, payload: Record<string, unknown>): Promise<void> {
      const requestedGeneration = currentGeneration;
      const response = await requests.request({ type, ...payload, requestId: requests.nextRequestId(), generation: requestedGeneration }, successType);
      if (disposed || currentGeneration !== requestedGeneration) throw new Error("财报更正确认属于已过期 generation");
      exact(response, ["type", "requestId", "generation"], "财报更正 Worker 确认");
    }

    function host(): EngineHost & WorkerE2EHost {
      return {
        capabilities: {
          persistence: "browser",
          deliveryModes: [],
          targetUiHz: UI_TARGET_HZ,
          sharedMemory: true,
          reconnect: false,
          publicCompanyReports: true,
          npcDecisionDiagnostics,
          indicatorCapabilities,
          personalTradeHistory,
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
          closeIngress(new Error("WASM ingress 已被销毁，操作已取消"));
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
        async npcDecisionTrace(account: string): Promise<readonly NpcDecisionTraceRecord[]> {
          if (!npcDecisionDiagnostics) throw new Error("当前 WASM 后端未协商启用 NPC 决策诊断");
          if (typeof account !== "string" || !/^(0|[1-9]\d*)$/.test(account) || account.length > 20 || BigInt(account) > 18_446_744_073_709_551_615n) throw new Error("NPC 账户 ID 必须是规范 u64 非负十进制字符串");
          const response = await requests.request({ type: "npcDecisionTrace", requestId: requests.nextRequestId(), generation: currentGeneration, account }, "npcDecisionTrace");
          return parseNpcDecisionTrace(response.records);
        },
        async calculateIndicators(input: IndicatorInput): Promise<IndicatorResults> {
          return requestWorkerIndicators(requests, requests.nextRequestId(), currentGeneration, input);
        },
        async calculateIntradayAverage(input: IntradayAverageInput): Promise<IntradayAverageResult | null> {
          if (cachedBaseline === null) throw new Error("Worker 基线尚未就绪，不能计算 VWAP");
          const requestedGeneration = currentGeneration;
          const requestedEpoch = baselineEpoch;
          const result = await requestWorkerIntradayAverage(requests, requests.nextRequestId(), requestedGeneration, input);
          if (disposed || currentGeneration !== requestedGeneration || baselineEpoch !== requestedEpoch) throw new Error("Worker VWAP 响应属于已过期会话 generation");
          return result;
        },
        async calculateIntradayAverageCurve(input: IntradayAverageCurveInput): Promise<readonly (IntradayAverageResult | null)[]> {
          if (cachedBaseline === null) throw new Error("Worker 基线尚未就绪，不能计算 VWAP 曲线");
          const normalized = normalizeIntradayAverageCurveInput(input);
          const requestedGeneration = currentGeneration;
          const requestedEpoch = baselineEpoch;
          const response = await requests.request({ type: "calculateIntradayAverageCurve", requestId: requests.nextRequestId(), generation: requestedGeneration, seriesKey: normalized.seriesKey, samples: normalized.samples }, "intradayAverageCurveCalculated");
          if (disposed || currentGeneration !== requestedGeneration || baselineEpoch !== requestedEpoch) throw new Error("Worker VWAP 曲线响应属于已过期会话 generation");
          if (response.seriesKey !== normalized.seriesKey) throw new Error("Worker VWAP 曲线响应 seriesKey 与请求不匹配");
          return normalizeIntradayAverageCurveResult(response.results, normalized.samples.length, normalized.samples);
        },
        async queryPersonalTradeConfirmations(beforeReceipt: string | null = null): Promise<readonly PersonalTradeConfirmation[]> {
          if (cachedBaseline === null) throw new Error("Worker 基线尚未就绪，不能查询本人交割单");
          const requestedGeneration = currentGeneration;
          const requestedEpoch = baselineEpoch;
          const rows = await requestWorkerPersonalTradeConfirmations(requests, requests.nextRequestId(), requestedGeneration, beforeReceipt);
          if (disposed || currentGeneration !== requestedGeneration || baselineEpoch !== requestedEpoch) throw new Error("Worker 交割单响应属于已过期会话 generation");
          return rows;
        },
        async queryPersonalTradeHistory(request: PersonalTradeHistoryRequest): Promise<PersonalTradeHistoryPage> {
          if (cachedBaseline === null) throw new Error("Worker基线尚未就绪，不能查询本人日期交割历史");
          const generation = currentGeneration;
          const epoch = baselineEpoch;
          const page = await requestWorkerPersonalTradeHistory(requests, requests.nextRequestId(), generation, request);
          if (disposed || generation !== currentGeneration || epoch !== baselineEpoch) throw new Error("Worker日期交割历史响应属于过期generation");
          return page;
        },
        async queryMarketHistory(request: MarketHistoryRequest): Promise<MarketHistoryPage> {
          if (cachedBaseline === null) throw new Error("Worker基线尚未就绪，不能查询永久量价历史");
          const generation = currentGeneration, epoch = baselineEpoch;
          const page = await requestWorkerMarketHistory(requests, requests.nextRequestId(), generation, request);
          if (disposed || generation !== currentGeneration || epoch !== baselineEpoch) throw new Error("Worker量价历史响应属于过期generation");
          return page;
        },
        async queryCurrentMinuteHistory(request: CurrentMinuteHistoryRequest): Promise<CurrentMinuteHistoryResponse> {
          if (cachedBaseline === null) throw new Error("Worker基线尚未就绪，不能查询当前分钟");
          const generation = currentGeneration, epoch = baselineEpoch;
          const response = await requestWorkerCurrentMinuteHistory(requests, requests.nextRequestId(), generation, request);
          if (disposed || generation !== currentGeneration || epoch !== baselineEpoch) throw new Error("Worker当前分钟响应属于过期generation");
          return response;
        },
        async stepOnceForE2E() {
          assertWorkerE2EStepAllowed(import.meta.env.MODE === "e2e", options.enableE2EStepping === true);
          return stepWorkerOnce(requests, requests.nextRequestId(), currentGeneration);
        },
        async submitIntent(intent) {
          const requestedGeneration = currentGeneration;
          const ready = ingressReady;
          if (ready === null) throw new Error("Browser 共享 ingress 尚未就绪，请重建 WASM bindings");
          await ready;
          if (disposed || requestedGeneration !== currentGeneration || requestedGeneration !== ingressGeneration) throw new Error("Browser ingress 请求属于已过期 generation");
          if (ingressRequests === null) throw new Error("Browser ingress 已关闭");
          await ingressRequests.request({ type: "enqueue", requestId: ingressRequests.nextRequestId(), generation: requestedGeneration, intent }, "enqueued");
          if (disposed || requestedGeneration !== currentGeneration || requestedGeneration !== ingressGeneration) throw new Error("Browser ingress 入队确认属于已过期或已销毁 generation，请核对权威委托状态，勿重复提交");
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
          const response = await refreshWorkerBaseline(requests, requests.nextRequestId(), requestedGeneration);
          if (disposed || currentGeneration !== requestedGeneration) throw new Error("Worker 基线刷新响应属于已过期会话 generation");
          const baseline = createBaselineUpdate(String(requestedGeneration), response.snapshot, { civilDate: response.civilDate, revision: null });
          baselineEpoch += 1;
          cachedBaseline = baseline;
          deliverLiveBaseline(baseline);
        },
        async load(slot, _archiveSlotId, onRestored) {
          const parsedSlot = parseSaveSlot(slot);
          const requestedGeneration = currentGeneration;
          const restored = await restoreWorkerSlot(requests, parsedSlot, requests.nextRequestId(), requestedGeneration);
          if (disposed || currentGeneration !== requestedGeneration && currentGeneration !== restored.nextGeneration) throw new Error("Worker restore 响应属于已过期 generation");
          currentGeneration = restored.nextGeneration;
          onRestored?.();
          const baseline = createBaselineUpdate(String(restored.nextGeneration), restored.snapshot, { civilDate: restored.civilDate, revision: null });
          baselineEpoch += 1;
          cachedBaseline = baseline;
          if (callback !== null && deliveredGeneration !== baseline.generation) {
            deliverLiveBaseline(baseline);
          }
        },
        async submitReportCorrection(request) {
          await reportCorrectionControl("submitReportCorrection", "reportCorrectionSubmitted", { request: parseCompanyReportCorrection(request) });
        },
        async cancelReportCorrection(operationId) {
          if (operationId.trim().length === 0) throw new Error("更正 operation_id 必须非空");
          await reportCorrectionControl("cancelReportCorrection", "reportCorrectionCancelled", { operationId });
        },
        async queryReportCorrections() {
          const requestedGeneration = currentGeneration;
          const queryEpoch = baselineEpoch;
          const response = await requests.request({ type: "queryReportCorrections", requestId: requests.nextRequestId(), generation: requestedGeneration }, "reportCorrections");
          if (disposed || currentGeneration !== requestedGeneration || baselineEpoch !== queryEpoch) throw new Error("财报更正查询属于已过期 generation");
          exact(response, ["type", "requestId", "generation", "value"], "财报更正 Worker 查询");
          return parseReportCorrectionStatus(response.value);
        },
        async queryPublicReports(query: PublicReportQuery): Promise<PublicReportPage> {
          const response = await requests.request({ type: "publicReports", requestId: requests.nextRequestId(), generation: currentGeneration, query }, "publicReports");
          return normalizePublicReportPage(response.page);
        },
        async queryPublicReportAvailability(query: PublicReportAvailabilityQuery): Promise<PublicReportAvailability> {
          const normalizedQuery = normalizePublicReportAvailabilityQuery(query);
          const requestedGeneration = currentGeneration;
          const queryEpoch = baselineEpoch;
          const response = await requests.request({ type: "publicReportAvailability", requestId: requests.nextRequestId(), generation: requestedGeneration, query: normalizedQuery }, "publicReportAvailability");
          if (disposed || currentGeneration !== requestedGeneration || baselineEpoch !== queryEpoch) throw new Error("公开报告可用性查询属于已过期 generation");
          return normalizePublicReportAvailability(response.availability, normalizedQuery);
        },
        async publicReportById(id: string): Promise<PublicReportSummary> {
          const response = await requests.request({ type: "publicReportById", requestId: requests.nextRequestId(), generation: currentGeneration, id }, "publicReportById");
          return normalizePublicReportById(response.report);
        },
        async dividendTaxStatus() {
          const requestedGeneration = currentGeneration;
          const queryEpoch = baselineEpoch;
          const response = await requests.request({ type: "dividendTaxStatus", requestId: requests.nextRequestId(), generation: requestedGeneration }, "dividendTaxStatus");
          if (disposed || currentGeneration !== requestedGeneration || baselineEpoch !== queryEpoch) throw new Error("股息税状态查询属于已过期 generation");
          return parseAccountDividendTaxStatusView(response.status);
        },
        async dividendTaxOutstanding() {
          const requestedGeneration = currentGeneration;
          const queryEpoch = baselineEpoch;
          const response = await requests.request({ type: "dividendTaxOutstanding", requestId: requests.nextRequestId(), generation: requestedGeneration }, "dividendTaxOutstanding");
          if (disposed || currentGeneration !== requestedGeneration || baselineEpoch !== queryEpoch) throw new Error("股息税未清税额查询属于已过期 generation");
          return parseDividendTaxOutstandingViews(response.views);
        },
        async rejectedRightsSubscriptions() {
          const requestedGeneration = currentGeneration;
          const queryEpoch = baselineEpoch;
          const response = await requests.request({ type: "rightsRejections", requestId: requests.nextRequestId(), generation: requestedGeneration }, "rightsRejections");
          if (disposed || currentGeneration !== requestedGeneration || baselineEpoch !== queryEpoch) throw new Error("配股认购拒绝回执查询属于已过期 generation");
          return parseRejectedRightsSubscriptions(response.receipts);
        },
        async companyPreferenceRejections(company) {
          if (typeof company !== "string" || company.trim().length === 0 || company.length > 64) throw new Error("偏好台账公司身份必须是非空且不超过 64 字符的字符串");
          const requestedGeneration = currentGeneration;
          const queryEpoch = baselineEpoch;
          const response = await requests.request({ type: "preferenceRejections", requestId: requests.nextRequestId(), generation: requestedGeneration, company }, "preferenceRejections");
          if (disposed || currentGeneration !== requestedGeneration || baselineEpoch !== queryEpoch) throw new Error("偏好拒绝台账查询属于已过期 generation");
          return parseCompanyPreferenceRejections(response.rejections);
        },
        async companyCapabilities(company) {
          if (typeof company !== "string" || company.trim().length === 0 || company.length > 64) throw new Error("能力面公司身份必须是非空且不超过 64 字符的字符串");
          const requestedGeneration = currentGeneration;
          const queryEpoch = baselineEpoch;
          const response = await requests.request({ type: "companyCapabilities", requestId: requests.nextRequestId(), generation: requestedGeneration, company }, "companyCapabilities");
          if (disposed || currentGeneration !== requestedGeneration || baselineEpoch !== queryEpoch) throw new Error("公司能力面查询属于已过期 generation");
          return parseCompanyCapabilities(response.capabilities);
        },
        async companyPeriodExplanation(company, periodEnd) {
          if (typeof company !== "string" || company.trim().length === 0 || company.length > 64) throw new Error("期间解释公司身份必须是非空且不超过 64 字符的字符串");
          if (typeof periodEnd !== "string" || !/^\d{4}-\d{2}-\d{2}$/.test(periodEnd)) throw new Error("期间解释 periodEnd 必须是 ISO YYYY-MM-DD");
          const requestedGeneration = currentGeneration;
          const queryEpoch = baselineEpoch;
          const response = await requests.request({ type: "periodExplanation", requestId: requests.nextRequestId(), generation: requestedGeneration, company, periodEnd }, "periodExplanation");
          if (disposed || currentGeneration !== requestedGeneration || baselineEpoch !== queryEpoch) throw new Error("期间解释查询属于已过期 generation");
          return parsePeriodChangeExplanation(response.explanation);
        },
        async ownerRightsOfferings() {
          const requestedGeneration = currentGeneration;
          const queryEpoch = baselineEpoch;
          const response = await requests.request({ type: "ownerRightsOfferings", requestId: requests.nextRequestId(), generation: requestedGeneration }, "ownerRightsOfferings");
          if (disposed || currentGeneration !== requestedGeneration || baselineEpoch !== queryEpoch) throw new Error("本人配股权益查询属于已过期 generation");
          return parseOwnerRightsOfferings(response.offerings);
        },
        async ownerFlatWithholdingReceipts() {
          const requestedGeneration = currentGeneration;
          const queryEpoch = baselineEpoch;
          const response = await requests.request({ type: "flatWithholdingReceipts", requestId: requests.nextRequestId(), generation: requestedGeneration }, "flatWithholdingReceipts");
          if (disposed || currentGeneration !== requestedGeneration || baselineEpoch !== queryEpoch) throw new Error("简税代扣回执查询属于已过期 generation");
          return parseFlatWithholdingReceipts(response.receipts);
        },
      };
    }
  });
}

export const MAX_SPEED = Infinity;
