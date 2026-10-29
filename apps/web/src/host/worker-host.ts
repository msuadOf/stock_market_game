import type { CatalogResolver } from "./catalog-resolver.ts";
import { createCompanyReadQueries } from "./company-read.ts";
import { createCompanyWriteCommands, type CompanyWriteCommands } from "./company-write.ts";
import type {
  PublicReportPage,
  PublicReportQuery,
  PublicReportSummary,
  SessionSetup,
  StockCode,
  HistoricalStockData,
} from "../types/engine.ts";
import { wireIntentOf } from "./protocol/stock-identity.ts";
import type { PausePreferences } from "../types/generated/PausePreferences.ts";
import { parseSaveSlot } from "../save/save-schema.ts";
import type { StrictSaveEnvelope } from "../save/schema/root.ts";
import { validateDayEndArchive } from "../save/day-end-candidate.ts";
import type { EngineHost, IndicatorCapabilities, SpeedMetrics } from "./engine-host.ts";
import type { IntradayAverageCurveInput, IntradayAverageInput, IntradayAverageResult, PersonalTradeConfirmation } from "./engine-host.ts";
import type { CurrentMinuteHistoryRequest, CurrentMinuteHistoryResponse } from "./current-minute-history.ts";
import type { MarketHistoryRequest, MarketHistoryPage } from "./market-history.ts";
import type { PersonalTradeHistoryRequest, PersonalTradeHistoryPage } from "./personal-trade-history.ts";
import { normalizeCurrentMinuteHistoryRequest, normalizeCurrentMinuteHistoryResponse, wireCurrentMinuteHistoryRequest } from "./current-minute-history.ts";
import { normalizeMarketHistoryRequest, normalizeMarketHistoryPage, wireMarketHistoryRequest } from "./market-history.ts";
import { normalizePersonalTradeHistoryRequest, normalizePersonalTradeHistoryPage, wirePersonalTradeHistoryRequest } from "./personal-trade-history.ts";
import { normalizeConfirmationCursor, normalizeIntradayAverageCurveInput, normalizeIntradayAverageCurveResult, normalizeIntradayAverageInput, normalizeIntradayAverageResult, normalizePersonalTradeConfirmations } from "./intraday-average.ts";
import { prepareBaselineUpdate, createProtocolUpdate, type HostFailure, type HostUpdate, UI_TARGET_HZ } from "./host-update.ts";
import { parseIsoDate } from "./protocol/wire-values.ts";
import { normalizePublicReportById, normalizePublicReportPage } from "./serde-normalize.ts";
import { assertValidSpeedMultiplier, parseSpeedMetrics } from "./speed.ts";
import { normalizePlayerWorkingOrders, type PlayerWorkingOrder } from "./player-working-orders.ts";
import { parseHostFailure } from "./protocol-failure.ts";
import { parseNpcDecisionTrace, type NpcDecisionTraceRecord } from "./npc-decision-trace.ts";
import type { IndicatorInput, IndicatorResults } from "../components/indicator-results.ts";
import { normalizeIndicatorInput, normalizeIndicatorResults } from "./indicator-transport.ts";
import { createEngineSupervisor } from "./engine-supervisor.ts";
import { WorkerRequestScope } from "./worker-request.ts";
import { parseHistoricalStockData } from "./stock-history.ts";
import { parseInitialAllocation } from "./initial-allocation.ts";
import { WasmHostInitializationError } from "./wasm-host-initialization-error.ts";

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

export async function readWorkerPlayerWorkingOrders(requests: WorkerRequestScope, requestId: number, currentGeneration: number, identity: CatalogResolver): Promise<readonly PlayerWorkingOrder[]> {
  const response = await requests.request({ type: "playerWorkingOrders", requestId, generation: currentGeneration }, "playerWorkingOrders");
  return normalizePlayerWorkingOrders(response.orders, identity);
}

export async function readWorkerStockHistory(requests: WorkerRequestScope, requestId: number, currentGeneration: number, code: StockCode, identity: CatalogResolver): Promise<HistoricalStockData> {
  const response = await requests.request({ type: "stockHistory", requestId, generation: currentGeneration, code: identity.stockId(code) }, "stockHistory");
  return parseHistoricalStockData(response.data, code, identity);
}

export async function requestWorkerIndicators(requests: WorkerRequestScope, requestId: number, currentGeneration: number, input: IndicatorInput): Promise<IndicatorResults> {
  const normalized = normalizeIndicatorInput(input);
  const response = await requests.request({ type: "calculateIndicators", requestId, generation: currentGeneration, ...normalized }, "indicatorsCalculated");
  return normalizeIndicatorResults(response.result, normalized);
}

/** W3 查询面传输：主线程 → Worker → wasm FFI；请求在 worker 内完成
 *  code→StockId 数值化（`wire*Request`，ADR-0041），响应经归一层译回视图词表。 */
export async function requestWorkerMarketHistory(requests: WorkerRequestScope, requestId: number, currentGeneration: number, request: MarketHistoryRequest, identity: CatalogResolver): Promise<MarketHistoryPage> {
  const normalized = normalizeMarketHistoryRequest(request);
  const response = await requests.request({ type: "marketHistory", requestId, generation: currentGeneration, query: wireMarketHistoryRequest(normalized, identity) }, "marketHistory");
  return normalizeMarketHistoryPage(response.page, normalized, identity);
}

export async function requestWorkerCurrentMinuteHistory(requests: WorkerRequestScope, requestId: number, currentGeneration: number, request: CurrentMinuteHistoryRequest, identity: CatalogResolver): Promise<CurrentMinuteHistoryResponse> {
  const normalized = normalizeCurrentMinuteHistoryRequest(request);
  const response = await requests.request({ type: "currentMinuteHistory", requestId, generation: currentGeneration, query: wireCurrentMinuteHistoryRequest(normalized, identity) }, "currentMinuteHistory");
  return normalizeCurrentMinuteHistoryResponse(response.response, normalized, identity);
}

export async function requestWorkerPersonalTradeHistory(requests: WorkerRequestScope, requestId: number, currentGeneration: number, request: PersonalTradeHistoryRequest, identity: CatalogResolver): Promise<PersonalTradeHistoryPage> {
  const normalized = normalizePersonalTradeHistoryRequest(request);
  const response = await requests.request({ type: "personalTradeHistory", requestId, generation: currentGeneration, query: wirePersonalTradeHistoryRequest(normalized, identity) }, "personalTradeHistory");
  return normalizePersonalTradeHistoryPage(response.page, normalized, identity);
}

export async function requestWorkerPersonalTradeConfirmations(requests: WorkerRequestScope, requestId: number, currentGeneration: number, beforeReceipt: string | null = null, identity: CatalogResolver): Promise<readonly PersonalTradeConfirmation[]> {
  const response = await requests.request({ type: "personalTradeConfirmations", requestId, generation: currentGeneration, beforeReceipt: normalizeConfirmationCursor(beforeReceipt) }, "personalTradeConfirmations");
  return normalizePersonalTradeConfirmations(response.confirmations, identity);
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
  onCommitted?: () => void,
): Promise<Extract<HostUpdate, { type: "baseline" }> & { readonly nextGeneration: number }> {
  const response = await requests.request({
    type: "restore",
    requestId,
    generation: currentGeneration,
    slot,
  }, "restored");
  onCommitted?.();
  const nextGeneration = generation(response.nextGeneration, "Worker restore generation");
  if (nextGeneration <= currentGeneration) {
    throw new Error(`Worker restore generation 必须大于当前 generation ${currentGeneration}，实际收到 ${nextGeneration}`);
  }
  return {
    ...prepareBaselineUpdate(String(nextGeneration), response.snapshot, response.stockCatalog, response.companyCatalog, { civilDate: parseIsoDate(response.civilDate, "Worker restored.civilDate"), revision: null }),
    nextGeneration,
  };
}

export async function refreshWorkerBaseline(requests: WorkerRequestScope, requestId: number, currentGeneration: number): Promise<Extract<HostUpdate, { type: "baseline" }>> {
  const response = await requests.request({ type: "refreshBaseline", requestId, generation: currentGeneration }, "refreshed");
  return prepareBaselineUpdate(String(currentGeneration), response.snapshot, response.stockCatalog, response.companyCatalog, { civilDate: parseIsoDate(response.civilDate, "Worker refreshed.civilDate"), revision: null });
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
  readonly onSpawnFailure: (failure: HostFailure) => void;
  readonly initialArchive?: StrictSaveEnvelope | null;
}

export function createWorkerHost(
  setup: SessionSetup,
  seed: bigint,
  options: WorkerHostOptions,
): Promise<EngineHost & WorkerE2EHost & CompanyWriteCommands> {
  return new Promise((resolve, reject) => {
    if (options === null || typeof options !== "object" || typeof options.onSpawnFailure !== "function") {
      reject(new Error("WorkerHost 必须提供 onSpawnFailure callback"));
      return;
    }
    if (options.initialArchive !== null && options.initialArchive !== undefined) {
      const archive = parseSaveSlot(structuredClone(options.initialArchive));
      validateDayEndArchive(archive);
      reject(new WasmHostInitializationError({
        code: "WASM_RESTORE_TOPOLOGY_NOT_READY",
        where: "worker-host.initial-archive",
        message: "WASM 日终存档恢复所需的宿主拓扑尚未就绪",
        context: { operation: "restore" },
      }));
      return;
    }
    const worker = new Worker(new URL("./wasm-worker.ts", import.meta.url), { type: "module" });
    let supervisor: ReturnType<typeof createEngineSupervisor> | null = null;
    const requests = new WorkerRequestScope(worker);
    let ingressWorker: Worker | null = null;
    let ingressRequests: WorkerRequestScope | null = null;
    let ingressReady: Promise<unknown> | null = null;
    let ingressGeneration = 0;
    let restoringGeneration: number | null = null;
    const closeIngress = (error: Error) => {
      ingressRequests?.close(error);
      ingressRequests = null;
      ingressReady = null;
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
    // W3 能力面：指标源能力与个人交割历史支持由 wasm host_capabilities 声明；
    // 未声明（旧 bindings）按明确不支持处理，不静默回退。
    let indicatorCapabilities: IndicatorCapabilities = { intradayAverage: false, macd: false, priceKdj: false, candleKdj: false };
    let personalTradeHistory = false;
    let pendingFailure: HostFailure | null = null;
    const timeout = setTimeout(() => {
      if (!initialized) {
        notifyFailure({ code: "WASM_WORKER_INIT_TIMEOUT", where: "worker-host.initialize", message: "WASM 多线程初始化超时（10s）。请检查 SharedArrayBuffer、COOP/COEP 和 WASM 线程绑定。" });
      }
    }, 10_000);

    const notifyFailure = (failure: HostFailure, preserveVisibleBaseline = false) => {
      if (disposed) return;
      disposed = true;
      callback = null;
      if (!preserveVisibleBaseline) cachedBaseline = null;
      const reportFailure = fatalCallback;
      fatalCallback = null;
      requests.close(new Error(`${failure.code} @ ${failure.where}: ${failure.message}`));
      closeIngress(new Error(`${failure.code}: ${failure.message}`));
      clearTimeout(timeout);
      worker.removeEventListener("message", onMessage);
      supervisor?.dispose();
      ingressWorker = null;
      if (!initialized) {
        reject(new WasmHostInitializationError(failure));
        return;
      }
      if (reportFailure !== null) {
        try { reportFailure(failure); }
        catch (error) {
          // 消息入口会捕获同步异常；清理后的 callback 故障须独立交给全局错误处理。
          const report = new AggregateError([failure, error], "WorkerHost fatal callback 失败；全部本人资源已清理");
          queueMicrotask(() => { throw report; });
        }
      } else pendingFailure = failure;
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

    function waitForInitialization(): void {
      const ready = ingressReady;
      const boundGeneration = ingressGeneration;
      if (ready === null) return;
      const stillCurrent = () => !disposed && ready === ingressReady && boundGeneration === ingressGeneration;
      void ready.then(() => {
        if (!stillCurrent() || initialized || currentGeneration !== boundGeneration
          || cachedBaseline?.generation !== String(boundGeneration)) return;
        initialized = true;
        clearTimeout(timeout);
        resolve(host());
      }, (error: unknown) => {
        if (!stillCurrent()) return;
        notifyFailure({ code: "WASM_INGRESS_BIND", where: "worker-host.ingress", message: error instanceof Error ? error.message : String(error) });
      });
    }

    function onMessage(event: MessageEvent<unknown>): void {
      if (disposed) return;
      try {
        if (supervisor === null) throw new Error("Worker supervisor 尚未建立");
        if (supervisor.handleGlobalMessage(worker, event.data)) return;
        const incoming = message(event.data);
        switch (incoming.type) {
          case "ingress": {
            supervisor.assertConfigured();
            if (Object.keys(incoming).sort().join(",") !== "generation,shared,token,type") throw new Error("Browser ingress 消息形状无效");
            const nextGeneration = generation(incoming.generation, "Browser ingress generation");
            if (!Number.isInteger(incoming.token) || typeof incoming.token !== "number" || incoming.token < 1 || incoming.token > 0xffffffff) throw new Error("Browser ingress token 必须是正 u32");
            if (incoming.shared !== true) throw new Error("Browser ingress 必须使用 shared memory");
            if (nextGeneration <= ingressGeneration) throw new Error("Browser ingress generation 必须严格推进");
            if (nextGeneration !== currentGeneration && !(currentGeneration === 0 && ingressGeneration === 0 && nextGeneration === 1)
              && !(ingressGeneration > 0 && restoringGeneration === currentGeneration && nextGeneration === currentGeneration + 1)) throw new Error("Browser ingress generation 没有当前会话或在途 restore 依据");
            closeIngress(new Error("Browser ingress generation 已过期，操作已取消"));
            ingressGeneration = nextGeneration;
            const first = ingressWorker === null;
            if (first) {
              ingressWorker = new Worker(new URL("./wasm-ingress-worker.ts", import.meta.url), { type: "module" });
              supervisor.attachIngress(ingressWorker);
            }
            if (ingressWorker === null) throw new Error("Browser ingress Worker 尚未初始化");
            ingressRequests = new WorkerRequestScope(ingressWorker);
            ingressReady = ingressRequests.request({
              ...(first ? supervisor.getSubstrate() : {}), type: "bindIngress", shared: true, token: incoming.token,
              requestId: ingressRequests.nextRequestId(), generation: nextGeneration,
            }, "ingressBound");
            waitForInitialization();
            return;
          }
          case "ready":
            supervisor.assertConfigured();
            worker.postMessage({ type: "create", seed });
            return;
          case "created":
            supervisor.assertConfigured();
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
            supervisor.assertConfigured();
            const nextGeneration = generation(incoming.generation, "Worker baseline generation");
            if (nextGeneration < currentGeneration) return;
            const next = prepareBaselineUpdate(String(nextGeneration), incoming.snapshot, incoming.stockCatalog, incoming.companyCatalog, {
              civilDate: parseIsoDate(incoming.civilDate, "Worker baseline.civilDate"), revision: null,
            });
            currentGeneration = nextGeneration;
            if (deliveredGeneration === next.generation) return;
            baselineEpoch += 1;
            cachedBaseline = next;
            if (!initialized) waitForInitialization();
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
                clearTimeout(timeout);
                worker.removeEventListener("message", onMessage);
                supervisor.dispose();
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
    }

    try {
      supervisor = createEngineSupervisor({
        engine: worker,
        createChildWorker: () => new Worker(new URL("./facade-child-worker.ts", import.meta.url), { type: "module" }),
        onFatalFailure: notifyFailure,
        onSpawnFailure: options.onSpawnFailure,
      });
      worker.addEventListener("message", onMessage);
      supervisor.bootstrapEngine({ type: "init", setup });
    } catch (error) {
      notifyFailure({ code: "WASM_WORKER_TRANSPORT", where: "worker-host.initialize", message: error instanceof Error ? error.message : String(error) });
    }

    async function currentResult<T>(read: () => Promise<T>): Promise<T> {
      const requestedGeneration = currentGeneration;
      const scope = requests;
      const result = await read();
      if (disposed || currentGeneration !== requestedGeneration || requests !== scope) throw new Error("Worker 响应属于已过期或已销毁 generation");
      return result;
    }

    async function control(type: string, successType: string, payload: Record<string, unknown> = {}): Promise<void> {
      const requestedGeneration = currentGeneration;
      await requests.request({ type, ...payload, requestId: requests.nextRequestId(), generation: requestedGeneration }, successType);
      if (disposed || currentGeneration !== requestedGeneration) throw new Error(`Worker ${type} 响应属于已过期 generation`);
    }

    function host(): EngineHost & WorkerE2EHost & CompanyWriteCommands {
      return {
        ...createCompanyWriteCommands(async command => {
          if (disposed || pendingFailure !== null || cachedBaseline === null || restoringGeneration !== null || ingressGeneration !== currentGeneration) throw new Error("Worker 公司写命令需要健康的当前会话基线");
          const requestedGeneration = currentGeneration, requestedEpoch = baselineEpoch;
          const stale = () => disposed || pendingFailure !== null || currentGeneration !== requestedGeneration || baselineEpoch !== requestedEpoch || restoringGeneration !== null || ingressGeneration !== requestedGeneration;
          const route = {
            ProposePlayerCompanyAction: { type: "proposePlayerCompanyAction", success: "playerCompanyActionAcknowledged" },
            EnqueueReportCorrection: { type: "enqueueReportCorrection", success: "reportCorrectionEnqueueAcknowledged" },
            CancelReportCorrection: { type: "cancelReportCorrection", success: "reportCorrectionCancelAcknowledged" },
          }[command.kind];
          const { kind: _kind, ...payload } = command;
          let response;
          try {
            response = await requests.request({ type: route.type, ...payload, requestId: requests.nextRequestId(), generation: requestedGeneration }, route.success);
          } catch (error) {
            if (stale()) throw new Error("Worker 公司写命令拒绝属于已过期或已销毁会话 generation", { cause: error });
            throw error;
          }
          if (stale()) throw new Error("Worker 公司写命令确认属于已过期或已销毁会话 generation");
          return response.result;
        }, () => { if (disposed || pendingFailure !== null || cachedBaseline === null || restoringGeneration !== null || ingressGeneration !== currentGeneration) throw new Error("Worker 公司写命令需要健康的当前会话目录"); return cachedBaseline.catalogResolver; }, "0"),
        ...createCompanyReadQueries(async query => {
          if (disposed || pendingFailure !== null || cachedBaseline === null) throw new Error("Worker 公司查询需要健康的当前会话基线");
          const queryGeneration = currentGeneration;
          const queryEpoch = baselineEpoch;
          const response = await requests.request({ type: "companyReadQuery", requestId: requests.nextRequestId(), generation: queryGeneration, query }, "companyReadQuery");
          if (disposed || pendingFailure !== null || currentGeneration !== queryGeneration || baselineEpoch !== queryEpoch) throw new Error("Worker 公司查询响应属于已过期会话 generation");
          return response.result;
        }, () => { if (cachedBaseline === null) throw new Error("Worker 公司查询需要当前目录"); return cachedBaseline.catalogResolver; }, "0"),
        capabilities: {
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
          clearTimeout(timeout);
          worker.removeEventListener("message", onMessage);
          supervisor?.dispose();
          ingressWorker = null;
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
          return currentResult(() => readWorkerSpeedMetrics(requests, requests.nextRequestId(), currentGeneration));
        },
        async playerWorkingOrders() {
          if (cachedBaseline === null) throw new Error("Worker 基线尚未就绪，不能查询玩家活动委托");
          const queryGeneration = currentGeneration;
          const queryEpoch = baselineEpoch;
          const orders = await readWorkerPlayerWorkingOrders(requests, requests.nextRequestId(), queryGeneration, (() => { if (cachedBaseline === null) throw new Error("Worker 活动委托需要 baseline"); return cachedBaseline.catalogResolver; })());
          if (disposed || currentGeneration !== queryGeneration || baselineEpoch !== queryEpoch) throw new Error("Worker 玩家活动委托响应属于已过期会话 generation");
          return orders;
        },
        async queryStockHistory(code: StockCode): Promise<HistoricalStockData> {
          if (cachedBaseline === null) throw new Error("Worker 基线尚未就绪，不能查询股票历史");
          const queryGeneration = currentGeneration;
          const queryEpoch = baselineEpoch;
          const data = await readWorkerStockHistory(requests, requests.nextRequestId(), queryGeneration, code, cachedBaseline.catalogResolver);
          if (disposed || currentGeneration !== queryGeneration || baselineEpoch !== queryEpoch) throw new Error("Worker 股票历史响应属于已过期会话 generation");
          return data;
        },
        async initialAllocation() {
          if (cachedBaseline === null) throw new Error("Worker 基线尚未就绪，不能查询初始分配");
          const queryGeneration = currentGeneration;
          const queryEpoch = baselineEpoch;
          const response = await requests.request({ type: "initialAllocation", requestId: requests.nextRequestId(), generation: queryGeneration }, "initialAllocation");
          if (disposed || currentGeneration !== queryGeneration || baselineEpoch !== queryEpoch) throw new Error("Worker 初始分配响应属于已过期会话 generation");
          return parseInitialAllocation(response.data, cachedBaseline.stockCatalog.map(stock => stock.code), cachedBaseline.catalogResolver);
        },
        async npcDecisionTrace(account: number): Promise<readonly NpcDecisionTraceRecord[]> {
          if (!npcDecisionDiagnostics) throw new Error("当前 WASM 后端未协商启用 NPC 决策诊断");
          if (!Number.isSafeInteger(account) || account < 0) throw new Error("NPC 账户 ID 必须是非负安全整数");
          if (cachedBaseline === null) throw new Error("Worker NPC诊断需要baseline");
          const identity = cachedBaseline.catalogResolver;
          const queryGeneration = currentGeneration;
          const queryEpoch = baselineEpoch;
          const response = await requests.request({ type: "npcDecisionTrace", requestId: requests.nextRequestId(), generation: queryGeneration, account }, "npcDecisionTrace");
          if (disposed || currentGeneration !== queryGeneration || baselineEpoch !== queryEpoch) throw new Error("Worker NPC诊断响应属于过期generation");
          return parseNpcDecisionTrace(response.records, identity);
        },
        async calculateIndicators(input: IndicatorInput): Promise<IndicatorResults> {
          return currentResult(() => requestWorkerIndicators(requests, requests.nextRequestId(), currentGeneration, input));
        },
        async queryMarketHistory(request: MarketHistoryRequest): Promise<MarketHistoryPage> {
          if (cachedBaseline === null) throw new Error("Worker 基线尚未就绪，不能查询保留历史");
          const queryGeneration = currentGeneration;
          const queryEpoch = baselineEpoch;
          const page = await requestWorkerMarketHistory(requests, requests.nextRequestId(), queryGeneration, request, cachedBaseline.catalogResolver);
          if (disposed || currentGeneration !== queryGeneration || baselineEpoch !== queryEpoch) throw new Error("Worker 保留历史响应属于已过期会话 generation");
          return page;
        },
        async queryCurrentMinuteHistory(request: CurrentMinuteHistoryRequest): Promise<CurrentMinuteHistoryResponse> {
          if (cachedBaseline === null) throw new Error("Worker 基线尚未就绪，不能查询当日分钟K");
          const queryGeneration = currentGeneration;
          const queryEpoch = baselineEpoch;
          const response = await requestWorkerCurrentMinuteHistory(requests, requests.nextRequestId(), queryGeneration, request, cachedBaseline.catalogResolver);
          if (disposed || currentGeneration !== queryGeneration || baselineEpoch !== queryEpoch) throw new Error("Worker 当日分钟K响应属于已过期会话 generation");
          return response;
        },
        async queryPersonalTradeHistory(request: PersonalTradeHistoryRequest): Promise<PersonalTradeHistoryPage> {
          if (cachedBaseline === null) throw new Error("Worker 基线尚未就绪，不能查询本人交割历史");
          if (!personalTradeHistory) throw new Error("当前 WASM bindings 不支持本人交割历史查询，请重建 bindings");
          const queryGeneration = currentGeneration;
          const queryEpoch = baselineEpoch;
          const page = await requestWorkerPersonalTradeHistory(requests, requests.nextRequestId(), queryGeneration, request, cachedBaseline.catalogResolver);
          if (disposed || currentGeneration !== queryGeneration || baselineEpoch !== queryEpoch) throw new Error("Worker 本人交割历史响应属于已过期会话 generation");
          return page;
        },
        async queryPersonalTradeConfirmations(beforeReceipt?: string | null): Promise<readonly PersonalTradeConfirmation[]> {
          if (cachedBaseline === null) throw new Error("Worker 基线尚未就绪，不能查询本人交割单");
          if (!personalTradeHistory) throw new Error("当前 WASM bindings 不支持本人交割单查询，请重建 bindings");
          const queryGeneration = currentGeneration;
          const queryEpoch = baselineEpoch;
          const rows = await requestWorkerPersonalTradeConfirmations(requests, requests.nextRequestId(), queryGeneration, beforeReceipt ?? null, cachedBaseline.catalogResolver);
          if (disposed || currentGeneration !== queryGeneration || baselineEpoch !== queryEpoch) throw new Error("Worker 本人交割单响应属于已过期会话 generation");
          return rows;
        },
        async calculateIntradayAverage(input: IntradayAverageInput): Promise<IntradayAverageResult | null> {
          if (!indicatorCapabilities.intradayAverage) throw new Error("当前 WASM bindings 不支持 Rust 分时均价计算，请重建 bindings");
          return currentResult(() => requestWorkerIntradayAverage(requests, requests.nextRequestId(), currentGeneration, input));
        },
        async calculateIntradayAverageCurve(input: IntradayAverageCurveInput): Promise<readonly (IntradayAverageResult | null)[]> {
          if (!indicatorCapabilities.intradayAverage) throw new Error("当前 WASM bindings 不支持 Rust 分时均价曲线计算，请重建 bindings");
          return currentResult(() => requestWorkerIntradayAverageCurve(requests, requests.nextRequestId(), currentGeneration, input));
        },
        async stepOnceForE2E() {
          assertWorkerE2EStepAllowed(import.meta.env.MODE === "e2e", options.enableE2EStepping === true);
          return currentResult(() => stepWorkerOnce(requests, requests.nextRequestId(), currentGeneration));
        },
        async submitIntent(intent) {
          if (disposed) throw new Error("Worker会话已关闭，写请求未入队");
          const baseline = cachedBaseline;
          if (baseline === null) throw new Error("Worker intent需要已安装的权威baseline");
          const requestedGeneration = currentGeneration;
          if (baseline.generation !== String(requestedGeneration)) throw new Error("Worker intent的baseline generation已过期");
          // 同步捕获目录与 generation，翻译必须先于 pending、等待与发送。
          const wiredIntent = wireIntentOf(intent, baseline.catalogResolver);
          const ready = ingressReady;
          const scope = ingressRequests;
          if (ready === null) throw new Error("Browser 共享 ingress 尚未就绪，请重建 WASM bindings");
          await ready;
          if (disposed || ready !== ingressReady || scope !== ingressRequests || requestedGeneration !== currentGeneration || requestedGeneration !== ingressGeneration) throw new Error("Browser ingress 请求属于已过期 generation");
          if (scope === null) throw new Error("Browser ingress 已关闭");
          await scope.request({ type: "enqueue", requestId: scope.nextRequestId(), generation: requestedGeneration, intent: wiredIntent }, "enqueued");
          if (disposed || ready !== ingressReady || scope !== ingressRequests || requestedGeneration !== currentGeneration || requestedGeneration !== ingressGeneration) throw new Error("Browser ingress 入队确认属于已过期或已销毁 generation，请核对权威委托状态，勿重复提交");
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
          const requestedGeneration = currentGeneration;
          const response = await requests.request({ type: "civilDate", requestId: requests.nextRequestId(), generation: requestedGeneration }, "civilDate");
          if (disposed || currentGeneration !== requestedGeneration) throw new Error("Worker 响应属于已过期或已销毁 generation");
          if (typeof response.date !== "string") throw new Error("Worker 返回的自然日无效");
          return response.date;
        },
        async endCivilDay() {
          const requestedGeneration = currentGeneration;
          await requests.request({ type: "endCivilDay", requestId: requests.nextRequestId(), generation: requestedGeneration }, "civilDayEnded");
          if (disposed || currentGeneration !== requestedGeneration) throw new Error("Worker 响应属于已过期或已销毁 generation");
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
          const baseline = response;
          baselineEpoch += 1;
          cachedBaseline = baseline;
          deliverLiveBaseline(baseline);
        },
        async load(slot, onRestored) {
          const parsedSlot = parseSaveSlot(slot);
          const requestedGeneration = currentGeneration;
          if (restoringGeneration !== null) throw new Error("Worker restore 已在途");
          restoringGeneration = requestedGeneration;
          let restoreCommitted = false;
          let restored: Awaited<ReturnType<typeof restoreWorkerSlot>>;
          try {
            restored = await restoreWorkerSlot(requests, parsedSlot, requests.nextRequestId(), requestedGeneration, () => { restoreCommitted = true; });
          } catch (error) {
            // 成功响应或新 ingress 均已表明旧 authority 不再可用；保留可见缓存但关闭真实发送通道。
            if (restoreCommitted || ingressGeneration > requestedGeneration) {
              notifyFailure({ code: "WASM_WORKER_RESTORE_BASELINE", where: "worker-host.load", message: error instanceof Error ? error.message : String(error), cause: error }, true);
            }
            throw error;
          } finally {
            restoringGeneration = null;
          }
          if (disposed || currentGeneration !== requestedGeneration && currentGeneration !== restored.nextGeneration) throw new Error("Worker restore 响应属于已过期 generation");
          const baseline = restored;
          currentGeneration = restored.nextGeneration;
          onRestored?.();
          baselineEpoch += 1;
          cachedBaseline = baseline;
          if (callback !== null && deliveredGeneration !== baseline.generation) {
            deliverLiveBaseline(baseline);
          }
        },
        async queryPublicReports(query: PublicReportQuery): Promise<PublicReportPage> {
          const requestedGeneration = currentGeneration;
          const response = await requests.request({ type: "publicReports", requestId: requests.nextRequestId(), generation: requestedGeneration, query }, "publicReports");
          if (disposed || currentGeneration !== requestedGeneration) throw new Error("Worker 响应属于已过期或已销毁 generation");
          return normalizePublicReportPage(response.page);
        },
        async publicReportById(id: string): Promise<PublicReportSummary> {
          const requestedGeneration = currentGeneration;
          const response = await requests.request({ type: "publicReportById", requestId: requests.nextRequestId(), generation: requestedGeneration, id }, "publicReportById");
          if (disposed || currentGeneration !== requestedGeneration) throw new Error("Worker 响应属于已过期或已销毁 generation");
          return normalizePublicReportById(response.report);
        },
      };
    }
  });
}

export const MAX_SPEED = Infinity;
