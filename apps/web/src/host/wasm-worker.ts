import { parseSaveSlot } from "../save/save-schema.ts";
import { parseTurnoverCents } from "../utils/turnover.ts";
import { normalizePublicReportAvailability, normalizePublicReportById, normalizePublicReportPage, normalizeSerdeMaps } from "./serde-normalize.ts";
import { WasmSessionSlot } from "./wasm-session-slot.ts";
import { WasmTickLoop } from "./wasm-tick-loop.ts";
import { classifyWasmFailure, describeWasmFailure } from "./wasm-failure.ts";
import { resolveThreadCount } from "./thread-count.ts";
import { parseHostFailure } from "./protocol-failure.ts";
import { shouldLoadDiagnosticsWasm } from "./wasm-build-mode.ts";
import type { HostFailure } from "./host-update.ts";
import type { NpcDecisionTraceRecord } from "./npc-decision-trace.ts";
import { parseReportCorrectionStatus } from "./report-corrections.ts";
import { parseCompanyReportCorrection } from "../save/schema/company/report-corrections.ts";
import { normalizeMarketHistoryRequest } from "./market-history.ts";
import { normalizeCurrentMinuteHistoryRequest } from "./current-minute-history.ts";

type WorkerMessage = Readonly<Record<string, unknown>> & { readonly type: string };
type WasmNpcDecisionTrace = (handle: number, account: bigint) => readonly NpcDecisionTraceRecord[];
type WasmTransportExtensions = typeof import("../../wasm-pkg/web_wasm.js") & {
  readonly public_report_availability?: (handle: number, query: unknown) => unknown;
  readonly ingress_token?: (handle: number) => number;
  readonly submit_report_correction?: (handle: number, request: unknown) => void;
  readonly cancel_report_correction?: (handle: number, operationId: string) => void;
  readonly query_report_corrections?: (handle: number) => unknown;
  readonly prepare_public_baseline?: (handle: number) => void;
  readonly host_capabilities?: () => {
    readonly npcDecisionDiagnostics: boolean;
    readonly indicatorCapabilities: {
      readonly intradayAverage: boolean;
      readonly macd: boolean;
      readonly priceKdj: boolean;
      readonly candleKdj: boolean;
    };
    readonly personalTradeHistory: boolean;
  };
  readonly player_working_orders?: (handle: number) => unknown;
  readonly query_stock_history?: (handle: number, code: string) => unknown;
  readonly initial_allocation?: (handle: number) => unknown;
  readonly calculate_indicators?: (prices: number[], candles: readonly { high: number; low: number; close: number }[]) => unknown;
  readonly calculate_intraday_average?: (turnoverCents: string, tradeCount: bigint, volumeShares: bigint) => unknown;
  readonly calculate_intraday_average_curve?: (samples: unknown) => unknown;
  readonly personal_trade_confirmations?: (handle: number, beforeReceipt: string | null) => unknown;
  readonly personal_trade_history?: (handle: number, query: unknown) => unknown;
  readonly owner_dividend_tax_status?: (handle: number) => unknown;
  readonly owner_dividend_tax_outstanding_views?: (handle: number) => unknown;
  readonly owner_rejected_rights_subscriptions?: (handle: number) => readonly unknown[];
  readonly company_preference_rejections?: (handle: number, company: string) => readonly unknown[];
  readonly market_history?: (handle: number, query: unknown) => unknown;
  readonly current_minute_history?: (handle: number, query: unknown) => unknown;
  readonly npc_decision_trace?: WasmNpcDecisionTrace;
  readonly save_candidate?: (handle: number, key: { readonly seq: number; readonly settledDate: string }) => unknown;
};
type WorkerPort = {
  postMessage(message: unknown): void;
  addEventListener(type: "message", listener: (event: MessageEvent<WorkerMessage>) => void): void;
};

const ctx: WorkerPort = self;

export function isE2EStepMode(mode: unknown): boolean {
  return mode === "e2e";
}

const E2E_STEP_ENABLED = isE2EStepMode(import.meta.env?.MODE);

let wasmModule: typeof import("../../wasm-pkg/web_wasm.js") | null = null;
let ingressBridge: { module: WebAssembly.Module; memory: WebAssembly.Memory; diagnostics: boolean } | null = null;
const slot = new WasmSessionSlot(() => wasmModule);
const loop = new WasmTickLoop({
  slot,
  now: () => performance.now(),
  schedule: (callback, delay) => setTimeout(callback, delay),
  cancel: (timer) => clearTimeout(timer),
  post: (message) => ctx.postMessage(message),
  failure: postFailure,
});

type WorkerFailureDetails = HostFailure;

function structuredHostFailure(error: unknown, where: string): WorkerFailureDetails | null {
  const classified = classifyWasmFailure(error);
  if (classified.kind === "structured") {
    const source = error as Readonly<Record<string, unknown>>;
    const allowed = ["code", "where", "message", "cause", "context", "recoverable", "recoveryActions"];
    return parseHostFailure(Object.fromEntries(Object.entries(source).filter(([key]) => allowed.includes(key))), where);
  }
  if (classified.kind === "access-error") {
    return { code: "WASM_WORKER_PROTOCOL", where, message: `读取结构化错误失败：${classified.reason}` };
  }
  return null;
}

function workerFailureDetails(error: unknown, where: string): WorkerFailureDetails {
  const structured = structuredHostFailure(error, where);
  if (structured !== null) return structured;
  return { code: "WASM_WORKER_PROTOCOL", where, message: describeWasmFailure(error) };
}

function requireRecord(value: unknown, label: string): Readonly<Record<string, unknown>> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${label} 必须是对象`);
  }
  return value as Readonly<Record<string, unknown>>;
}

function readWasmCapabilities(wasm: typeof import("../../wasm-pkg/web_wasm.js")) {
  const capabilities = (wasm as WasmTransportExtensions).host_capabilities;
  return capabilities?.() ?? { npcDecisionDiagnostics: false };
}

function optionalNpcDecisionTrace(wasm: typeof import("../../wasm-pkg/web_wasm.js")): WasmNpcDecisionTrace | undefined {
  return (wasm as WasmTransportExtensions).npc_decision_trace;
}

export function postFailure(where: string, error: unknown): void {
  const failure = workerFailureDetails(error, where);
  postFailureDetails(where, failure);
}

function postFailureDetails(where: string, failure: WorkerFailureDetails): void {
  ctx.postMessage({ type: "failure", generation: slot.readGeneration(), ...failure, where: failure.where || where });
}

function postBaseline(): void {
  const [session, wasm] = slot.requireHandle();
  slot.prepareBaseline();
  ctx.postMessage({
    type: "baseline",
    generation: slot.readGeneration(),
    snapshot: wasm.snapshot(session),
    civilDate: wasm.civil_date(session),
  });
}

function postIngressBridge(): void {
  const [session, wasm] = slot.requireHandle();
  if (ingressBridge === null) throw new Error("共享 WASM ingress memory 尚未初始化");
  const token = (wasm as WasmTransportExtensions).ingress_token;
  if (token === undefined) throw new Error("当前 WASM bindings 缺少共享 ingress，请重建 bindings");
  ctx.postMessage({ type: "ingress", generation: slot.readGeneration(), token: token(session), ...ingressBridge });
}

function respondOperationError(message: WorkerMessage, error: unknown): void {
  ctx.postMessage({ type: "operationError", requestId: message.requestId, generation: message.generation, message: describeWasmFailure(error) });
}

async function initialize(requestedThreads: unknown): Promise<void> {
  if (wasmModule !== null) return;
  const threads = resolveThreadCount(navigator.hardwareConcurrency, requestedThreads);
  let diagnosticsWasm = false;
  let wasmUrl: URL;
  if (import.meta.env.DEV) {
    diagnosticsWasm = shouldLoadDiagnosticsWasm(true, import.meta.env.VITE_ENGINE_DIAGNOSTICS);
    if (diagnosticsWasm) {
      const diagnosticPackageUrl = new URL("/wasm-diagnostics-pkg/", self.location.origin);
      const moduleUrl = new URL("web_wasm.js", diagnosticPackageUrl);
      wasmModule = await import(/* @vite-ignore */ moduleUrl.href) as typeof import("../../wasm-pkg/web_wasm.js");
      wasmUrl = new URL("web_wasm_bg.wasm", diagnosticPackageUrl);
    } else {
      wasmModule = await import("../../wasm-pkg/web_wasm.js");
      wasmUrl = new URL("../../wasm-pkg/web_wasm_bg.wasm", import.meta.url);
    }
  } else {
    wasmModule = await import("../../wasm-pkg/web_wasm.js");
    wasmUrl = new URL("../../wasm-pkg/web_wasm_bg.wasm", import.meta.url);
  }
  const response = await fetch(wasmUrl);
  if (!response.ok) throw new Error(`加载 WASM 二进制失败：HTTP ${response.status}`);
  const module = await WebAssembly.compile(await response.arrayBuffer());
  const exports = await wasmModule.default(module);
  ingressBridge = { module, memory: exports.memory, diagnostics: diagnosticsWasm };
  await wasmModule.initThreadPool(threads);
  ctx.postMessage({ type: "ready", threads });
}

ctx.addEventListener("message", (event) => {
  const message = event.data;
  void (async () => {
    try {
      switch (message.type) {
        case "init":
          await initialize(message.threads);
          return;
        case "create": {
          slot.create(message.setup, message.seed);
          const [, wasm] = slot.requireHandle();
          const capabilities = readWasmCapabilities(wasm);
          postIngressBridge();
          ctx.postMessage({ type: "created", generation: slot.readGeneration(), capabilities: {
            ...capabilities,
            npcDecisionDiagnostics: capabilities.npcDecisionDiagnostics && optionalNpcDecisionTrace(wasm) !== undefined,
          } });
          postBaseline();
          return;
        }
        case "start": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          loop.start();
          ctx.postMessage({ type: "started", requestId: message.requestId, generation: requestedGeneration });
          return;
        }
        case "stop": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          loop.stop();
          ctx.postMessage({ type: "stopped", requestId: message.requestId, generation: requestedGeneration });
          return;
        }
        case "uiFrame":
          if (!Number.isSafeInteger(message.deliveryId) || Number(message.deliveryId) <= 0) throw new Error("Worker uiFrame deliveryId 无效");
          loop.acknowledge(Number(message.generation), Number(message.deliveryId));
          return;
        case "stepOnce": {
          if (!E2E_STEP_ENABLED) throw new Error("Worker 受控单步只允许在 E2E 构建中调用");
          const requestedGeneration = slot.requireGeneration(message.generation);
          if (loop.isRunning()) throw new Error("Worker 受控单步只允许在暂停状态执行");
          if (!loop.stepOnce()) throw new Error("Worker 受控单步失败");
          const [session, wasm] = slot.requireHandle();
          const committedTick = wasm.tick(session);
          if (committedTick > BigInt(Number.MAX_SAFE_INTEGER)) throw new Error("Worker 单步 tick 超出安全整数范围");
          ctx.postMessage({ type: "stepped", requestId: message.requestId, generation: requestedGeneration, tick: Number(committedTick) });
          return;
        }
        case "setSpeed": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          loop.setSpeed(Number(message.speed));
          ctx.postMessage({ type: "speedSet", requestId: message.requestId, generation: requestedGeneration });
          return;
        }
        case "setFrameRate":
          loop.setFrameRate(message.fps);
          return;
        case "setPausePreferences": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          const preferences = requireRecord(message.preferences, "暂停偏好");
          if (typeof preferences.pause_after_close !== "boolean" || typeof preferences.pause_before_open !== "boolean") {
            throw new Error("暂停偏好必须包含布尔值");
          }
          loop.setPausePreferences({
            pause_after_close: preferences.pause_after_close,
            pause_before_open: preferences.pause_before_open,
          });
          ctx.postMessage({ type: "pausePreferencesSet", requestId: message.requestId, generation: requestedGeneration });
          return;
        }
        case "speedMetrics": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          ctx.postMessage({ type: "speedMetrics", requestId: message.requestId, generation: requestedGeneration, metrics: loop.readSpeedMetrics() });
          return;
        }
        case "enqueue": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          const [session, wasm] = slot.requireHandle();
          wasm.enqueue(session, message.intent);
          ctx.postMessage({ type: "enqueued", requestId: message.requestId, generation: requestedGeneration });
          return;
        }
        case "save": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          const [session, wasm] = slot.requireHandle();
          const key = message.candidate;
          let saved: unknown;
          if (key === undefined) saved = wasm.save(session);
          else {
            const saveCandidate = (wasm as WasmTransportExtensions).save_candidate;
            if (saveCandidate === undefined) throw new Error("当前 WASM binding 不支持按日终候选保存，请重建 bindings");
            saved = saveCandidate(session, key as { readonly seq: number; readonly settledDate: string });
          }
          const savedSlot = parseSaveSlot(normalizeSerdeMaps(saved));
          ctx.postMessage({ type: "saved", requestId: message.requestId, generation: requestedGeneration, slot: savedSlot });
          return;
        }
        case "refreshBaseline": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          loop.flushForControl();
          const [session, wasm] = slot.requireHandle();
          slot.prepareBaseline();
          ctx.postMessage({
            type: "refreshed",
            requestId: message.requestId,
            generation: requestedGeneration,
            snapshot: wasm.snapshot(session),
            civilDate: wasm.civil_date(session),
          });
          return;
        }
        case "restore": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          const parsed = parseSaveSlot(message.slot);
          const restoredSnapshot = slot.restore(parsed, {
            wasRunning: loop.isRunning(),
            stop: () => loop.stop(),
            restart: () => { queueMicrotask(() => loop.start()); },
          });
          postIngressBridge();
          const [session, wasm] = slot.requireHandle();
          const civilDate = wasm.civil_date(session);
          ctx.postMessage({
            type: "restored",
            requestId: message.requestId,
            generation: requestedGeneration,
            nextGeneration: slot.readGeneration(),
            snapshot: restoredSnapshot,
            civilDate,
          });
          ctx.postMessage({ type: "baseline", generation: slot.readGeneration(), snapshot: restoredSnapshot, civilDate });
          return;
        }
        case "civilDate": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          const [session, wasm] = slot.requireHandle();
          ctx.postMessage({ type: "civilDate", requestId: message.requestId, generation: requestedGeneration, date: wasm.civil_date(session) });
          return;
        }
        case "endCivilDay": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          loop.requireDeliveryCapacity();
          const [session, wasm] = slot.requireHandle();
          loop.publish(wasm.end_civil_day(session));
          ctx.postMessage({ type: "civilDayEnded", requestId: message.requestId, generation: requestedGeneration });
          return;
        }
        case "submitReportCorrection": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          const [session, wasm] = slot.requireHandle();
          const submit = (wasm as WasmTransportExtensions).submit_report_correction;
          if (typeof submit !== "function") throw new Error("当前 WASM 产物缺少财报更正入口");
          submit(session, parseCompanyReportCorrection(message.request));
          ctx.postMessage({ type: "reportCorrectionSubmitted", requestId: message.requestId, generation: requestedGeneration });
          break;
        }
        case "cancelReportCorrection": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          const [session, wasm] = slot.requireHandle();
          const cancel = (wasm as WasmTransportExtensions).cancel_report_correction;
          if (typeof cancel !== "function") throw new Error("当前 WASM 产物缺少财报更正取消入口");
          if (typeof message.operationId !== "string" || message.operationId.trim().length === 0) throw new Error("更正 operation_id 必须非空");
          cancel(session, message.operationId);
          ctx.postMessage({ type: "reportCorrectionCancelled", requestId: message.requestId, generation: requestedGeneration });
          break;
        }
        case "queryReportCorrections": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          const [session, wasm] = slot.requireHandle();
          const query = (wasm as WasmTransportExtensions).query_report_corrections;
          if (typeof query !== "function") throw new Error("当前 WASM 产物缺少财报更正查询入口");
          const value = parseReportCorrectionStatus(query(session));
          ctx.postMessage({ type: "reportCorrections", requestId: message.requestId, generation: requestedGeneration, value });
          break;
        }
        case "publicReports": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          const [session, wasm] = slot.requireHandle();
          ctx.postMessage({ type: "publicReports", requestId: message.requestId, generation: requestedGeneration, page: normalizePublicReportPage(wasm.public_report_page(session, message.query) ) });
          return;
        }
        case "publicReportAvailability": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          const [session, wasm] = slot.requireHandle();
          const queryAvailability = (wasm as WasmTransportExtensions).public_report_availability;
          if (queryAvailability === undefined) throw new Error("当前 WASM bindings 不支持公开报告可用性查询，请重建 bindings");
          ctx.postMessage({ type: "publicReportAvailability", requestId: message.requestId, generation: requestedGeneration, availability: normalizePublicReportAvailability(queryAvailability(session, message.query)) });
          return;
        }
        case "playerWorkingOrders": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          const [session, wasm] = slot.requireHandle();
          const readOrders = (wasm as WasmTransportExtensions).player_working_orders;
          if (readOrders === undefined) throw new Error("当前 WASM bindings 不支持玩家活动委托查询，请重建 bindings");
          ctx.postMessage({
            type: "playerWorkingOrders",
            requestId: message.requestId,
            generation: requestedGeneration,
            orders: readOrders(session),
          });
          return;
        }
        case "stockHistory": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          if (typeof message.code !== "string" || message.code.length === 0) throw new Error("股票历史查询代码无效");
          const [session, wasm] = slot.requireHandle();
          const queryHistory = (wasm as WasmTransportExtensions).query_stock_history;
          if (queryHistory === undefined) throw new Error("当前 WASM bindings 不支持股票历史查询，请重建 bindings");
          ctx.postMessage({ type: "stockHistory", requestId: message.requestId, generation: requestedGeneration, data: queryHistory(session, message.code) });
          return;
        }
        case "initialAllocation": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          const [session, wasm] = slot.requireHandle();
          const queryAllocation = (wasm as WasmTransportExtensions).initial_allocation;
          if (queryAllocation === undefined) throw new Error("当前 WASM bindings 不支持初始分配查询，请重建 bindings");
          ctx.postMessage({ type: "initialAllocation", requestId: message.requestId, generation: requestedGeneration, data: queryAllocation(session) });
          return;
        }
        case "npcDecisionTrace": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          if (typeof message.account !== "string" || !/^(0|[1-9]\d*)$/.test(message.account) || message.account.length > 20 || BigInt(message.account) > 18_446_744_073_709_551_615n) throw new Error("NPC 账户 ID 必须是规范 u64 非负十进制字符串");
          const [session, wasm] = slot.requireHandle();
          const capabilities = readWasmCapabilities(wasm);
          const trace = optionalNpcDecisionTrace(wasm);
          if (!capabilities.npcDecisionDiagnostics || trace === undefined) throw new Error("当前 WASM 构建未启用 NPC 决策诊断");
          ctx.postMessage({
            type: "npcDecisionTrace",
            requestId: message.requestId,
            generation: requestedGeneration,
            records: trace(session, BigInt(message.account)),
          });
          return;
        }
        case "calculateIndicators": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          if (!Number.isSafeInteger(message.requestId) || Number(message.requestId) < 0) throw new Error("指标请求 ID 无效");
          const prices = message.prices;
          const candles = message.candles;
          if (!Array.isArray(prices) || !prices.every((price) => typeof price === "number" && Number.isFinite(price))) throw new Error("指标 prices 必须是有限数值数组");
          if (!Array.isArray(candles)) throw new Error("指标 candles 必须是数组");
          if (!candles.every((candle) => candle !== null && typeof candle === "object"
            && Number.isFinite((candle as Record<string, unknown>).high)
            && Number.isFinite((candle as Record<string, unknown>).low)
            && Number.isFinite((candle as Record<string, unknown>).close))) throw new Error("指标 candles 必须包含有限 high/low/close");
          if (!candles.every((candle) => {
            const bar = candle as Record<string, number>;
            return bar.high >= bar.low && bar.close >= bar.low && bar.close <= bar.high;
          })) throw new Error("指标 candles 必须满足 low ≤ close ≤ high");
          const [, wasm] = slot.requireHandle();
          const calculateIndicators = (wasm as WasmTransportExtensions).calculate_indicators;
          if (calculateIndicators === undefined) throw new Error("当前 WASM bindings 不支持指标计算，请重建 bindings");
          const result = calculateIndicators(prices, candles);
          ctx.postMessage({ type: "indicatorsCalculated", requestId: message.requestId, generation: requestedGeneration, result });
          return;
        }
        case "calculateIntradayAverage": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          if (!Number.isSafeInteger(message.requestId) || Number(message.requestId) < 0) throw new Error("VWAP 请求 ID 无效");
          const turnoverCents = parseTurnoverCents(message.turnoverCents, "VWAP turnoverCents");
          if (!Number.isSafeInteger(message.tradeCount) || Number(message.tradeCount) < 0) throw new Error("VWAP tradeCount 必须是非负安全整数");
          if (!Number.isSafeInteger(message.volumeShares) || Number(message.volumeShares) < 0) throw new Error("VWAP volumeShares 必须是非负安全整数");
          const [, wasm] = slot.requireHandle();
          const calculateAverage = (wasm as WasmTransportExtensions).calculate_intraday_average;
          if (calculateAverage === undefined) throw new Error("当前 WASM bindings 不支持 VWAP，请重建 bindings");
          const result = calculateAverage(turnoverCents, BigInt(Number(message.tradeCount)), BigInt(Number(message.volumeShares)));
          ctx.postMessage({ type: "intradayAverageCalculated", requestId: message.requestId, generation: requestedGeneration, result });
          return;
        }
        case "calculateIntradayAverageCurve": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          if (!Number.isSafeInteger(message.requestId) || Number(message.requestId) < 0) throw new Error("VWAP curve 请求 ID 无效");
          if (typeof message.seriesKey !== "string" || message.seriesKey.length === 0 || message.seriesKey.length > 256 || !Array.isArray(message.samples) || message.samples.length > 600) throw new Error("VWAP curve 请求字段无效");
          const [, wasm] = slot.requireHandle();
          const calculateCurve = (wasm as WasmTransportExtensions).calculate_intraday_average_curve;
          if (calculateCurve === undefined) throw new Error("当前 WASM bindings 不支持 VWAP curve，请重建 bindings");
          const samples = message.samples.map((item) => {
            if (item === null || typeof item !== "object" || Array.isArray(item)) throw new Error("VWAP curve sample 必须是对象");
            const sample = item as Record<string, unknown>;
            const turnoverCents = parseTurnoverCents(sample.turnoverCents, "VWAP curve turnoverCents");
            if (!Number.isSafeInteger(sample.tradeCount) || Number(sample.tradeCount) < 0
              || !Number.isSafeInteger(sample.volumeShares) || Number(sample.volumeShares) < 0) throw new Error("VWAP curve sample 字段无效");
            return { turnover_cents: turnoverCents, trade_count: Number(sample.tradeCount), volume_shares: Number(sample.volumeShares) };
          });
          const results = calculateCurve(samples);
          ctx.postMessage({ type: "intradayAverageCurveCalculated", requestId: message.requestId, generation: requestedGeneration, seriesKey: message.seriesKey, results });
          return;
        }
        case "currentMinuteHistory": {
          const generation = slot.requireGeneration(message.generation);
          if (!Number.isSafeInteger(message.requestId) || Number(message.requestId) < 0) throw new Error("当前分钟requestId无效");
          const request = normalizeCurrentMinuteHistoryRequest(message.query);
          const [session, wasm] = slot.requireHandle();
          const query = (wasm as WasmTransportExtensions).current_minute_history;
          if (query === undefined) throw new Error("当前WASM bindings不支持当前分钟，请重建bindings");
          ctx.postMessage({ type: "currentMinuteHistory", requestId: message.requestId, generation, response: query(session, request) });
          return;
        }
        case "marketHistory": {
          const generation = slot.requireGeneration(message.generation);
          if (!Number.isSafeInteger(message.requestId) || Number(message.requestId) < 0) throw new Error("量价历史requestId无效");
          const request = normalizeMarketHistoryRequest(message.query);
          const [session, wasm] = slot.requireHandle();
          const query = (wasm as WasmTransportExtensions).market_history;
          if (query === undefined) throw new Error("当前WASM bindings不支持永久量价历史，请重建bindings");
          ctx.postMessage({ type: "marketHistory", requestId: message.requestId, generation, page: query(session, request) });
          return;
        }
        case "personalTradeHistory": {
          const generation = slot.requireGeneration(message.generation);
          if (!Number.isSafeInteger(message.requestId) || Number(message.requestId) < 0) throw new Error("日期交割历史requestId无效");
          const [session, wasm] = slot.requireHandle();
          const query = (wasm as WasmTransportExtensions).personal_trade_history;
          if (query === undefined) throw new Error("当前WASM bindings不支持日期交割历史，请重建bindings");
          ctx.postMessage({ type: "personalTradeHistory", requestId: message.requestId, generation, page: query(session, message.query) });
          return;
        }
        case "personalTradeConfirmations": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          if (!Number.isSafeInteger(message.requestId) || Number(message.requestId) < 0) throw new Error("交割单请求 ID 无效");
          const [session, wasm] = slot.requireHandle();
          const queryConfirmations = (wasm as WasmTransportExtensions).personal_trade_confirmations;
          if (queryConfirmations === undefined) throw new Error("当前 WASM bindings 不支持本人交割单，请重建 bindings");
          if (message.beforeReceipt !== null && (typeof message.beforeReceipt !== "string" || message.beforeReceipt.length > 20 || !/^(0|[1-9]\d*)$/.test(message.beforeReceipt) || BigInt(message.beforeReceipt) > 18_446_744_073_709_551_615n)) throw new Error("交割单 beforeReceipt 必须是规范u64字符串或null");
          ctx.postMessage({ type: "personalTradeConfirmations", requestId: message.requestId, generation: requestedGeneration, confirmations: queryConfirmations(session, message.beforeReceipt) });
          return;
        }
        case "publicReportById": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          const [session, wasm] = slot.requireHandle();
          ctx.postMessage({ type: "publicReportById", requestId: message.requestId, generation: requestedGeneration, report: normalizePublicReportById(wasm.public_report_by_id(session, String(message.id))) });
          return;
        }
        case "dividendTaxStatus": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          const [session, wasm] = slot.requireHandle();
          const queryStatus = (wasm as WasmTransportExtensions).owner_dividend_tax_status;
          if (queryStatus === undefined) throw new Error("当前 WASM bindings 不支持股息税状态查询，请重建 bindings");
          ctx.postMessage({ type: "dividendTaxStatus", requestId: message.requestId, generation: requestedGeneration, status: queryStatus(session) });
          return;
        }
        case "dividendTaxOutstanding": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          const [session, wasm] = slot.requireHandle();
          const queryOutstanding = (wasm as WasmTransportExtensions).owner_dividend_tax_outstanding_views;
          if (queryOutstanding === undefined) throw new Error("当前 WASM bindings 不支持股息税未清税额查询，请重建 bindings");
          ctx.postMessage({ type: "dividendTaxOutstanding", requestId: message.requestId, generation: requestedGeneration, views: queryOutstanding(session) });
          return;
        }
        case "rightsRejections": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          if (!Number.isSafeInteger(message.requestId) || Number(message.requestId) < 0) throw new Error("配股认购拒绝回执请求 ID 无效");
          const [session, wasm] = slot.requireHandle();
          const queryRejected = (wasm as WasmTransportExtensions).owner_rejected_rights_subscriptions;
          if (queryRejected === undefined) throw new Error("当前 WASM bindings 不支持配股认购拒绝回执查询，请重建 bindings");
          ctx.postMessage({ type: "rightsRejections", requestId: message.requestId, generation: requestedGeneration, receipts: queryRejected(session) });
          return;
        }
        case "preferenceRejections": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          if (!Number.isSafeInteger(message.requestId) || Number(message.requestId) < 0) throw new Error("偏好台账请求 ID 无效");
          if (typeof message.company !== "string" || message.company.trim().length === 0 || message.company.length > 64) throw new Error("偏好台账公司身份必须是非空且不超过 64 字符的字符串");
          const [session, wasm] = slot.requireHandle();
          const queryLedger = (wasm as WasmTransportExtensions).company_preference_rejections;
          if (queryLedger === undefined) throw new Error("当前 WASM bindings 不支持偏好拒绝台账查询，请重建 bindings");
          ctx.postMessage({ type: "preferenceRejections", requestId: message.requestId, generation: requestedGeneration, rejections: queryLedger(session, message.company) });
          return;
        }
        case "drop": {
          loop.stop();
          slot.drop();
          return;
        }
        default:
          throw new Error(`未知 Worker 消息：${message.type}`);
      }
    } catch (error) {
      const structuredFailure = structuredHostFailure(error, `wasm-worker.${message.type}`);
      if (structuredFailure !== null) {
        loop.stop();
        if (structuredFailure.code === "REPORT_CORRECTION_REJECTED" && structuredFailure.recoverable === true && typeof message.requestId === "number") respondOperationError(message, error);
        postFailureDetails(`wasm-worker.${message.type}`, structuredFailure);
      } else if (typeof message.requestId === "number") {
        respondOperationError(message, error);
      } else {
        postFailure("wasm-worker.message", error);
      }
    }
  })();
});
