/**
 * 显示与配色工具（全 UI 共用）。
 */
import type { Cents } from "../types/engine";
import type { IntentRejectedEvent } from "../types/engine";

/** 分 → 元（保留 2 位）。 */
export function yuan(cents: Cents): string {
  return (cents / 100).toFixed(2);
}

/** 元（带正负号，用于涨跌额 / 盈亏）。 */
export function yuanSigned(cents: Cents): string {
  return `${cents >= 0 ? "+" : ""}${yuan(cents)}`;
}

/** 百分比（带正负号，保留 2 位）。入参为小数（如 0.0235 表示 2.35%）。 */
export function pctSigned(ratio: number): string {
  return `${ratio >= 0 ? "+" : ""}${(ratio * 100).toFixed(2)}%`;
}

/** 涨跌方向 → 颜色 class 名（up/down/flat）。 */
export function colorClass(diff: number): string {
  if (diff > 0) return "up";
  if (diff < 0) return "down";
  return "flat";
}

/** 把引擎的拒单原因枚举翻成中文提示。 */
export function rejectionText(reason: IntentRejectedEvent["reason"]): string {
  switch (reason) {
    case "InsufficientCash":
      return "资金不足";
    case "InsufficientShares":
      return "持仓不足";
    case "LimitExceeded":
      return "超出涨跌停限制";
    case "UnknownStock":
      return "未知股票";
    default:
      return String(reason);
  }
}
