import { parseSaveSlot } from "../save/save-schema.ts";
import { normalizePublicReportById, normalizePublicReportPage, normalizeSerdeMaps } from "./serde-normalize.ts";
import { HostSpeedMeter, assertValidSpeedMultiplier } from "./speed.ts";
import { UI_TARGET_HZ } from "./host-update.ts";
import { restoreWasmSession } from "./wasm-restore-transaction.ts";
import { classifyWasmFailure, describeWasmFailure } from "./wasm-failure.ts";
import { inspectWasmUpdateDelivery } from "./wasm-update-delivery.ts";
import { resolveThreadCount } from "./thread-count.ts";
import { parseHostFailure } from "./protocol-failure.ts";
import { shouldLoadDiagnosticsWasm } from "./wasm-build-mode.ts";
import type { HostFailure } from "./host-update.ts";
import type { NpcDecisionTraceRecord } from "./npc-decision-trace.ts";

type WorkerMessage = Readonly<Record<string, unknown>> & { readonly type: string };
type WasmNpcDecisionTrace = (handle: number, account: bigint) => readonly NpcDecisionTraceRecord[];
type WasmTransportExtensions = typeof import("../../wasm-pkg/web_wasm.js") & {
  readonly prepare_public_baseline?: (handle: number) => void;
  readonly host_capabilities?: () => { readonly npcDecisionDiagnostics: boolean };
  readonly player_working_orders?: (handle: number) => unknown;
  readonly calculate_indicators?: (prices: number[], candles: readonly { high: number; low: number; close: number }[]) => unknown;
  readonly npc_decision_trace?: WasmNpcDecisionTrace;
  readonly save_candidate?: (handle: number, key: { readonly seq: number; readonly settledDate: string }) => unknown;
};
type WorkerPort = {
  postMessage(message: unknown): void;
  addEventListener(type: "message", listener: (event: MessageEvent<WorkerMessage>) => void): void;
};

const ctx: WorkerPort = self;
const TICK_MS = 1_000;
const FRAME_MS = 16;

export function isE2EStepMode(mode: unknown): boolean {
  return mode === "e2e";
}

const E2E_STEP_ENABLED = isE2EStepMode(import.meta.env?.MODE);

let wasmModule: typeof import("../../wasm-pkg/web_wasm.js") | null = null;
let handle: number | null = null;
let timer: ReturnType<typeof setTimeout> | null = null;
let running = false;
let speed = 1;
let generation = 0;
let flushMs = 1_000 / UI_TARGET_HZ;
let lastStepAt = 0;
let pausePreferences = { pause_after_close: false, pause_before_open: false };
const speedMeter = new HostSpeedMeter(() => performance.now());

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

function workerFailureDetails(error: unknown): WorkerFailureDetails {
  const structured = structuredHostFailure(error, "wasm-worker.message");
  if (structured !== null) return structured;
  return { code: "WASM_WORKER_PROTOCOL", where: "wasm-worker.message", message: describeWasmFailure(error) };
}

function requireRecord(value: unknown, label: string): Readonly<Record<string, unknown>> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${label} 必须是对象`);
  }
  return value as Readonly<Record<string, unknown>>;
}

function requestGeneration(message: WorkerMessage): number {
  if (!Number.isSafeInteger(message.generation) || message.generation !== generation) {
    throw new Error("Worker 请求属于已过期会话");
  }
  return generation;
}

function requireHandle(): [number, typeof import("../../wasm-pkg/web_wasm.js")] {
  if (handle === null || wasmModule === null) throw new Error("Worker 会话尚未就绪");
  return [handle, wasmModule];
}

function requirePreparePublicBaseline(wasm: typeof import("../../wasm-pkg/web_wasm.js")) {
  const prepare = (wasm as WasmTransportExtensions).prepare_public_baseline;
  if (prepare === undefined) throw new Error("当前 WASM bindings 缺少公开基线准备接口，请重建 bindings");
  return prepare;
}

function readWasmCapabilities(wasm: typeof import("../../wasm-pkg/web_wasm.js")) {
  const capabilities = (wasm as WasmTransportExtensions).host_capabilities;
  return capabilities?.() ?? { npcDecisionDiagnostics: false };
}

function optionalNpcDecisionTrace(wasm: typeof import("../../wasm-pkg/web_wasm.js")): WasmNpcDecisionTrace | undefined {
  return (wasm as WasmTransportExtensions).npc_decision_trace;
}

export function postFailure(where: string, error: unknown): void {
  const failure = workerFailureDetails(error);
  postFailureDetails(where, failure);
}

function postFailureDetails(where: string, failure: WorkerFailureDetails): void {
  ctx.postMessage({ type: "failure", generation, ...failure, where: failure.where || where });
}

function postBaseline(): void {
  const [session, wasm] = requireHandle();
  requirePreparePublicBaseline(wasm)(session);
  ctx.postMessage({
    type: "baseline",
    generation,
    snapshot: wasm.snapshot(session),
  });
}

function publish(rawUpdate: unknown): boolean {
  const delivery = inspectWasmUpdateDelivery(rawUpdate, pausePreferences);
  ctx.postMessage({ type: "protocol", generation, update: rawUpdate, civilDate: null, revision: null });
  if (delivery.pausesAtBarrier) {
    stopLoop();
    ctx.postMessage({ type: "barrierPaused", generation });
  }
  return delivery.recordsMarketTick;
}

function stepOnce(): boolean {
  try {
    const [session, wasm] = requireHandle();
    const rawUpdate = wasm.step(session);
    if (publish(rawUpdate)) speedMeter.recordTicks();
    return true;
  } catch (error) {
    stopLoop();
    postFailure("wasm-worker.step", error);
    return false;
  }
}

function frameLoop(): void {
  if (!running) return;
  const now = performance.now();
  if (speed === Infinity) {
    // A Worker message can only be handled between tasks. Yield after each
    // market tick so an arriving request is queued before the next tick.
    if (!stepOnce()) return;
  } else {
    const interval = TICK_MS / speed;
    if (lastStepAt + interval <= now) {
      lastStepAt += interval;
      if (!stepOnce()) return;
    }
  }
  if (!running) return;
  const untilNextTick = speed === Infinity ? 0 : lastStepAt + TICK_MS / speed - performance.now();
  const delay = Math.max(0, Math.min(FRAME_MS, flushMs, untilNextTick));
  timer = setTimeout(frameLoop, delay);
}

function startLoop(): void {
  if (running) return;
  running = true;
  speedMeter.setRunning(true);
  lastStepAt = performance.now();
  timer = setTimeout(frameLoop, FRAME_MS);
}

function stopLoop(): void {
  running = false;
  speedMeter.setRunning(false);
  if (timer !== null) clearTimeout(timer);
  timer = null;
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
  await wasmModule.default(new Uint8Array(await response.arrayBuffer()));
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
          if (wasmModule === null) throw new Error("wasm 未初始化");
          if (typeof message.seed !== "bigint") throw new Error("seed 必须是 bigint");
          handle = wasmModule.create_session(message.setup, message.seed);
          generation += 1;
          const capabilities = readWasmCapabilities(wasmModule);
          ctx.postMessage({ type: "created", generation, capabilities: {
            ...capabilities,
            npcDecisionDiagnostics: capabilities.npcDecisionDiagnostics && optionalNpcDecisionTrace(wasmModule) !== undefined,
          } });
          postBaseline();
          return;
        }
        case "start":
          startLoop();
          return;
        case "stop":
          stopLoop();
          return;
        case "stepOnce": {
          if (!E2E_STEP_ENABLED) throw new Error("Worker 受控单步只允许在 E2E 构建中调用");
          const requestedGeneration = requestGeneration(message);
          if (running) throw new Error("Worker 受控单步只允许在暂停状态执行");
          if (!stepOnce()) throw new Error("Worker 受控单步失败");
          const [session, wasm] = requireHandle();
          const committedTick = wasm.tick(session);
          if (committedTick > BigInt(Number.MAX_SAFE_INTEGER)) throw new Error("Worker 单步 tick 超出安全整数范围");
          ctx.postMessage({ type: "stepped", requestId: message.requestId, generation: requestedGeneration, tick: Number(committedTick) });
          return;
        }
        case "setSpeed":
          assertValidSpeedMultiplier(Number(message.speed));
          speed = Number(message.speed);
          speedMeter.setSpeed(speed);
          lastStepAt = performance.now();
          return;
        case "setFrameRate":
          if (!Number.isFinite(message.fps) || Number(message.fps) <= 0) throw new Error("帧率必须是正有限数");
          flushMs = 1_000 / Number(message.fps);
          return;
        case "setPausePreferences": {
          const requestedGeneration = requestGeneration(message);
          const preferences = requireRecord(message.preferences, "暂停偏好");
          if (typeof preferences.pause_after_close !== "boolean" || typeof preferences.pause_before_open !== "boolean") {
            throw new Error("暂停偏好必须包含布尔值");
          }
          pausePreferences = {
            pause_after_close: preferences.pause_after_close,
            pause_before_open: preferences.pause_before_open,
          };
          ctx.postMessage({ type: "pausePreferencesSet", requestId: message.requestId, generation: requestedGeneration });
          return;
        }
        case "speedMetrics": {
          const requestedGeneration = requestGeneration(message);
          ctx.postMessage({ type: "speedMetrics", requestId: message.requestId, generation: requestedGeneration, metrics: speedMeter.read() });
          return;
        }
        case "enqueue": {
          const requestedGeneration = requestGeneration(message);
          const [session, wasm] = requireHandle();
          wasm.enqueue(session, message.intent);
          ctx.postMessage({ type: "enqueued", requestId: message.requestId, generation: requestedGeneration });
          return;
        }
        case "save": {
          const requestedGeneration = requestGeneration(message);
          const [session, wasm] = requireHandle();
          const key = message.candidate;
          let saved: unknown;
          if (key === undefined) saved = wasm.save(session);
          else {
            const saveCandidate = (wasm as WasmTransportExtensions).save_candidate;
            if (saveCandidate === undefined) throw new Error("当前 WASM binding 不支持按日终候选保存，请重建 bindings");
            saved = saveCandidate(session, key as { readonly seq: number; readonly settledDate: string });
          }
          const slot = parseSaveSlot(normalizeSerdeMaps(saved));
          ctx.postMessage({ type: "saved", requestId: message.requestId, generation: requestedGeneration, slot });
          return;
        }
        case "refreshBaseline": {
          const requestedGeneration = requestGeneration(message);
          const [session, wasm] = requireHandle();
          requirePreparePublicBaseline(wasm)(session);
          ctx.postMessage({
            type: "refreshed",
            requestId: message.requestId,
            generation: requestedGeneration,
            snapshot: wasm.snapshot(session),
          });
          return;
        }
        case "restore": {
          const requestedGeneration = requestGeneration(message);
          const parsed = parseSaveSlot(message.slot);
          const [_, wasm] = requireHandle();
          const wasRunning = running;
          const restoredSnapshot = restoreWasmSession({
            currentHandle: () => handle,
            replaceHandle: (restoredHandle) => { handle = restoredHandle; },
            restore: () => wasm.restore_json(JSON.stringify(parsed)),
            snapshot: wasm.snapshot,
            drop: wasm.drop_session,
            wasRunning,
            stop: stopLoop,
            restart: () => { queueMicrotask(startLoop); },
          });
          generation += 1;
          if (handle === null) throw new Error("WASM restore 没有安装新会话句柄");
          requirePreparePublicBaseline(wasm)(handle);
          ctx.postMessage({
            type: "restored",
            requestId: message.requestId,
            generation: requestedGeneration,
            nextGeneration: generation,
            snapshot: restoredSnapshot,
          });
          ctx.postMessage({ type: "baseline", generation, snapshot: restoredSnapshot });
          return;
        }
        case "civilDate": {
          const requestedGeneration = requestGeneration(message);
          const [session, wasm] = requireHandle();
          ctx.postMessage({ type: "civilDate", requestId: message.requestId, generation: requestedGeneration, date: wasm.civil_date(session) });
          return;
        }
        case "endCivilDay": {
          const requestedGeneration = requestGeneration(message);
          const [session, wasm] = requireHandle();
          publish(wasm.end_civil_day(session));
          ctx.postMessage({ type: "civilDayEnded", requestId: message.requestId, generation: requestedGeneration });
          return;
        }
        case "publicReports": {
          const requestedGeneration = requestGeneration(message);
          const [session, wasm] = requireHandle();
          ctx.postMessage({ type: "publicReports", requestId: message.requestId, generation: requestedGeneration, page: normalizePublicReportPage(wasm.public_report_page(session, message.query) ) });
          return;
        }
        case "playerWorkingOrders": {
          const requestedGeneration = requestGeneration(message);
          const [session, wasm] = requireHandle();
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
        case "npcDecisionTrace": {
          const requestedGeneration = requestGeneration(message);
          if (!Number.isSafeInteger(message.account) || Number(message.account) < 0) throw new Error("NPC 账户 ID 必须是非负安全整数");
          const [session, wasm] = requireHandle();
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
          const requestedGeneration = requestGeneration(message);
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
          const [, wasm] = requireHandle();
          const calculateIndicators = (wasm as WasmTransportExtensions).calculate_indicators;
          if (calculateIndicators === undefined) throw new Error("当前 WASM bindings 不支持指标计算，请重建 bindings");
          const result = calculateIndicators(prices, candles);
          ctx.postMessage({ type: "indicatorsCalculated", requestId: message.requestId, generation: requestedGeneration, result });
          return;
        }
        case "publicReportById": {
          const requestedGeneration = requestGeneration(message);
          const [session, wasm] = requireHandle();
          ctx.postMessage({ type: "publicReportById", requestId: message.requestId, generation: requestedGeneration, report: normalizePublicReportById(wasm.public_report_by_id(session, String(message.id))) });
          return;
        }
        case "drop": {
          stopLoop();
          if (handle !== null && wasmModule !== null) wasmModule.drop_session(handle);
          handle = null;
          return;
        }
        default:
          throw new Error(`未知 Worker 消息：${message.type}`);
      }
    } catch (error) {
      const structuredFailure = structuredHostFailure(error, `wasm-worker.${message.type}`);
      if (structuredFailure !== null) {
        stopLoop();
        postFailureDetails(`wasm-worker.${message.type}`, structuredFailure);
      } else if (typeof message.requestId === "number") {
        respondOperationError(message, error);
      } else {
        postFailure("wasm-worker.message", error);
      }
    }
  })();
});
