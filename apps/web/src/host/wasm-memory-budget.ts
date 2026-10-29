export const WASM_MEMORY_MAXIMUM_BYTES = 2 * 1024 * 1024 * 1024;
export const WASM_MEMORY_RECLAIM_BYTES = 512 * 1024 * 1024;

export interface WasmMemoryBudgetInput {
  maximumBytes: number;
  reclaimBytes: number;
  // 此次判定选择的可信 live 上界；生产与 Memory.maximum 共用强制容量事实，来源未知传 null。
  liveUpperBoundBytes: number | null;
  carryBase: number;
  sessionResident: number;
  // actual live 台账，不可用当前值代替尚未分配实例的峰值上界。
  liveBytes: number;
}

export interface WasmMemoryBudgetDecision {
  status: "NOT_READY" | "READY" | "RELOAD" | "EXHAUSTED" | "BUDGET_EXCEEDED";
  accountedBytes: number;
  thresholdEffective: number | null;
}

function bytes(value: number, field: string): number {
  if (!Number.isSafeInteger(value) || value < 0) throw new Error(`${field} 必须为非负 safe integer bytes`);
  return value;
}

// 保守账没有 GC、ACK 或 FinalizationRegistry 扣减入口。
export function addWasmMemoryBytes(...values: number[]): number {
  let total = 0;
  for (const value of values) {
    bytes(value, "bytes");
    if (value > Number.MAX_SAFE_INTEGER - total) throw new Error("bytes 相加溢出 safe integer");
    total += value;
  }
  return total;
}

function validateParameters(input: Pick<WasmMemoryBudgetInput, "maximumBytes" | "reclaimBytes" | "liveUpperBoundBytes">): void {
  bytes(input.maximumBytes, "maximumBytes");
  bytes(input.reclaimBytes, "reclaimBytes");
  if (input.maximumBytes === 0 || input.reclaimBytes === 0 || input.reclaimBytes > input.maximumBytes) {
    throw new Error("maximumBytes / reclaimBytes 必须为正且 reclaimBytes 不超过 maximumBytes");
  }
  if (input.liveUpperBoundBytes !== null) {
    bytes(input.liveUpperBoundBytes, "liveUpperBoundBytes");
    if (input.liveUpperBoundBytes === 0) throw new Error("liveUpperBoundBytes 必须为正的可信 live 上界");
    if (input.liveUpperBoundBytes > input.maximumBytes - input.reclaimBytes) {
      throw new Error("live 上界违反 reclaimBytes ≤ maximumBytes − liveUpperBoundBytes");
    }
  }
}

// admission / drop 使用 caller 已选的安全 live 上界；本叶不从 observed sample 或当前 byteLength 推测上界。
export function evaluateWasmMemoryBudget(input: WasmMemoryBudgetInput, phase: "admission" | "drop"): WasmMemoryBudgetDecision {
  validateParameters(input);
  bytes(input.carryBase, "carryBase");
  bytes(input.sessionResident, "sessionResident");
  bytes(input.liveBytes, "liveBytes");
  if (phase !== "admission" && phase !== "drop") throw new Error("phase 必须为 admission 或 drop");
  if (phase === "drop" && input.liveBytes !== 0) throw new Error("drop 检查点 liveBytes 必须为 0");
  const residual = addWasmMemoryBytes(input.carryBase, input.sessionResident);
  const accountedBytes = addWasmMemoryBytes(residual, input.liveBytes);
  const liveUpperBoundBytes = input.liveUpperBoundBytes;
  if (liveUpperBoundBytes === null) return { status: "NOT_READY", accountedBytes, thresholdEffective: null };
  const room = input.maximumBytes - liveUpperBoundBytes;
  const thresholdEffective = Math.min(input.reclaimBytes, room - input.carryBase);
  const result = (status: WasmMemoryBudgetDecision["status"]): WasmMemoryBudgetDecision => ({ status, accountedBytes, thresholdEffective });
  // 先检查实际累计账；准入用剩余量比较，避免加法越过 safe integer。
  if (accountedBytes > input.maximumBytes) return result("BUDGET_EXCEEDED");
  if (residual > room) return result("EXHAUSTED");
  if (phase === "admission") return result("READY");
  // R_eff=0 的顶点首个 drop 直接终局，不生成无效 reload。
  if (thresholdEffective <= 0) return result("EXHAUSTED");
  if (input.sessionResident >= thresholdEffective) return result("RELOAD");
  return result("READY");
}

// 这里只计算补足值；parameters.liveUpperBoundBytes 必须由 caller 显式传入同源强制构造上界。
// genesis 判别、跨键读取与通道优先序属于未来 storage 层；未知构造上界返回 null。
export function rebuildWasmResidualBytes(
  highWaterCarry: number,
  parameters: Pick<WasmMemoryBudgetInput, "maximumBytes" | "reclaimBytes" | "liveUpperBoundBytes">,
): number | null {
  validateParameters(parameters);
  bytes(highWaterCarry, "highWaterCarry");
  if (highWaterCarry > parameters.maximumBytes) throw new Error("highWaterCarry 超出 maximumBytes");
  if (parameters.liveUpperBoundBytes === null) return null;
  const live = parameters.liveUpperBoundBytes;
  const room = parameters.maximumBytes - live;
  const supplement = Math.min(parameters.reclaimBytes, Math.max(0, room - highWaterCarry));
  // 按剩余额度逐步封顶，不能先溢出再 Math.min。
  const afterSupplement = highWaterCarry + Math.min(supplement, parameters.maximumBytes - highWaterCarry);
  return afterSupplement + Math.min(live, parameters.maximumBytes - afterSupplement);
}
