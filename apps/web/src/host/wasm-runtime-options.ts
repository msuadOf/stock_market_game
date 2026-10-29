export interface WasmRuntimeOptions {
  readonly runtimeOverride?: boolean;
  readonly waitSettings?: {
    readonly spinUs: number;
    readonly quietUs: number;
    readonly fineUs: number;
    readonly coarseUs: number;
  };
}

// 这些是明确启用 runtime 时的工程初值，单位为微秒；尚未经过性能标定。
const RUNTIME_WAIT_SETTINGS = {
  spinUs: 500,
  quietUs: 50_000,
  fineUs: 500,
  coarseUs: 100_000,
} as const;

export function parseWasmRuntimeOptions(search: string): WasmRuntimeOptions {
  const values = new URLSearchParams(search).getAll("wasmRuntime");
  if (values.length === 0) return {};
  if (values.length !== 1) throw new Error("URL 参数 wasmRuntime 不得重复");
  if (values[0] === "0") return { runtimeOverride: false };
  if (values[0] === "1") return { runtimeOverride: true, waitSettings: RUNTIME_WAIT_SETTINGS };
  throw new Error(`URL 参数 wasmRuntime 只接受 0 或 1，实际收到 ${JSON.stringify(values[0])}`);
}
