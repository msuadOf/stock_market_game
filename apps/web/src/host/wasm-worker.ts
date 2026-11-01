import { parseVerifiedRestoreCapability } from "./wasm-restore-package-capability.ts";
import { WasmRestartGate } from "./wasm-restart-gate.ts";
import { prepareBaselineUpdate } from "./host-update.ts";
import { parseSaveSlot } from "../save/save-schema.ts";
import { normalizePublicReportById, normalizePublicReportPage, normalizeSerdeMaps } from "./serde-normalize.ts";
import { WasmSessionSlot } from "./wasm-session-slot.ts";
import { WasmTickLoop } from "./wasm-tick-loop.ts";
import { WorkerTaskScheduler } from "./worker-task-scheduler.ts";
import { classifyWasmFailure, describeWasmFailure, describeReadFailureFields, type WasmFailureClassification } from "./wasm-failure.ts";
import { parseHostFailure } from "./protocol-failure.ts";
import { CompanyWriteRejectedError, parseCompanyWriteFailure, type CompanyWriteFailure } from "./company-write.ts";
import { CompanyReadRejectedError, parseCompanyReadFailure, type CompanyReadFailure } from "./company-read-failure.ts";
import { exact } from "../save/schema/primitives.ts";
import { shouldLoadDiagnosticsWasm } from "./wasm-build-mode.ts";
import type { VerifiedWasmRestoreCapability } from "./wasm-document-memory-owner.ts";
import type { HostFailure } from "./host-update.ts";
import type { NpcDecisionTraceRecord } from "./npc-decision-trace.ts";
import { createFacadeEngineInitializer, type FacadeEngineGlue } from "./facade-engine-initialize.ts";
import { bindFacadeThreadEnvironment, installFacadeSlotLayout, getArenaView } from "./engine-clock-env.ts";

type WorkerMessage = Readonly<Record<string, unknown>> & { readonly type: string };
type WasmNpcDecisionTrace = (handle: number, account: bigint) => readonly NpcDecisionTraceRecord[];
// 旧 bindings 的声明文件仍把 query_stock_history 标为 code: string；按 ADR 0041 以
// 数值 StockId（u32）覆盖该成员，重建 bindings 后声明亦为数值。
type WasmTransportExtensions = Omit<typeof import("../../wasm-pkg/web_wasm.js"), "query_stock_history"> & {
  readonly shutdown_session?: (handle: number) => unknown;
  readonly ingress_token?: (handle: number) => number;
  readonly prepare_public_baseline?: (handle: number) => void;
  readonly stock_catalog_view: (handle: number) => unknown;
  readonly company_catalog_view: (handle: number) => unknown;
  readonly host_capabilities?: () => { readonly npcDecisionDiagnostics: boolean; readonly indicatorCapabilities?: { readonly intradayAverage: boolean; readonly macd: boolean; readonly priceKdj: boolean; readonly candleKdj: boolean }; readonly personalTradeHistory?: boolean };
  readonly player_working_orders?: (handle: number) => unknown;
  readonly query_stock_history?: (handle: number, code: number) => unknown;
  readonly initial_allocation?: (handle: number) => unknown;
  readonly company_read_query?: (handle: number, query: unknown) => unknown;
  readonly propose_player_company_action?: (handle: number, proposal: unknown) => unknown;
  readonly enqueue_report_correction?: (handle: number, request: unknown) => unknown;
  readonly subscribe_rights_offering?: (handle: number, eventId: unknown, requestedShares: unknown) => unknown;
  readonly cancel_report_correction?: (handle: number, operationId: unknown) => unknown;
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
  reportError?(error: unknown): void;
  addEventListener(type: "message", listener: (event: MessageEvent<WorkerMessage>) => void): void;
};

/** 把 bindings 模块视为传输扩展面。
 * 旧 bindings 声明（query_stock_history 的 code: string）与 ADR 0041 数值签名不重叠，
 * 直接 as 断言会被 TS2352 拒绝，故经 unknown 中转；所有扩展成员均为可选探测，
 * 运行期缺失时各调用点已显式报错并提示重建 bindings。 */
function transportExtensions(wasm: typeof import("../../wasm-pkg/web_wasm.js")): WasmTransportExtensions {
  return wasm as unknown as WasmTransportExtensions;
}

let businessPort: MessagePort | undefined;
let engineControlPort: MessagePort | undefined;
let ownerInstanceId: number | undefined;
let initializedTransport = false;
const ctx: WorkerPort = {
  postMessage(message) {
    if (businessPort === undefined) throw new Error("engine business port 尚未绑定");
    businessPort.postMessage(message);
  },
  reportError(error) {
    if (typeof self.reportError === "function") self.reportError(error);
    else queueMicrotask(() => { throw error; });
  },
  addEventListener() { throw new Error("engine business listener 必须由 transfer 后的 dataPort 接收"); },
};

export function isE2EStepMode(mode: unknown): boolean {
  return mode === "e2e";
}

const E2E_STEP_ENABLED = isE2EStepMode(import.meta.env?.MODE);

let wasmModule: typeof import("../../wasm-pkg/web_wasm.js") | null = null;
// shutdown export 是可选扩展；保留其余真实 generated 签名，运行期由 consuming slot 探测。
const slot = new WasmSessionSlot(() => wasmModule as (typeof import("../../wasm-pkg/web_wasm.js") & Pick<WasmTransportExtensions, "shutdown_session">) | null);
const taskScheduler = new WorkerTaskScheduler();
const loop = new WasmTickLoop({
  slot,
  now: () => performance.now(),
  schedule: (callback, delay) => taskScheduler.schedule(callback, delay),
  cancel: (timer) => taskScheduler.cancel(timer),
  post: (message) => ctx.postMessage(message),
  failure: postFailure,
});
const engineInitializer = createFacadeEngineInitializer({
  ownsIncomingArchive: true,
  isolated: () => self.crossOriginIsolated === true,
  environment: { bindFacadeThreadEnvironment, installFacadeSlotLayout, getArenaView },
  load: loadEngineModule,
  onFailure(error) { loop.stop(); postFailure("wasm-worker.facade-control", error); },
});

function bindEngineTransport(value: unknown): void {
  if (initializedTransport) throw new Error("engine transport 只允许绑定一次");
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error("engine transport 必须是对象");
  const source = value as Record<string, unknown>;
  if (Object.keys(source).sort().join(",") !== "controlPort,dataPort,instanceId,type" || source.type !== "bindEngineTransport"
    || !Number.isSafeInteger(source.instanceId) || Number(source.instanceId) < 1
    || !(source.controlPort instanceof MessagePort) || !(source.dataPort instanceof MessagePort)) throw new Error("engine transport 字段无效");
  initializedTransport = true;
  businessPort = source.dataPort;
  engineControlPort = source.controlPort;
  ownerInstanceId = Number(source.instanceId);
  businessPort.addEventListener("message", (event) => { void handleBusinessMessage(event as MessageEvent<WorkerMessage>); });
  businessPort.addEventListener("messageerror", (event) => postFailure("wasm-worker.business-port", event));
  businessPort.start();
}

type WorkerFailureDetails = HostFailure;

function structuredHostFailure(error: unknown, where: string, classified: Exclude<WasmFailureClassification, { readonly kind: "unstructured" }>): WorkerFailureDetails {
  if (classified.kind === "structured") {
    const source = error as Readonly<Record<string, unknown>>;
    const details: Record<string, unknown> = { code: classified.code, message: classified.message };
    let phase = "读取结构化错误详情失败";
    try {
      for (const key of ["where", "cause", "context", "recoverable", "recoveryActions"]) {
        phase = `读取结构化错误字段 ${key} 失败`;
        if (!Object.hasOwn(source, key)) continue;
        const value = source[key];
        // 先克隆后校验，避免 parser 与传输分别触发嵌套 getter 而读取不同值。
        phase = `克隆结构化错误字段 ${key} 失败`;
        details[key] = structuredClone(value);
      }
      phase = "校验结构化错误详情失败";
      return parseHostFailure(details, where);
    } catch (detailError) {
      // details 仅含已读取且可克隆的快照；保留原故障，显式报告详情处理的失败。
      return {
        code: "WASM_WORKER_PROTOCOL", where,
        message: `${phase}：${describeWasmFailure(detailError)}${describeReadFailureFields({ code: classified.code, message: classified.message, where: details.where })}`,
        cause: details,
      };
    }
  }
  return {
    code: "WASM_WORKER_PROTOCOL", where,
    message: `读取结构化错误字段 ${classified.field} 失败：${classified.reason}${describeReadFailureFields(classified.cause)}`,
    ...(Object.keys(classified.cause).length > 0 ? { cause: classified.cause } : {}),
  };
}

function unstructuredFailureDetails(message: string, where: string, cause?: Readonly<Record<string, unknown>>): WorkerFailureDetails {
  if (message.trim().length === 0) {
    return { code: "WASM_WORKER_PROTOCOL", where, message: "非结构化错误没有可读的非空 message", cause: { ...cause, message } };
  }
  return {
    code: "WASM_WORKER_PROTOCOL", where, message,
    ...(cause !== undefined && (Object.hasOwn(cause, "code") || Object.hasOwn(cause, "where")) ? { cause } : {}),
  };
}

function workerFailureDetails(error: unknown, where: string): WorkerFailureDetails {
  const classified = classifyWasmFailure(error);
  if (classified.kind === "unstructured") return unstructuredFailureDetails(classified.description, where, classified.cause);
  return structuredHostFailure(error, where, classified);
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

function postFailureDetails(where: string, failure: WorkerFailureDetails, initializationQuiescent: false | "initialization-failed" | "initial-session-failed" = false, allocationKnownAbsent = false): void {
  const generation = slot.readGeneration();
  try {
    ctx.postMessage({ type: "failure", generation, ...failure, ...(initializationQuiescent ? { engineQuiescent: initializationQuiescent } : {}), ...(allocationKnownAbsent ? { memoryAllocation: "not-allocated" } : {}) });
  } catch (sendError) {
    const sendReason = describeWasmFailure(sendError);
    try {
      ctx.postMessage({
        type: "failure", generation, code: "WASM_WORKER_PROTOCOL", where,
        ...(initializationQuiescent ? { engineQuiescent: initializationQuiescent } : {}), ...(allocationKnownAbsent ? { memoryAllocation: "not-allocated" } : {}),
        message: `发送结构化错误详情失败：${sendReason}`,
        cause: failure,
      });
    } catch (lastResortError) {
      const lastReason = describeWasmFailure(lastResortError);
      const reportingError = new Error(
        `WASM Worker 错误报告通道双失败（${where}）：${failure.code}：${failure.message}；原始位置：${failure.where}；首次发送：${sendReason}；再次发送：${lastReason}`,
        { cause: { failure, sendError: sendReason, lastResortError: lastReason } },
      );
      // reportError 经 Worker error 事件进入 worker-host 的用户错误通道。
      // 旧环境用独立 microtask 抛出，避免 async listener 的 rejected Promise 丢失。
      if (typeof ctx.reportError === "function") ctx.reportError(reportingError);
      else queueMicrotask(() => { throw reportingError; });
    }
  }
}

const restoreRestart = new WasmRestartGate();
let fatalSession = false;
let closingSession = false;

function postBaseline(): void {
  const [session, wasm] = slot.requireHandle();
  slot.prepareBaseline();
  ctx.postMessage({
    type: "baseline",
    generation: slot.readGeneration(),
    snapshot: wasm.snapshot(session),
    stockCatalog: wasm.stock_catalog_view(session),
    companyCatalog: wasm.company_catalog_view(session),
    civilDate: wasm.civil_date(session),
  });
}

function postIngressBridge(): void {
  engineInitializer.requireConfigured();
  const generation = slot.readGeneration();
  const [session, wasm] = slot.requireHandle();
  const token = transportExtensions(wasm).ingress_token;
  if (token === undefined) throw new Error("当前 WASM bindings 缺少共享 ingress，请重建 bindings");
  ctx.postMessage({
    type: "ingress",
    generation,
    shared: true,
    token: token(session),
  });
}

function respondOperationError(message: WorkerMessage, failureMessage: string, failure?: CompanyWriteFailure | CompanyReadFailure): void {
  try {
    ctx.postMessage({ type: "operationError", requestId: message.requestId, generation: message.generation, message: failureMessage, ...(failure === undefined ? {} : { failure }) });
  } catch (sendError) {
    const where = `wasm-worker.${message.type}`;
    postFailureDetails(where, {
      code: "WASM_WORKER_PROTOCOL", where,
      message: `发送 operationError 失败：${describeWasmFailure(sendError)}；原始错误：${failureMessage}`,
      cause: failure === undefined ? { message: failureMessage } : { message: failureMessage, failure, sendCause: describeWasmFailure(sendError) },
    });
  }
}

function companyWriteProtocolFailure(error: unknown, decoderError: unknown): HostFailure {
  return { code: "WASM_WORKER_PROTOCOL", where: "wasm-worker.company-write",
    message: `Worker 公司写命令失败协议无效：${describeWasmFailure(decoderError)}`,
    cause: { failure: error, decoderError } };
}

function invokeCompanyWrite(message: WorkerMessage, requestedGeneration: number, successType: string, invoke: () => unknown): void {
  let result: unknown;
  try { result = invoke(); }
  catch (error) {
    let kind: unknown;
    try { kind = error !== null && typeof error === "object" ? Reflect.get(error, "kind") : undefined; }
    catch (decoderError) { throw companyWriteProtocolFailure(error, decoderError); }
    // 未识别错误及真正 Step 保原对象进入既有错误链，不以四分类推断 fatal。
    if (kind !== "PlayerProposal" && kind !== "ReportCorrection" && kind !== "RightsSubscription") throw error;
    let failure: CompanyWriteFailure;
    try {
      failure = parseCompanyWriteFailure(error);
      if (failure.kind !== (message.type === "proposePlayerCompanyAction" ? "PlayerProposal" : message.type === "subscribeRightsOffering" ? "RightsSubscription" : "ReportCorrection")) throw new Error("failure family 与当前公司写命令不符");
    } catch (decoderError) { throw companyWriteProtocolFailure(error, decoderError); }
    respondOperationError({ ...message, generation: requestedGeneration }, new CompanyWriteRejectedError(failure).message, failure);
    return;
  }
  ctx.postMessage({ type: successType, requestId: message.requestId, generation: requestedGeneration, result: normalizeSerdeMaps(result) });
}

type LoadedEngineModule = { readonly module: WebAssembly.Module; readonly diagnostics: boolean; readonly glue: FacadeEngineGlue };
let verifiedRestoreModule: LoadedEngineModule | undefined;
let initRequested = false;
let memoryInitializationStarted = false;
let engineQuiescentReported = false;

async function loadEngineModule(expected?: VerifiedWasmRestoreCapability): Promise<LoadedEngineModule> {
  if (expected === undefined && verifiedRestoreModule !== undefined) {
    const loaded = verifiedRestoreModule; verifiedRestoreModule = undefined; return loaded;
  }
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
  const wasmBytes = await response.arrayBuffer();
  if (expected !== undefined) {
    const digest = await crypto.subtle.digest("SHA-256", wasmBytes);
    const actual = [...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
    if (actual !== expected.wasmSha256) throw Object.assign(new Error("实际 WASM 字节与 verified restore package 身份不符，尚未分配 Memory"), { code: "WASM_PACKAGE_IDENTITY_MISMATCH", where: "wasm-worker.restore-package", context: { expected: expected.wasmSha256, actual } });
  }
  const module = await WebAssembly.compile(wasmBytes);
  if (expected !== undefined) {
    const wasmExports = new Set(WebAssembly.Module.exports(module).filter((entry) => entry.kind === "function").map((entry) => entry.name));
    const glue = wasmModule as unknown as Record<string, unknown>;
    for (const name of expected.exports) {
      if (!wasmExports.has(name) || typeof glue[name] !== "function") throw Object.assign(new Error(`实际 package 缺少 verified 导出 ${name}，尚未分配 Memory`), { code: "WASM_RESTORE_TOPOLOGY_NOT_READY", where: "wasm-worker.restore-package", context: { operation: "restore", missing: name } });
    }
  }
  return { module, diagnostics: diagnosticsWasm, glue: wasmModule as unknown as FacadeEngineGlue };
}

function handleBusinessMessage(event: MessageEvent<WorkerMessage>): void {
  const message = event.data;
  void (async () => {
    let restoreAttempt: { handle: number; generation: number; token: number; restartWanted: boolean } | null = null;
    try {
      if (closingSession && message.type !== "shutdown") throw new Error("Worker 会话正在关闭，拒绝新请求");
      if (fatalSession && message.type !== "drop" && message.type !== "shutdown") throw new Error("Worker 会话已停止，必须重建有效会话");
      switch (message.type) {
        case "init": {
          if (initRequested) throw new Error("Worker init 只允许首次请求，禁止预加载期间重复初始化");
          initRequested = true;
          const { restoreCapability, ...initializerMessage } = message;
          if (Object.hasOwn(message, "initialArchive")) {
            const candidate = parseVerifiedRestoreCapability(restoreCapability, message.runtimeOverride);
            // glue/source/manifest 身份由实际发布包 producer 核实；此处核对实际装载的 WASM 字节与两层导出。
            verifiedRestoreModule = await loadEngineModule(structuredClone(candidate));
            if (closingSession) throw new Error("package 验证完成时实例已经关闭");
          } else if (restoreCapability !== undefined) throw new Error("新局不得携带初始 restore capability");
          memoryInitializationStarted = true;
          ctx.postMessage({ type: "initializationProgress", phase: "engine" });
          if (engineControlPort === undefined) throw new Error("engine transport controlPort 尚未绑定");
          await engineInitializer.initialize({ ...initializerMessage, controlPort: engineControlPort });
          ctx.postMessage({ type: "ready" });
          return;
        }
        case "restoreInitial": {
          if (Object.keys(message).join(",") !== "type") throw new Error("restoreInitial 只能消费 initializer 保存的完整 X，禁止第二份 payload");
          const configured = engineInitializer.requireConfigured();
          if (configured.initialArchive === null) throw new Error("restoreInitial 需要唯一完整初始档");
          slot.restoreInitial();
          const [, wasm] = slot.requireHandle();
          const capabilities = readWasmCapabilities(wasm);
          postIngressBridge();
          ctx.postMessage({ type: "created", generation: slot.readGeneration(), capabilities: {
            ...capabilities, npcDecisionDiagnostics: capabilities.npcDecisionDiagnostics && optionalNpcDecisionTrace(wasm) !== undefined,
          } });
          postBaseline();
          return;
        }
        case "create": {
          const configured = engineInitializer.requireConfigured();
          if (configured.initialArchive !== null) throw new Error("初始完整 X 只能 restoreInitial，禁止先建空局");
          if (Object.hasOwn(message, "setup")) throw new Error("create 不接受第二份 setup；必须使用首次 init 保存的同份 setup");
          slot.create(configured.setup, message.seed);
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
    stockCatalog: wasm.stock_catalog_view(session),
    companyCatalog: wasm.company_catalog_view(session),
            civilDate: wasm.civil_date(session),
          });
          return;
        }
        case "restore": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          const parsed = parseSaveSlot(message.slot);
          const restartToken = restoreRestart.begin();
          restoreAttempt = { handle: slot.requireHandle()[0], generation: slot.readGeneration(), token: restartToken, restartWanted: false };
          const restoredSnapshot = slot.restore(parsed, {
            wasRunning: loop.isRunning(), stop: () => loop.stop(), restart: () => { if (restoreAttempt !== null) restoreAttempt.restartWanted = true; },
          });
          const [session, wasm] = slot.requireHandle();
          const civilDate = wasm.civil_date(session);
          const stockCatalog = wasm.stock_catalog_view(session);
          const companyCatalog = wasm.company_catalog_view(session);
          const restoredGeneration = slot.readGeneration();
          prepareBaselineUpdate(String(restoredGeneration), restoredSnapshot, stockCatalog, companyCatalog, { civilDate, revision: null });
          postIngressBridge();
          ctx.postMessage({ type: "restored", requestId: message.requestId, generation: requestedGeneration,
            nextGeneration: restoredGeneration, snapshot: restoredSnapshot, stockCatalog, companyCatalog, civilDate });
          if (restoreAttempt.restartWanted) restoreRestart.schedule(restartToken,
            () => slot.readGeneration() === restoredGeneration && slot.requireHandle()[0] === session,
            () => loop.start());
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
        case "subscribeRightsOffering": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          if (typeof message.requestId !== "number" || !Number.isSafeInteger(message.requestId) || message.requestId < 1) throw new Error("Worker 配股认购 requestId 必须是正安全整数");
          exact({ ...message }, ["type", "requestId", "generation", "event_id", "requested_shares"], "Worker 配股认购");
          const [session, wasm] = slot.requireHandle();
          const binding = transportExtensions(wasm).subscribe_rights_offering;
          if (typeof binding !== "function") throw new Error("当前 WASM bindings 不支持配股认购，请重建 bindings");
          invokeCompanyWrite(message, requestedGeneration, "rightsSubscriptionAcknowledged", () => binding(session, message.event_id, message.requested_shares));
          return;
        }
        case "proposePlayerCompanyAction":
        case "enqueueReportCorrection":
        case "cancelReportCorrection": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          if (typeof message.requestId !== "number" || !Number.isSafeInteger(message.requestId) || message.requestId < 1) throw new Error("Worker 公司写命令 requestId 必须是正安全整数");
          const field = message.type === "proposePlayerCompanyAction" ? "proposal" : message.type === "enqueueReportCorrection" ? "request" : "operation_id";
          exact({ ...message }, ["type", "requestId", "generation", field], "Worker 公司写命令");
          const [session, wasm] = slot.requireHandle();
          const extensions = transportExtensions(wasm);
          const binding = message.type === "proposePlayerCompanyAction" ? extensions.propose_player_company_action : message.type === "enqueueReportCorrection" ? extensions.enqueue_report_correction : extensions.cancel_report_correction;
          if (typeof binding !== "function") throw new Error("当前 WASM bindings 不支持此公司写命令，请重建 bindings");
          const successType = message.type === "proposePlayerCompanyAction" ? "playerCompanyActionAcknowledged" : message.type === "enqueueReportCorrection" ? "reportCorrectionEnqueueAcknowledged" : "reportCorrectionCancelAcknowledged";
          invokeCompanyWrite(message, requestedGeneration, successType, () => binding(session, message[field]));
          return;
        }
        case "companyReadQuery": {
          const requestedGeneration = slot.requireGeneration(message.generation);
          const [session, wasm] = slot.requireHandle();
          const read = transportExtensions(wasm).company_read_query;
          if (read === undefined) throw new Error("当前 WASM bindings 不支持公司只读查询，请重建 bindings");
          let result: unknown;
          try { result = read(session, message.query); }
          catch (error) {
            const protocolFailure = (decoderError: unknown): HostFailure => ({
              code: "WASM_WORKER_PROTOCOL", where: "wasm-worker.company-read",
              message: `公司只读拒绝详情解码失败：${describeWasmFailure(decoderError)}`,
              cause: { failure: error, decoderError },
            });
            let kind: unknown;
            try { kind = error !== null && typeof error === "object" ? Reflect.get(error, "kind") : undefined; }
            catch (decoderError) { throw protocolFailure(decoderError); }
            if (typeof kind !== "string" || !["Unhealthy", "UnknownOwner", "UnknownCompany", "Catalog", "Company", "CorporateState", "DividendTaxOutstanding"].includes(kind)) throw error;
            let failure: CompanyReadFailure;
            try { failure = parseCompanyReadFailure(error); }
            catch (decoderError) { throw protocolFailure(decoderError); }
            const rejected = new CompanyReadRejectedError(failure, error);
            respondOperationError({ ...message, generation: requestedGeneration }, rejected.message, failure);
            return;
          }
          ctx.postMessage({ type: "companyReadQuery", requestId: message.requestId, generation: requestedGeneration, result });
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
          if (typeof message.code !== "number" || !Number.isInteger(message.code) || message.code < 0 || message.code > 4294967295) throw new Error("股票历史查询 StockId 无效");
          const [session, wasm] = slot.requireHandle();
          const queryHistory = transportExtensions(wasm).query_stock_history;
          if (queryHistory === undefined) throw new Error("当前 WASM bindings 不支持股票历史查询，请重建 bindings");
          // 主线程已用本代实际目录将只读查询 code 译为数值 StockId。
          ctx.postMessage({ type: "stockHistory", requestId: message.requestId, generation: requestedGeneration, data: queryHistory(session, message.code) });
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
        case "shutdown": {
          if (Object.keys(message).sort().join(",") !== "generation,requestId,type" || !Number.isSafeInteger(message.requestId) || Number(message.requestId) < 1) throw new Error("Worker shutdown 需要唯一正 safe requestId 与 exact 字段");
          const requestedGeneration = slot.requireGeneration(message.generation);
          if (closingSession) throw new Error("Worker shutdown 不能重复消费会话");
          closingSession = true;
          restoreRestart.invalidate();
          loop.stop();
          const report = slot.shutdown();
          ctx.postMessage({ type: "shutdownCompleted", requestId: message.requestId, generation: requestedGeneration, report });
          reportEngineQuiescence();
          return;
        }
        case "drop": {
          restoreRestart.invalidate();
          loop.stop();
          slot.drop();
          return;
        }
        default:
          throw new Error(`未知 Worker 消息：${message.type}`);
      }
    } catch (error) {
      let failureError = error;
      let initializationClosed = false;
      if (message.type === "init" || message.type === "restoreInitial") {
        closingSession = true;
        restoreRestart.invalidate(); loop.stop();
        try {
          slot.drop(); engineInitializer.dispose(); verifiedRestoreModule = undefined;
          initializationClosed = true;
        } catch (cleanupError) {
          failureError = new AggregateError([error, cleanupError], "初始会话失败且清理失败，quiescence 未证明", { cause: { primary: error, cleanup: cleanupError } });
        }
      }
      const attempt = restoreAttempt;
      const committed = attempt !== null && (slot.readGeneration() !== attempt.generation || slot.requireHandle()[0] !== attempt.handle);
      const classified = classifyWasmFailure(failureError);
      if (attempt !== null && !committed && classified.kind === "unstructured" && attempt.restartWanted) {
        restoreRestart.schedule(attempt.token,
          () => !fatalSession && slot.readGeneration() === attempt.generation && slot.requireHandle()[0] === attempt.handle,
          () => loop.start());
      } else restoreRestart.invalidate();
      if (committed || classified.kind !== "unstructured") {
        const closeAuthority = committed || (classified.kind === "structured" && classified.code === "CATALOG_BASELINE_FAILED");
        if (closeAuthority) fatalSession = true;
        loop.stop();
        let failure: WorkerFailureDetails = classified.kind === "unstructured"
          ? { code: "WASM_RESTORE_COMMITTED_FAILED", where: "wasm-worker.restore",
              message: "恢复已替换实际会话，后续准备或清理失败，必须重建有效会话", context: { operation: "restore", generation: String(slot.readGeneration()) }, recoverable: false,
              recoveryActions: ["重新加载有效日终档或新建会话", "复制错误详情反馈"], cause: { message: classified.description } }
          : structuredHostFailure(failureError, `wasm-worker.${message.type}`, classified);
        // drop_session 同时撤销 SessionRegistry 的 ingress token，阻止独立 intake 继续入队。
        if (closeAuthority) {
          try { slot.drop(); }
          catch (cleanupError) { failure = { ...failure, cause: { primary: failure.cause, cleanup: describeWasmFailure(cleanupError) } }; }
        }
        postFailureDetails(`wasm-worker.${message.type}`, failure, initializationClosed ? message.type === "init" ? "initialization-failed" : "initial-session-failed" : false, initializationClosed && message.type === "init" && !memoryInitializationStarted);
        if (initializationClosed) reportEngineQuiescence(message.type === "init" && !memoryInitializationStarted ? "unallocated" : "quiescent");
      } else if (typeof message.requestId === "number") respondOperationError(message, classified.description);
      else {
        postFailureDetails("wasm-worker.message", unstructuredFailureDetails(classified.description, "wasm-worker.message", classified.cause), initializationClosed ? message.type === "init" ? "initialization-failed" : "initial-session-failed" : false, initializationClosed && message.type === "init" && !memoryInitializationStarted);
        if (initializationClosed) reportEngineQuiescence(message.type === "init" && !memoryInitializationStarted ? "unallocated" : "quiescent");
      }
    }
  })();
}

function reportEngineQuiescence(state: "quiescent" | "unallocated" = "quiescent"): void {
  if (engineQuiescentReported || ownerInstanceId === undefined) return;
  engineQuiescentReported = true;
  self.postMessage({ type: "engineLifecycle", instanceId: ownerInstanceId, state });
}

self.addEventListener("message", (event: MessageEvent<unknown>) => {
  if (initializedTransport) {
    postFailure("wasm-worker.global-message", new Error("engine 不接受 transport 绑定后的全局消息"));
    return;
  }
  try { bindEngineTransport(event.data); }
  catch (error) {
    const detail = error instanceof Error ? error : new Error("engine transport 失败", { cause: error });
    if (typeof self.reportError === "function") self.reportError(detail);
    else queueMicrotask(() => { throw detail; });
  }
});
