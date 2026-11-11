export interface WasmRuntimeOptions {
  readonly runtimeOverride?: boolean;
}

export function parseWasmRuntimeOptions(search: string): WasmRuntimeOptions {
  const values = new URLSearchParams(search).getAll("wasmRuntime");
  if (values.length === 0) return { runtimeOverride: true };
  if (values.length !== 1) throw new Error("URL 参数 wasmRuntime 不得重复");
  if (values[0] === "0") throw new Error("URL 参数 wasmRuntime=0 已不支持；必须启用 runtime");
  if (values[0] === "1") return { runtimeOverride: true };
  throw new Error(`URL 参数 wasmRuntime 只接受 1，实际收到 ${JSON.stringify(values[0])}`);
}
