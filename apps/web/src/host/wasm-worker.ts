import { parseEngineUpdate } from "./protocol/index.ts";
import { normalizePublicReportById, normalizePublicReportPage, normalizeSerdeMaps } from "./serde-normalize.ts";
import { HostSpeedMeter, assertValidSpeedMultiplier } from "./speed.ts";
import { UI_TARGET_HZ } from "./host-update.ts";

type WorkerMessage = Readonly<Record<string, unknown>> & { readonly type: string };
type WorkerPort = {
  postMessage(message: unknown): void;
  addEventListener(type: "message", listener: (event: MessageEvent<WorkerMessage>) => void): void;
};

const ctx: WorkerPort = self;
const TICK_MS = 1_000;
const FRAME_MS = 16;
const FASTEST_SLICE_MS = 14;
const MAX_STEPS = 100_000;

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

function stringifyError(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  if (error !== null && typeof error === "object") {
    try {
      return `非标准错误对象：${JSON.stringify(error)}`;
    } catch (serializationError) {
      const reason = serializationError instanceof Error ? serializationError.message : String(serializationError);
      return `非标准错误对象无法序列化：${reason}`;
    }
  }
  return String(error);
}

type WorkerFailureDetails = { readonly code: string; readonly message: string };

function structuredHostFailure(error: unknown): WorkerFailureDetails | null {
  if (error === null || typeof error !== "object" || Array.isArray(error)) return null;
  try {
    const candidate = error as Readonly<Record<string, unknown>>;
    if (typeof candidate.code === "string" && typeof candidate.message === "string") {
      return { code: candidate.code, message: candidate.message };
    }
  } catch (accessError) {
    return {
      code: "WASM_WORKER_PROTOCOL",
      message: `读取结构化错误失败：${stringifyError(accessError)}`,
    };
  }
  return null;
}

function workerFailureDetails(error: unknown): WorkerFailureDetails {
  const structured = structuredHostFailure(error);
  if (structured !== null) return structured;
  return { code: "WASM_WORKER_PROTOCOL", message: stringifyError(error) };
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

export function postFailure(where: string, error: unknown): void {
  const failure = workerFailureDetails(error);
  postFailureDetails(where, failure);
}

function postFailureDetails(where: string, failure: WorkerFailureDetails): void {
  ctx.postMessage({ type: "failure", generation, code: failure.code, where, message: failure.message });
}

function postBaseline(): void {
  const [session, wasm] = requireHandle();
  ctx.postMessage({
    type: "baseline",
    generation,
    snapshot: wasm.snapshot(session),
  });
}

function shouldPause(rawUpdate: unknown): boolean {
  const update = parseEngineUpdate(rawUpdate);
  if (!("CivilUpdate" in update)) return false;
  return (pausePreferences.pause_after_close && update.CivilUpdate.kinds.includes("AfterClose"))
    || (pausePreferences.pause_before_open && update.CivilUpdate.kinds.includes("BeforeOpen"));
}

function publish(rawUpdate: unknown): void {
  ctx.postMessage({ type: "protocol", generation, update: rawUpdate, civilDate: null, revision: null });
  if (shouldPause(rawUpdate)) {
    stopLoop();
    ctx.postMessage({ type: "barrierPaused", generation });
  }
}

function stepOnce(): boolean {
  try {
    const [session, wasm] = requireHandle();
    try {
      publish(wasm.step(session));
    } catch (error) {
      if (!String(error).includes("civil day barrier must be published before stepping")) throw error;
      publish(wasm.end_civil_day(session));
    }
    speedMeter.recordTicks();
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
    const until = now + FASTEST_SLICE_MS;
    let steps = 0;
    while (steps < MAX_STEPS && performance.now() < until && running) {
      if (!stepOnce()) return;
      steps += 1;
    }
  } else {
    const interval = TICK_MS / speed;
    let steps = 0;
    while (lastStepAt + interval <= now && steps < MAX_STEPS && running) {
      lastStepAt += interval;
      if (!stepOnce()) return;
      steps += 1;
    }
  }
  const elapsed = performance.now() - now;
  const delay = Math.max(1, Math.min(FRAME_MS, flushMs) - elapsed);
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
  ctx.postMessage({ type: "operationError", requestId: message.requestId, generation: message.generation, message: stringifyError(error) });
}

async function initialize(): Promise<void> {
  if (wasmModule !== null) return;
  wasmModule = await import("../../wasm-pkg/web_wasm.js");
  const response = await fetch(new URL("../../wasm-pkg/web_wasm_bg.wasm", import.meta.url));
  if (!response.ok) throw new Error(`加载 WASM 二进制失败：HTTP ${response.status}`);
  await wasmModule.default(new Uint8Array(await response.arrayBuffer()));
  const cores = Math.max(1, (navigator.hardwareConcurrency ?? 4) - 2);
  await wasmModule.initThreadPool(cores);
  ctx.postMessage({ type: "ready", cores });
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
          if (wasmModule === null) throw new Error("wasm 未初始化");
          if (typeof message.seed !== "bigint") throw new Error("seed 必须是 bigint");
          handle = wasmModule.create_session(message.setup, message.seed);
          generation += 1;
          ctx.postMessage({ type: "created", generation });
          postBaseline();
          return;
        }
        case "start":
          startLoop();
          return;
        case "stop":
          stopLoop();
          return;
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
          ctx.postMessage({ type: "saved", requestId: message.requestId, generation: requestedGeneration, slot: normalizeSerdeMaps(wasm.save(session)) });
          return;
        }
        case "restore": {
          const requestedGeneration = requestGeneration(message);
          const wasRunning = running;
          stopLoop();
          const [_, wasm] = requireHandle();
          const restored = wasm.restore_json(JSON.stringify(message.slot));
          const previous = handle;
          handle = restored;
          generation += 1;
          if (previous !== null) wasm.drop_session(previous);
          ctx.postMessage({
            type: "restored",
            requestId: message.requestId,
            generation: requestedGeneration,
            nextGeneration: generation,
            snapshot: wasm.snapshot(restored),
          });
          postBaseline();
          if (wasRunning) {
            queueMicrotask(startLoop);
          }
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
      const structuredFailure = structuredHostFailure(error);
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
