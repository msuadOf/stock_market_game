/**
 * 显示与配色工具（全 UI 共用）。
 */
import type { Cents } from "../types/engine";
import type { IntentRejectedEvent } from "../types/engine";

const CHINESE_UNIT_STEP = 10_000;

function chineseUnit(tier: number): string {
  if (tier <= 0) return "";
  return `${tier % 2 === 1 ? "万" : ""}${"亿".repeat(Math.floor(tier / 2))}`;
}

export interface RecursiveChineseNumberOptions {
  minimum: number;
  maximum: number;
  baseFractionDigits: number;
  trimBaseTrailingZeros?: boolean;
}

/**
 * 以四位一组递归生成中文数量级：万、亿、万亿、亿亿……。
 * minimum / maximum 是该格式化器允许接收的真实数值范围，越界立即报错。
 */
export class RecursiveChineseNumberFormatter {
  readonly minimum: number;
  readonly maximum: number;
  readonly baseFractionDigits: number;
  readonly trimBaseTrailingZeros: boolean;

  constructor(options: RecursiveChineseNumberOptions) {
    const { minimum, maximum, baseFractionDigits, trimBaseTrailingZeros = false } = options;
    if (!Number.isFinite(minimum) || !Number.isFinite(maximum) || minimum > maximum) {
      throw new RangeError(`格式化范围无效：minimum=${String(minimum)}, maximum=${String(maximum)}`);
    }
    if (!Number.isSafeInteger(baseFractionDigits) || baseFractionDigits < 0 || baseFractionDigits > 20) {
      throw new RangeError(`小数位必须是 0 到 20 的安全整数，收到 ${String(baseFractionDigits)}`);
    }
    this.minimum = minimum;
    this.maximum = maximum;
    this.baseFractionDigits = baseFractionDigits;
    this.trimBaseTrailingZeros = trimBaseTrailingZeros;
  }

  format(value: number): string {
    if (!Number.isFinite(value) || value < this.minimum || value > this.maximum) {
      throw new RangeError(`待格式化数值超出范围 [${this.minimum}, ${this.maximum}]：${String(value)}`);
    }
    const sign = value < 0 ? "-" : "";
    const absolute = Math.abs(value);
    if (absolute < CHINESE_UNIT_STEP) {
      const fixed = absolute.toFixed(this.baseFractionDigits);
      const base = this.trimBaseTrailingZeros ? this.trimTrailingZeros(fixed) : fixed;
      return sign + base;
    }
    const scaled = this.scale(absolute, 0);
    return `${sign}${this.trimTrailingZeros(scaled.value.toFixed(2))}${this.unit(scaled.tier)}`;
  }

  private trimTrailingZeros(value: string): string {
    return value.replace(/(\.\d*?[1-9])0+$|\.0+$/, "$1");
  }

  private scale(value: number, tier: number): { value: number; tier: number } {
    if (value < CHINESE_UNIT_STEP) return { value, tier };
    return this.scale(value / CHINESE_UNIT_STEP, tier + 1);
  }

  private unit(tier: number): string {
    return chineseUnit(tier);
  }
}

const MONEY_FORMATTER = new RecursiveChineseNumberFormatter({
  minimum: -Number.MAX_VALUE,
  maximum: Number.MAX_VALUE,
  baseFractionDigits: 2,
  trimBaseTrailingZeros: true,
});

const LOT_FORMATTER = new RecursiveChineseNumberFormatter({
  minimum: 0,
  maximum: Number.MAX_VALUE,
  baseFractionDigits: 2,
  trimBaseTrailingZeros: true,
});

/** 金额以元显示，小于一万固定两位；大额递归使用中文数量级。 */
export function formatYuanAmount(yuanValue: number): string {
  return MONEY_FORMATTER.format(yuanValue);
}

/** 将跨 JSON 的非负十进制“分”无损格式化为元；全程不转为 Number。 */
export function formatDecimalCentsAsYuan(decimalCents: string): string {
  if (!/^\d+$/.test(decimalCents)) {
    throw new RangeError(`成交额必须是非负十进制整数分，收到 ${decimalCents}`);
  }
  const cents = BigInt(decimalCents);
  const unitStep = 10_000n;
  let tier = 0;
  let centsPerUnit = 100n;
  while (cents >= centsPerUnit * unitStep) {
    centsPerUnit *= unitStep;
    tier += 1;
  }
  let hundredths = (cents * 100n + centsPerUnit / 2n) / centsPerUnit;
  if (hundredths >= unitStep * 100n) {
    centsPerUnit *= unitStep;
    tier += 1;
    hundredths = (cents * 100n + centsPerUnit / 2n) / centsPerUnit;
  }
  const whole = hundredths / 100n;
  const fraction = (hundredths % 100n).toString().padStart(2, "0").replace(/0+$/, "");
  return `${whole}${fraction.length > 0 ? `.${fraction}` : ""}${chineseUnit(tier)}`;
}

/** 引擎股数换算成手后显示，非整手数量保留至两位。 */
export function formatSharesAsLots(shares: number, lotSize = 100): string {
  if (!Number.isSafeInteger(shares) || shares < 0) {
    throw new RangeError(`成交股数必须是非负安全整数，收到 ${String(shares)}`);
  }
  if (!Number.isSafeInteger(lotSize) || lotSize <= 0) {
    throw new RangeError(`每手股数必须是正安全整数，收到 ${String(lotSize)}`);
  }
  return formatLotAmount(shares / lotSize);
}

/** 已经以“手”为单位的行情量格式化。 */
export function formatLotAmount(lots: number): string {
  return LOT_FORMATTER.format(lots);
}

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
    case "PriceCageExceeded":
      return "委托价格超出连续竞价价格笼子";
    case "UnknownStock":
      return "未知股票";
    case "AuctionLimitOrderRequired":
      return "集合竞价仅接受限价委托";
    case "AuctionOrderNotCancelable":
      return "集合竞价委托当前不可撤销";
    case "AuctionOrderEntryClosed":
      return "09:25–09:30 不接受新委托";
    case "InvalidQuantity":
      return "委托数量不符合 A 股交易单位";
    case "ResourceLimitExceeded":
      return "当前未成交委托过多，请先撤单后再试";
    case "OrderNotFound":
      return "委托不存在或已成交";
    case "NotOrderOwner":
      return "不能撤销其他账户的委托";
  }
}
