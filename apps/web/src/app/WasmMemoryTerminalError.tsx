import { wasmFailureStack } from "../host/wasm-failure.ts";
import type { HostFailure } from "../host/host-update.ts";
import { buildErrorFeedback, copyErrorFeedback, type FeedbackClipboard } from "./error-details.ts";
import { FeedbackCopyButton } from "./HostStatusViews.tsx";

export type WasmMemoryTerminalFailure = HostFailure & {
  readonly code: "MEMORY_RESIDUAL_EXHAUSTED" | "MEMORY_HANDOVER_CORRUPT";
};
export interface WasmMemoryTerminalErrorProps {
  readonly error: WasmMemoryTerminalFailure;
  readonly clipboard?: FeedbackClipboard | null;
}

// oxlint-disable-next-line react/only-export-components -- App 分支与面板共用同一终局代码判定。
export function isWasmMemoryTerminalFailure(error: string | HostFailure | null | undefined): error is WasmMemoryTerminalFailure {
  return error !== null && error !== undefined && typeof error === "object"
    && (error.code === "MEMORY_RESIDUAL_EXHAUSTED" || error.code === "MEMORY_HANDOVER_CORRUPT");
}

export function WasmMemoryTerminalError({ error, clipboard }: WasmMemoryTerminalErrorProps) {
  if (!isWasmMemoryTerminalFailure(error)) {
    const received = error !== null && typeof error === "object" ? String((error as HostFailure).code) : String(error);
    throw new Error(`WasmMemoryTerminalError 收到非终局错误：${received}`);
  }
  // 终局权限高于宿主恢复建议；显式读取以保留 Error 的非枚举诊断字段。
  const feedback: HostFailure = {
    code: error.code, where: error.where, message: error.message,
    cause: error.cause, context: error.context,
    recoverable: false,
    recoveryActions: ["关闭当前标签页，再在全新标签页手动打开原网址；不要使用浏览器恢复或复制标签页", "复制错误反馈"],
  };
  // 复制描述符，保留非枚举诊断而不执行未知 getter；恢复建议只由终局权限覆盖。
  const descriptors: Record<string, PropertyDescriptor> = Object.getOwnPropertyDescriptors(error);
  const stack = wasmFailureStack(error);
  if (stack !== undefined) descriptors.stack = { value: stack, enumerable: true };
  for (const key of ["recoverable", "recoveryActions"]) delete descriptors[key];
  Object.defineProperties(feedback, descriptors);
  const exhausted = error.code === "MEMORY_RESIDUAL_EXHAUSTED";
  return <div className="app-error" role="alert" aria-live="assertive">
    <h2>{exhausted ? "内存保守残留预算已耗尽" : "内存残留账无法安全恢复"}</h2>
    <p>{exhausted
      ? "保守内存保险丝已阻止继续分配。这不代表真实泄漏，也不代表你的操作有误。"
      : "内存残留账或交接记录无法安全继承，当前标签页不能继续分配游戏实例。"}</p>
    <p>请关闭当前标签页，再在全新标签页手动打开原网址；不要使用浏览器恢复或复制标签页，否则可能继承同一份内存残留记录。仅自然日日结成功后更新存档，未保存的日内进度会丢失。</p>
    <FeedbackCopyButton onCopy={(report) => copyErrorFeedback(feedback, report, clipboard)} />
    <pre aria-label="错误反馈详情">{buildErrorFeedback(feedback)}</pre>
  </div>;
}
