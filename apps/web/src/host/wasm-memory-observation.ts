export type WasmMemoryByteObservation = {
  readonly kind: "observed";
  readonly source: "WebAssembly.Memory.buffer.byteLength" | "performance.memory.totalJSHeapSize";
  readonly bytes: number;
};
export type JsHeapObservation = WasmMemoryByteObservation | {
  readonly kind: "unobservable";
  readonly capability: "js_face_unobservable";
  readonly reason: string;
};
export class WasmMemoryObservationError extends Error {
  readonly code = "MEMORY_OBSERVATION_FAILED";
  readonly where: string;
  readonly context: Readonly<Record<string, unknown>>;
  readonly recoveryActions = ["保留采集错误详情反馈", "检查当前浏览器与 WASM 构建配置"];
  constructor(where: string, message: string, context: Readonly<Record<string, unknown>>, cause?: unknown) {
    super(message, { cause }); this.where = where; this.context = context;
  }
}

const PAGE_BYTES = 64 * 1024;
// 只承诺保留导入时realm的原生getter；不认证导入前已被篡改的运行环境。
const memoryBufferGetter = typeof WebAssembly === "object" && typeof WebAssembly.Memory === "function"
  ? Object.getOwnPropertyDescriptor(WebAssembly.Memory.prototype, "buffer")?.get : undefined;
const sharedByteLengthGetter = typeof SharedArrayBuffer === "function"
  ? Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype, "byteLength")?.get : undefined;

function failed(field: string, message: string, cause?: unknown): WasmMemoryObservationError {
  return new WasmMemoryObservationError(`wasm-memory-observation.${field}`, message, { unit: "bytes" }, cause);
}
function bytes(value: unknown, field: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) throw failed(field, `${field} 必须为非负safe integer bytes`);
  return value;
}
function object(value: unknown, field: string): object {
  if (typeof value !== "object" || value === null) throw failed(field, `${field} 已存在但不是有效观测对象`);
  let array: boolean;
  try { array = Array.isArray(value); }
  catch (cause) { throw failed(field, `${field} 对象形状校验失败`, cause); }
  if (array) throw failed(field, `${field} 不能为数组`);
  return value;
}
function has(source: object, key: string, field: string): boolean {
  try { return Reflect.has(source, key); }
  catch (cause) { throw failed(`${field}.has`, `${field} API存在性检查失败`, cause); }
}
function get(source: object, key: string, field: string): unknown {
  try { return Reflect.get(source, key); }
  catch (cause) { throw failed(field, `${field} getter读取失败`, cause); }
}

/** 当前逻辑长度观测；maximumBytes为caller构造事实，不是从API读到的maximum或未来maxLive。 */
export function observeWasmMemory(memory: unknown, maximumBytes: number): WasmMemoryByteObservation {
  bytes(maximumBytes, "maximumBytes");
  if (maximumBytes === 0 || maximumBytes % PAGE_BYTES !== 0) throw failed("maximumBytes", "maximumBytes 必须为正的64KiB整页bytes");
  if (memoryBufferGetter === undefined) throw failed("memory", "当前环境缺少WebAssembly.Memory原生buffer getter");
  let buffer: unknown;
  try { buffer = memoryBufferGetter.call(memory); }
  catch (cause) { throw failed("memory", "输入必须为真实WebAssembly.Memory，无法取得原生buffer", cause); }
  if (sharedByteLengthGetter === undefined) throw failed("shared", "当前环境缺少SharedArrayBuffer原生byteLength getter");
  let length: unknown;
  try { length = sharedByteLengthGetter.call(buffer); }
  catch (cause) { throw failed("shared", "WASM memory必须具有真实SharedArrayBuffer", cause); }
  const current = bytes(length, "byteLength");
  if (current % PAGE_BYTES !== 0) throw failed("byteLength", "WASM当前逻辑长度不符合64KiB页单位");
  if (current > maximumBytes) throw failed("maximumBytes", "当前WASM逻辑长度超过caller声明的构造maximumBytes");
  return { kind: "observed", source: "WebAssembly.Memory.buffer.byteLength", bytes: current };
}

/** JS只采集totalJSHeapSize；能力缺失显式返回，已有坏数据或读取错误保持typed失败。 */
export function observeJsHeap(performanceObject: unknown): JsHeapObservation {
  const unavailable = (): JsHeapObservation => ({ kind: "unobservable", capability: "js_face_unobservable", reason: "当前环境未提供performance.memory，JS面不可观测" });
  if (performanceObject === null || performanceObject === undefined) return unavailable();
  const source = object(performanceObject, "performance");
  if (!has(source, "memory", "performance.memory")) return unavailable();
  const memory = object(get(source, "memory", "performance.memory"), "performance.memory");
  const field = "performance.memory.totalJSHeapSize";
  if (!has(memory, "totalJSHeapSize", field)) throw failed(field, "performance.memory已存在，但缺少totalJSHeapSize bytes字段");
  const current = bytes(get(memory, "totalJSHeapSize", field), field);
  return { kind: "observed", source: "performance.memory.totalJSHeapSize", bytes: current };
}
