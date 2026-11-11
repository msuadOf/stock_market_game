import { wasmFailureStack } from "../host/wasm-failure.ts";
import type { HostFailure } from "../host/host-update.ts";

export type FeedbackClipboard = { writeText: (text: string) => Promise<void> };
export type FeedbackCopyStatus = {
  readonly kind: "copying" | "copied" | "failed";
  readonly message: string;
};

function diagnosticDetails(value: unknown, seen = new Set<object>()): unknown {
  if (value === undefined) return "[未提供]";
  if (typeof value === "bigint") return `${value}n`;
  if (typeof value === "number" && !Number.isFinite(value)) return `[非法数值：${String(value)}]`;
  if (typeof value === "symbol" || typeof value === "function") return String(value);
  if (value === null || typeof value !== "object") return value;
  if (seen.has(value)) return "[循环引用]";
  seen.add(value);
  try {
    if (Array.isArray(value)) return value.map((entry) => diagnosticDetails(entry, seen));
    if (value instanceof Map) return { type: "Map", entries: Array.from(Map.prototype.entries.call(value), ([key, entry]) => [diagnosticDetails(key, seen), diagnosticDetails(entry, seen)]) };
    if (value instanceof Set) return { type: "Set", values: Array.from(Set.prototype.values.call(value), (entry) => diagnosticDetails(entry, seen)) };
    if (value instanceof Date) return Number.isNaN(Date.prototype.getTime.call(value)) ? "Invalid Date" : Date.prototype.toISOString.call(value);
    const result: Record<string, unknown> = {};
    const descriptors = Object.getOwnPropertyDescriptors(value);
    const stack = wasmFailureStack(value);
    if (stack !== undefined) descriptors.stack = { value: stack, enumerable: true };
    // Error.name 通常在 prototype 上；message、stack、cause 和 AggregateError.errors 为非枚举自身字段。
    if (value instanceof Error && !Object.hasOwn(descriptors, "name")) {
      let prototype = Object.getPrototypeOf(value);
      while (prototype !== null) {
        const name = Object.getOwnPropertyDescriptor(prototype, "name");
        if (name !== undefined) { descriptors.name = name; break; }
        prototype = Object.getPrototypeOf(prototype);
      }
    }
    for (const [key, descriptor] of Object.entries(descriptors)) {
      Object.defineProperty(result, key, {
        value: Object.hasOwn(descriptor, "value")
          ? diagnosticDetails(descriptor.value, seen)
          : "[详情读取失败：访问器字段未执行]",
        enumerable: true,
      });
    }
    return result;
  } catch (error) {
    return `[详情读取失败：${error instanceof Error ? error.message : String(error)}]`;
  } finally {
    seen.delete(value);
  }
}

function detailsText(value: unknown): string {
  return value === null || value === undefined ? "未提供" : JSON.stringify(diagnosticDetails(value), null, 2);
}

export function buildErrorFeedback(error: string | HostFailure): string {
  const failure = typeof error === "string" ? null : error;
  const recovery = failure?.recoverable === true
    ? "宿主声明可恢复（不保证刷新可解决）"
    : failure?.recoverable === false ? "不可直接恢复" : "未知";
  const actions = failure?.recoveryActions;
  return [
    "股票游戏错误反馈",
    `错误码：${failure === null ? "未提供" : failure.code}`,
    `位置：${failure === null ? "未提供" : failure.where}`,
    `错误：${typeof error === "string" ? error : error.message}`,
    `原因链：${detailsText(failure?.cause)}`,
    `上下文：${detailsText(failure?.context)}`,
    `恢复能力：${recovery}`,
    `宿主建议动作：${actions === null || actions === undefined || actions.length === 0 ? "未提供" : actions.join("；")}`,
    `原始错误详情：${detailsText(error)}`,
    "反馈方式：复制此详情并附上触发操作；详情保留宿主提供的原始错误、上下文与调用栈。",
    "刷新前注意：日内进度未保存，仅自然日日结成功后更新存档。刷新不保证恢复当前日内进度。",
  ].join("\n");
}

export async function copyErrorFeedback(
  error: string | HostFailure,
  report: (status: FeedbackCopyStatus) => void,
  clipboard?: FeedbackClipboard | null,
): Promise<void> {
  report({ kind: "copying", message: "正在复制错误反馈…" });
  try {
    const target = clipboard === undefined ? (typeof navigator === "undefined" ? null : navigator.clipboard) : clipboard;
    if (target === null || target === undefined || typeof target.writeText !== "function") {
      throw new Error("浏览器 Clipboard 不可用（需安全上下文及剪贴板权限）");
    }
    await target.writeText(buildErrorFeedback(error));
  } catch (copyError) {
    const reason = copyError instanceof Error ? copyError.message : detailsText(copyError);
    report({ kind: "failed", message: `复制失败 @ error-feedback.clipboard：${reason}。请手动复制下方错误详情反馈。` });
    return;
  }
  report({ kind: "copied", message: "错误反馈已复制，请附上触发操作后反馈。" });
}
