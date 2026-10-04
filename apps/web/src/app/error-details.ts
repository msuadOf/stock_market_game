import type { HostFailure } from "../host/host-update.ts";
import { TRADING_PHASES } from "../host/protocol/wire-values.ts";

export type FeedbackClipboard = { writeText: (text: string) => Promise<void> };
export type FeedbackCopyStatus = {
  readonly kind: "copying" | "copied" | "failed";
  readonly message: string;
};

const PRIVATE_FIELDS = /password|passwd|secret|token|authorization|cookie|credential|session.?id|timeline.?id|email|phone|address|path|stack|save|raw|account|持仓|存档|密码|令牌|身份证/i;
const PUBLIC_CONTEXT_FIELDS = new Set([
  "code", "operation", "phase", "tick", "seq", "fromSeq", "toSeq", "seq_from", "seq_to",
  "generation", "timeline_generation", "day", "civilDate", "civil_date", "revision", "public_revision",
  "requestId", "request_id", "expected", "actual",
  "tickFrom", "tickTo", "seqFrom", "seqTo", "kind", "cursor", "baselineRequired",
]);
const CAUSE_FIELDS = new Set(["code", "where", "message", "name", "cause", "context"]);
const PUBLIC_PHASES = new Set<string>([...TRADING_PHASES, "IntradayTrading", "ClosedDay"]);

function publicContextString(value: string, field: string | undefined): string {
  if (field === "kind") return value === "TickBatch" || value === "CivilUpdate" ? value : "[已脱敏：非公开诊断值]";
  if (field === "operation") return value === "step" || value === "endCivilDay" ? value : "[已脱敏：非公开诊断值]";
  if ((field === "civilDate" || field === "civil_date") && /^\d{4}-\d{2}-\d{2}$/.test(value)) return value;
  if (field === "phase" && PUBLIC_PHASES.has(value)) return value;
  if (field === "code" && /^[A-Z][A-Z0-9_]{0,63}$/.test(value)) return value;
  if (field !== undefined && PUBLIC_CONTEXT_FIELDS.has(field) && /^\d{1,20}$/.test(value)) return value;
  return "[已脱敏：非公开诊断值]";
}

export function redactFeedbackText(value: string): string {
  return value
    .replace(/\b(?:https?|wss?):\/\/[^\s<>"']+/gi, "[已脱敏：链接]")
    .replace(/\bBearer\s+[^\s,;]+/gi, "Bearer [已脱敏]")
    .replace(/\b(password|passwd|token|secret|authorization|cookie|api[_-]?key)\s*[:=]\s*[^\s,;]+/gi, "$1=[已脱敏]")
    .replace(/[\w.+-]+@[\w.-]+\.[A-Za-z]{2,}/g, "[已脱敏：邮箱]")
    .replace(/(?:[A-Za-z]:\\|\/(?:home|Users|data\d*|tmp)\/)[^\s<>"']+/g, "[已脱敏：本地路径]");
}

function diagnosticDetails(
  value: unknown,
  mode: "cause" | "context",
  seen = new Set<object>(),
  depth = 0,
  field?: string,
): unknown {
  if (mode === "context" && field === "operation") return typeof value === "string" ? publicContextString(value, field) : "[已脱敏：非公开诊断值]";
  if (depth >= 12) return "[详情超过展示深度]";
  if (value === null || value === undefined) return value === null ? null : "[未提供]";
  if (typeof value !== "object") {
    if (mode === "context" && (field === undefined || !PUBLIC_CONTEXT_FIELDS.has(field))) return "[已脱敏：非公开诊断字段]";
    if (typeof value === "string") return mode === "context" ? publicContextString(value, field) : redactFeedbackText(value);
    if (typeof value === "number" && !Number.isFinite(value)) return `[非法数值：${String(value)}]`;
    if (typeof value === "boolean" || typeof value === "number") return value;
    return `[不支持的详情类型：${typeof value}]`;
  }
  if (seen.has(value)) return "[循环引用]";
  seen.add(value);
  try {
    if (Array.isArray(value)) return value.map((entry) => diagnosticDetails(entry, mode, seen, depth + 1, field));
    const result: Record<string, unknown> = {};
    const descriptors = Object.getOwnPropertyDescriptors(value);
    for (const [index, [key, descriptor]] of Object.entries(descriptors).entries()) {
      const publicField = mode === "context" ? PUBLIC_CONTEXT_FIELDS.has(key) : CAUSE_FIELDS.has(key);
      const safeKey = publicField ? key : `[已脱敏字段${index + 1}]`;
      if (PRIVATE_FIELDS.test(key) || (mode === "cause" && !CAUSE_FIELDS.has(key))) {
        Object.defineProperty(result, safeKey, { value: "[已脱敏]", enumerable: true });
      } else if (!Object.hasOwn(descriptor, "value")) {
        Object.defineProperty(result, safeKey, { value: "[详情读取失败：访问器字段未执行]", enumerable: true });
      } else {
        Object.defineProperty(result, safeKey, {
          value: diagnosticDetails(descriptor.value, key === "context" ? "context" : mode, seen, depth + 1, key),
          enumerable: true,
        });
      }
    }
    return result;
  } catch (error) {
    return `[详情读取失败：${error instanceof Error ? redactFeedbackText(error.message) : "非 Error 异常"}]`;
  } finally {
    seen.delete(value);
  }
}

function detailsText(value: unknown, mode: "cause" | "context"): string {
  return value === null || value === undefined ? "未提供" : JSON.stringify(diagnosticDetails(value, mode), null, 2);
}

export function buildErrorFeedback(error: string | HostFailure): string {
  const failure = typeof error === "string" ? null : error;
  const recovery = failure?.recoverable === true
    ? "宿主声明可恢复（不保证刷新可解决）"
    : failure?.recoverable === false ? "不可直接恢复" : "未知";
  const actions = failure?.recoveryActions;
  return [
    "股票游戏错误反馈",
    `错误码：${failure === null ? "未提供" : redactFeedbackText(failure.code)}`,
    `位置：${failure === null ? "未提供" : redactFeedbackText(failure.where)}`,
    `错误：${redactFeedbackText(typeof error === "string" ? error : error.message)}`,
    `原因链：${detailsText(failure?.cause, "cause")}`,
    `上下文：${detailsText(failure?.context, "context")}`,
    `恢复能力：${recovery}`,
    `宿主建议动作：${actions === null || actions === undefined || actions.length === 0 ? "未提供" : actions.map(redactFeedbackText).join("；")}`,
    "反馈方式：复制此详情并附上触发操作；公开诊断字段以外的 context 内容已脱敏，不包含完整存档或调用栈。",
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
    const reason = copyError instanceof Error ? redactFeedbackText(copyError.message) : detailsText(copyError, "cause");
    report({ kind: "failed", message: `复制失败 @ error-feedback.clipboard：${reason}。请手动复制下方错误详情反馈。` });
    return;
  }
  report({ kind: "copied", message: "错误反馈已复制，请附上触发操作后反馈。" });
}
