import { parseSaveSlot } from "../save/save-schema.ts";
import { normalizePublicReportById, normalizePublicReportPage, normalizeSerdeMaps } from "./serde-normalize.ts";
import { WasmSessionSlot } from "./wasm-session-slot.ts";
import { WasmTickLoop } from "./wasm-tick-loop.ts";
import { WorkerTaskScheduler } from "./worker-task-scheduler.ts";
import { classifyWasmFailure, describeWasmFailure } from "./wasm-failure.ts";
import { parseHostFailure } from "./protocol-failure.ts";
import { stockIdOf } from "../config/stock-ids.ts";
import { shouldLoadDiagnosticsWasm } from "./wasm-build-mode.ts";
import type { HostFailure } from "./host-update.ts";
import type { NpcDecisionTraceRecord } from "./npc-decision-trace.ts";

type WorkerMessage = Readonly<Record<string, unknown>> & { readonly type: string };
type WasmNpcDecisionTrace = (handle: number, account: bigint) => readonly NpcDecisionTraceRecord[];
// 旧 bindings 的声明文件仍把 query_stock_history 标为 code: string；按 ADR 0041 以
// 数值 StockId（u32）覆盖该成员，重建 bindings 后声明亦为数值。
type WasmTransportExtensions = Omit<typeof import("../../wasm-pkg/web_wasm.js"), "query_stock_history"> & {
  readonly ingress_token?: (handle: number) => number;
  readonly prepare_public_baseline?: (handle: number) => void;
  readonly host_capabilities?: () => { readonly npcDecisionDiagnostics: boolean; readonly indicatorCapabilities?: { readonly intradayAverage: boolean; readonly macd: boolean; readonly priceKdj: boolean; readonly candleKdj: boolean }; readonly personalTradeHistory?: boolean };
  readonly player_working_orders?: (handle: number) => unknown;
  readonly query_stock_history?: (handle: number, code: number) => unknown;
  readonly initial_allocation?: (handle: number) => unknown;
  readonly calculate_indicators?: (prices: number[], candles: readonly { high: number; low: number; close: number }[]) => unknown;
  readonly npc_decision_trace?: WasmNpcDecisionTrace;
  readonly save_candidate?: (handle: number, key: { readonly seq: number; readonly settledDate: string }) => unknown;
  /** W3 查询面（B2 查询 API 的 wasm 绑定——请求已由主线程译为数值 StockId wire 形态）。 */
  readonly market_history?: (handle: number, query: unknown) => unknown;
  readonly current_minute_history?: (handle: number, query: unknown) => unknown;
  readonly personal_trade_history?: (handle: number, query: unknown) => unknown;
  readonly personal_trade_confirmations?: (handle: number, beforeReceipt: bigint | null) => unknown;
  readonly calculate_intraday_average?: (turnoverCents: string, tradeCount: bigint, volumeShares: bigint) => unknown;
  readonly calculate_intraday_average_curve?: (samples: unknown) => unknown;
};
type WorkerPort = {
  postMessage(message: unknown): void;
  addEventListener(type: "message", listener: (event: MessageEvent<WorkerMessage>) => void): void;
};

/** 把 bindings 模块视为传输扩展面。
 * 旧 bindings 声明（query_stock_history 的 code: string）与 ADR 0041 数值签名不重叠，
 * 直接 as 断言会被 TS2352 拒绝，故经 unknown 中转；所有扩展成员均为可选探测，
 * 运行期缺失时各调用点已显式报错并提示重建 bindings。 */
function transportExtensions(wasm: typeof import("../../wasm-pkg/web_wasm.js")): WasmTransportExtensions {
  return wasm as unknown as WasmTransportExtensions;
}

const ctx: WorkerPort = self;

export function isE2EStepMode(mode: unknown): boolean {
  return mode === "e2e";
}

const E2E_STEP_ENABLED = isE2EStepMode(import.meta.env?.MODE);

let wasmModule: typeof import("../../wasm-pkg/web_wasm.js") | null = null;
let ingressBridge: { module: WebAssembly.Module; memory: WebAssembly.Memory; diagnostics: boolean } | null = null;
const slot = new WasmSessionSlot(() => wasmModule);
const taskScheduler = new WorkerTaskScheduler();
const loop = new WasmTickLoop({
  slot,
  now: () => performance.now(),
  schedule: (callback, delay) => taskScheduler.schedule(callback, delay),
  cancel: (timer) => taskScheduler.cancel(timer),
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
  const capabilities = transportExtensions(wasm).host_capabilities;
  // 旧 bindings 只带 npcDecisionDiagnostics——指标源与个人历史按明确不支持
  // 处理（缺省 false，消费点显式报错不回退）。
  return capabilities?.() ?? { npcDecisionDiagnostics: false, indicatorCapabilities: { intradayAverage: false, macd: false, priceKdj: false, candleKdj: false }, personalTradeHistory: false };
}

function optionalNpcDecisionTrace(wasm: typeof import("../../wasm-pkg/web_wasm.js")): WasmNpcDecisionTrace | undefined {
  return transportExtensions(wasm).npc_decision_trace;
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
  const token = transportExtensions(wasm).ingress_token;
  if (token === undefined) throw new Error("当前 WASM bindings 缺少共享 ingress，请重建 bindings");
  ctx.postMessage({ type: "ingress", generation: slot.readGeneration(), token: token(session), ...ingressBridge });
}

function respondOperationError(message: WorkerMessage, error: unknown): void {
  ctx.postMessage({ type: "operationError", requestId: message.requestId, generation: message.generation, message: describeWasmFailure(error) });
}

async function initialize(): Promise<void> {
  if (wasmModule !== null) return;
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
  ctx.postMessage({ type: "ready" });
}

ctx.addEventListener("message", (event) => {
  const message = event.data;
  void (async () => {
    try {
      switch (message.type) {
        case "init":
          await initialize();
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
            const saveCandidate = transportExtensions(wasm).save_candidate;
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
        case "publicReports": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          const [session, wasm] = slot.requireHandle();
          ctx.postMessage({ type: "publicReports", requestId: message.requestId, generation: requestedGeneration, page: normalizePublicReportPage(wasm.public_report_page(session, message.query) ) });
          return;
        }
        case "playerWorkingOrders": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          const [session, wasm] = slot.requireHandle();
          const readOrders = transportExtensions(wasm).player_working_orders;
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
          const queryHistory = transportExtensions(wasm).query_stock_history;
          if (queryHistory === undefined) throw new Error("当前 WASM bindings 不支持股票历史查询，请重建 bindings");
          // 请求层翻译：主线程传来的代码串 code 译为数值 StockId 再调用 wasm FFI（u32）。
          ctx.postMessage({ type: "stockHistory", requestId: message.requestId, generation: requestedGeneration, data: queryHistory(session, stockIdOf(message.code)) });
          return;
        }
        // W3 查询面：query 已由主线程归一并译为数值 StockId wire 形态（ADR-0041），
        // worker 只做绑定存在性检查与透传；错误经 operationError 原文回传。
        case "marketHistory": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          const [session, wasm] = slot.requireHandle();
          const query = transportExtensions(wasm).market_history;
          if (query === undefined) throw new Error("当前 WASM bindings 不支持保留历史查询，请重建 bindings");
          ctx.postMessage({ type: "marketHistory", requestId: message.requestId, generation: requestedGeneration, page: query(session, message.query) });
          return;
        }
        case "currentMinuteHistory": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          const [session, wasm] = slot.requireHandle();
          const query = transportExtensions(wasm).current_minute_history;
          if (query === undefined) throw new Error("当前 WASM bindings 不支持当日分钟K查询，请重建 bindings");
          ctx.postMessage({ type: "currentMinuteHistory", requestId: message.requestId, generation: requestedGeneration, response: query(session, message.query) });
          return;
        }
        case "personalTradeHistory": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          const [session, wasm] = slot.requireHandle();
          const query = transportExtensions(wasm).personal_trade_history;
          if (query === undefined) throw new Error("当前 WASM bindings 不支持本人交割历史查询，请重建 bindings");
          ctx.postMessage({ type: "personalTradeHistory", requestId: message.requestId, generation: requestedGeneration, page: query(session, message.query) });
          return;
        }
        case "personalTradeConfirmations": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          if (message.beforeReceipt !== null && typeof message.beforeReceipt !== "string") throw new Error("本人交割单 beforeReceipt 必须是规范 u64 字符串或 null");
          const before = message.beforeReceipt === null ? null : BigInt(message.beforeReceipt);
          const [session, wasm] = slot.requireHandle();
          const query = transportExtensions(wasm).personal_trade_confirmations;
          if (query === undefined) throw new Error("当前 WASM bindings 不支持本人交割单查询，请重建 bindings");
          ctx.postMessage({ type: "personalTradeConfirmations", requestId: message.requestId, generation: requestedGeneration, confirmations: query(session, before) });
          return;
        }
        case "calculateIntradayAverage": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          const turnoverCents = message.turnoverCents;
          const tradeCount = message.tradeCount;
          const volumeShares = message.volumeShares;
          if (typeof turnoverCents !== "string" || typeof tradeCount !== "number" || !Number.isSafeInteger(tradeCount) || typeof volumeShares !== "number" || !Number.isSafeInteger(volumeShares)) throw new Error("VWAP 输入必须是规范字符串额与安全整数");
          const [, wasm] = slot.requireHandle();
          const calculateAverage = transportExtensions(wasm).calculate_intraday_average;
          if (calculateAverage === undefined) throw new Error("当前 WASM bindings 不支持 VWAP，请重建 bindings");
          ctx.postMessage({ type: "intradayAverageCalculated", requestId: message.requestId, generation: requestedGeneration, result: calculateAverage(turnoverCents, BigInt(tradeCount), BigInt(volumeShares)) });
          return;
        }
        case "calculateIntradayAverageCurve": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          if (typeof message.seriesKey !== "string" || !Array.isArray(message.samples)) throw new Error("VWAP curve 请求必须是字符串键与样本数组");
          const [, wasm] = slot.requireHandle();
          const calculateCurve = transportExtensions(wasm).calculate_intraday_average_curve;
          if (calculateCurve === undefined) throw new Error("当前 WASM bindings 不支持 VWAP curve，请重建 bindings");
          ctx.postMessage({ type: "intradayAverageCurveCalculated", requestId: message.requestId, generation: requestedGeneration, seriesKey: message.seriesKey, results: calculateCurve(message.samples) });
          return;
        }
        case "initialAllocation": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          const [session, wasm] = slot.requireHandle();
          const queryAllocation = transportExtensions(wasm).initial_allocation;
          if (queryAllocation === undefined) throw new Error("当前 WASM bindings 不支持初始分配查询，请重建 bindings");
          ctx.postMessage({ type: "initialAllocation", requestId: message.requestId, generation: requestedGeneration, data: queryAllocation(session) });
          return;
        }
        case "npcDecisionTrace": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          if (!Number.isSafeInteger(message.account) || Number(message.account) < 0) throw new Error("NPC 账户 ID 必须是非负安全整数");
          const [session, wasm] = slot.requireHandle();
          const capabilities = readWasmCapabilities(wasm);
          const trace = optionalNpcDecisionTrace(wasm);
          if (!capabilities.npcDecisionDiagnostics || trace === undefined) throw new Error("当前 WASM 构建未启用 NPC 决策诊断");
          ctx.postMessage({
            type: "npcDecisionTrace",
            requestId: message.requestId,
            generation: requestedGeneration,
            records: trace(session, BigInt(Number(message.account))),
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
          const calculateIndicators = transportExtensions(wasm).calculate_indicators;
          if (calculateIndicators === undefined) throw new Error("当前 WASM bindings 不支持指标计算，请重建 bindings");
          const result = calculateIndicators(prices, candles);
          ctx.postMessage({ type: "indicatorsCalculated", requestId: message.requestId, generation: requestedGeneration, result });
          return;
        }
        case "publicReportById": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          const [session, wasm] = slot.requireHandle();
          ctx.postMessage({ type: "publicReportById", requestId: message.requestId, generation: requestedGeneration, report: normalizePublicReportById(wasm.public_report_by_id(session, String(message.id))) });
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
        postFailureDetails(`wasm-worker.${message.type}`, structuredFailure);
      } else if (typeof message.requestId === "number") {
        respondOperationError(message, error);
      } else {
        postFailure("wasm-worker.message", error);
      }
    }
  })();
});
