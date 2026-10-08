import type { PlayerProposalOutcome } from "../types/generated/PlayerProposalOutcome.ts";
import type { PlayerProposalKind } from "../types/generated/PlayerProposalKind.ts";
import { civilDate, decimal, exact, integer, money, oneOf, record, string } from "../save/schema/primitives.ts";

/**
 * 玩家公司行为提案（N2b，2026-10-08 用户决策「持仓即可、直接生效」）的
 * 宿主传输层：提案 wire 输入（与 engine `PlayerCompanyProposal` serde 形状
 * 同构的外部标签枚举）、提交前的最小参数域校验（只做客户端预检，权威校验
 * 在 engine approve_* 入口）与三类显式结果的严格 parser（受理／制度拒绝／
 * 无持仓拒绝），不用宽松 any 透传。
 */

const PROPOSAL_KINDS = ["CashDividend", "StockDistribution", "RightsOffering", "SecondaryOffering", "IssuerRepurchase", "ShareSplit"] as const;
const ERROR_CLASSES = ["InvalidInput", "BusinessCondition", "UnsupportedOperation", "SystemState"] as const;
const DISTRIBUTION_KINDS = ["BonusShares", "CapitalReserveConversion"] as const;
const REPURPOSE_PURPOSES = ["ReduceCapital", "EmployeeIncentive", "ConvertibleConversion", "ValueMaintenance"] as const;
const SPLIT_DIRECTIONS = ["Split", "Consolidate"] as const;

/** 玩家提案 wire 输入（serde 外部标签枚举形态；金额为规范十进制分字符串，
 * 股数/比例为规范 u64 十进制字符串，天数/枚举为原生值）。 */
export type PlayerProposalWire =
  | { readonly CashDividend: { readonly company: string; readonly gross_per_share: string } }
  | { readonly StockDistribution: { readonly company: string; readonly kind: (typeof DISTRIBUTION_KINDS)[number]; readonly shares_per_existing_share_micros: string } }
  | { readonly RightsOffering: { readonly company: string; readonly price_per_share: string; readonly shares_per_existing_share_micros: string; readonly payment_days: number } }
  | { readonly SecondaryOffering: { readonly company: string; readonly price_per_share: string; readonly shares: string } }
  | { readonly IssuerRepurchase: { readonly company: string; readonly price_cap_per_share: string; readonly total_budget: string; readonly max_shares: string; readonly window_trading_days: number; readonly purpose: (typeof REPURPOSE_PURPOSES)[number] } }
  | { readonly ShareSplit: { readonly company: string; readonly direction: (typeof SPLIT_DIRECTIONS)[number]; readonly ratio: string } };

type JsonRecord = Readonly<Record<string, unknown>>;

function requireCompany(company: string): string {
  const trimmed = company.trim();
  if (trimmed.length === 0 || trimmed.length > 64) {
    throw new Error("提案公司身份必须是非空且不超过 64 字符的字符串");
  }
  return trimmed;
}

/** 正数金额（分）预检：复用 `money` 的规范十进制字符串校验并要求严格为正。 */
function positiveMoney(value: string, label: string): string {
  const cents = money(value, label);
  if (BigInt(cents) <= 0n) throw new Error(`${label}必须是正数（分）`);
  return cents;
}

/** 正整数（规范十进制字符串）预检；`minimum` 为闭下界（如拆股比例 ≥ 2）。 */
function positiveDecimal(value: string, label: string, minimum = 1n): string {
  const digits = decimal(value, label);
  if (BigInt(digits) < minimum) throw new Error(`${label}必须 ≥ ${minimum}`);
  return digits;
}

function positiveDays(value: number, label: string): number {
  if (!Number.isSafeInteger(value) || value < 1 || value > 255) {
    throw new Error(`${label}必须是 1..=255 的整数个交易日`);
  }
  return value;
}

/** 现金分红提案（每股税前红利，元文本最多两位小数 → 规范分字符串）。 */
export function buildCashDividendProposal(company: string, grossPerShareYuan: string): PlayerProposalWire {
  const trimmedCompany = requireCompany(company);
  const text = grossPerShareYuan.trim();
  if (!/^\d+(\.\d{1,2})?$/.test(text)) throw new Error("每股税前红利（元）必须是正数且最多两位小数");
  const [whole, fraction = ""] = text.split(".");
  const cents = `${whole}${(fraction + "00").slice(0, 2)}`.replace(/^0+(?=\d)/, "");
  positiveMoney(cents, "每股税前红利");
  return { CashDividend: { company: trimmedCompany, gross_per_share: cents } };
}

/** 送转提案（种类 + 每股送转比例，百万分之一股/股）。 */
export function buildStockDistributionProposal(company: string, kind: (typeof DISTRIBUTION_KINDS)[number], ratioMicros: string): PlayerProposalWire {
  return {
    StockDistribution: {
      company: requireCompany(company),
      kind: oneOf(kind, "送转种类", DISTRIBUTION_KINDS),
      shares_per_existing_share_micros: positiveDecimal(ratioMicros.trim(), "每股送转比例（百万分之一股/股）"),
    },
  };
}

/** 配股提案（面向全体股东：发行价 + 每 1 股配售比例 + 缴款期交易日数）。 */
export function buildRightsOfferingProposal(company: string, pricePerShareYuan: string, ratioMicros: string, paymentDays: number): PlayerProposalWire {
  const trimmedCompany = requireCompany(company);
  const text = pricePerShareYuan.trim();
  if (!/^\d+(\.\d{1,2})?$/.test(text)) throw new Error("配股发行价（元）必须是正数且最多两位小数");
  const [whole, fraction = ""] = text.split(".");
  const cents = `${whole}${(fraction + "00").slice(0, 2)}`.replace(/^0+(?=\d)/, "");
  return {
    RightsOffering: {
      company: trimmedCompany,
      price_per_share: positiveMoney(cents, "配股发行价"),
      shares_per_existing_share_micros: positiveDecimal(ratioMicros.trim(), "配股比例（百万分之一股/股）"),
      payment_days: positiveDays(paymentDays, "缴款期交易日数"),
    },
  };
}

/** 增发提案（定向提案玩家本人承购：发行价 + 定向股数）。 */
export function buildSecondaryOfferingProposal(company: string, pricePerShareYuan: string, shares: string): PlayerProposalWire {
  const trimmedCompany = requireCompany(company);
  const text = pricePerShareYuan.trim();
  if (!/^\d+(\.\d{1,2})?$/.test(text)) throw new Error("增发发行价（元）必须是正数且最多两位小数");
  const [whole, fraction = ""] = text.split(".");
  const cents = `${whole}${(fraction + "00").slice(0, 2)}`.replace(/^0+(?=\d)/, "");
  return {
    SecondaryOffering: {
      company: trimmedCompany,
      price_per_share: positiveMoney(cents, "增发发行价"),
      shares: positiveDecimal(shares.trim(), "增发定向股数"),
    },
  };
}

/** 发行人回购提案（价格上限 + 获批额度 + 数量上限 + 窗口交易日数 + 用途）。 */
export function buildIssuerRepurchaseProposal(
  company: string,
  priceCapYuan: string,
  budgetYuan: string,
  maxShares: string,
  windowTradingDays: number,
  purpose: (typeof REPURPOSE_PURPOSES)[number],
): PlayerProposalWire {
  const trimmedCompany = requireCompany(company);
  const yuanToCents = (text: string, label: string): string => {
    const trimmed = text.trim();
    if (!/^\d+(\.\d{1,2})?$/.test(trimmed)) throw new Error(`${label}（元）必须是正数且最多两位小数`);
    const [whole, fraction = ""] = trimmed.split(".");
    return `${whole}${(fraction + "00").slice(0, 2)}`.replace(/^0+(?=\d)/, "");
  };
  return {
    IssuerRepurchase: {
      company: trimmedCompany,
      price_cap_per_share: positiveMoney(yuanToCents(priceCapYuan, "回购价格上限"), "回购价格上限"),
      total_budget: positiveMoney(yuanToCents(budgetYuan, "回购获批额度"), "回购获批额度"),
      max_shares: positiveDecimal(maxShares.trim(), "回购数量上限"),
      window_trading_days: positiveDays(windowTradingDays, "回购窗口交易日数"),
      purpose: oneOf(purpose, "回购用途", REPURPOSE_PURPOSES),
    },
  };
}

/** 拆股／缩股提案（方向 + 整数换算比例 ≥ 2）。 */
export function buildShareSplitProposal(company: string, direction: (typeof SPLIT_DIRECTIONS)[number], ratio: string): PlayerProposalWire {
  return {
    ShareSplit: {
      company: requireCompany(company),
      direction: oneOf(direction, "拆股／缩股方向", SPLIT_DIRECTIONS),
      ratio: positiveDecimal(ratio.trim(), "整数换算比例", 2n),
    },
  };
}

type RequireVariant = { readonly key: string; readonly body: JsonRecord };

function requireVariant(value: unknown, path: string, variants: readonly string[]): RequireVariant {
  const wrapper = record(value, path);
  const keys = Object.keys(wrapper);
  if (keys.length !== 1 || !variants.includes(keys[0])) {
    throw new Error(`${path} 必须是单键枚举（${variants.join(" | ")}）`);
  }
  return { key: keys[0], body: record(wrapper[keys[0]], `${path}.${keys[0]}`) };
}

/** 提案结果的 UI 视图：三类显式分流（受理／无持仓拒绝／制度拒绝）。 */
export type PlayerProposalResultView =
  | { readonly outcome: "accepted"; readonly kind: PlayerProposalKind; readonly identity: string; readonly approved_on: string; readonly announced_on: string }
  | { readonly outcome: "no_holding"; readonly company: string; readonly stock: string; readonly detail: string }
  | { readonly outcome: "institutional_rejection"; readonly kind: PlayerProposalKind; readonly detail: string; readonly class: (typeof ERROR_CLASSES)[number] };

/**
 * 玩家提案结果的严格 parser：只接受 engine `PlayerProposalOutcome` 的当前
 * 契约形状；三类结果显式分流，未知形状显式拒绝，不静默吞错。
 */
export function parsePlayerProposalOutcome(value: unknown, path = "player_proposal_outcome"): PlayerProposalResultView {
  const { key, body } = requireVariant(value, path, ["Accepted", "NoHolding", "InstitutionalRejection"]);
  if (key === "Accepted") {
    const receiptHolder = record(body.receipt, `${path}.Accepted.receipt`);
    exact(receiptHolder, ["kind", "identity", "approved_on", "announced_on"], `${path}.Accepted.receipt`);
    const identity = string(receiptHolder.identity, `${path}.Accepted.receipt.identity`);
    if (!identity.trim()) throw new Error(`${path}.Accepted.receipt.identity 方案身份不能为空`);
    return {
      outcome: "accepted",
      kind: oneOf(receiptHolder.kind, `${path}.Accepted.receipt.kind`, PROPOSAL_KINDS),
      identity,
      approved_on: civilDate(receiptHolder.approved_on, `${path}.Accepted.receipt.approved_on`),
      announced_on: civilDate(receiptHolder.announced_on, `${path}.Accepted.receipt.announced_on`),
    };
  }
  if (key === "NoHolding") {
    exact(body, ["company", "stock", "detail"], `${path}.NoHolding`);
    const company = string(body.company, `${path}.NoHolding.company`);
    const stock = string(body.stock, `${path}.NoHolding.stock`);
    const detail = string(body.detail, `${path}.NoHolding.detail`);
    if (!company.trim() || !stock.trim() || !detail.trim()) {
      throw new Error(`${path}.NoHolding 公司、证券与拒绝原因都必须非空`);
    }
    return { outcome: "no_holding", company, stock, detail };
  }
  exact(body, ["kind", "detail", "class"], `${path}.InstitutionalRejection`);
  const detail = string(body.detail, `${path}.InstitutionalRejection.detail`);
  if (!detail.trim()) throw new Error(`${path}.InstitutionalRejection.detail 拒绝原因必须非空`);
  return {
    outcome: "institutional_rejection",
    kind: oneOf(body.kind, `${path}.InstitutionalRejection.kind`, PROPOSAL_KINDS),
    detail,
    class: oneOf(body.class, `${path}.InstitutionalRejection.class`, ERROR_CLASSES),
  };
}

/** 兼容 re-export：engine 契约类型与视图共用（严格 parser 保证形状）。 */
export type { PlayerProposalOutcome, PlayerProposalKind };

import type { SimpleCompanyPreferences } from "../types/generated/SimpleCompanyPreferences.ts";

/**
 * 当前 simple 行为偏好的严格 parser（局内偏好编辑入口的初值读取）：
 * 与 engine `SimpleCompanyPreferences` serde 形状同构，两项均可为 null
 * （未配置 = 不自动提案）；域校验与 engine `validate` 同口径。
 */
export function parseSimplePreferencesValue(value: unknown, path = "simple_preferences"): SimpleCompanyPreferences {
  const view = record(value, path);
  exact(view, ["cash_dividend", "stock_distribution"], path);
  const parseCash = (nested: unknown, nestedPath: string) => {
    const cash = record(nested, nestedPath);
    exact(cash, ["target_payout_bp", "min_distributable_profit", "cycles_between_proposals"], nestedPath);
    const payout = integer(cash.target_payout_bp, `${nestedPath}.target_payout_bp`, 1);
    if (payout > 10000) throw new Error(`${nestedPath}.target_payout_bp 必须在 1..=10000`);
    const cycles = integer(cash.cycles_between_proposals, `${nestedPath}.cycles_between_proposals`, 1);
    if (cycles > 65535) throw new Error(`${nestedPath}.cycles_between_proposals 必须在 1..=65535`);
    const threshold = money(cash.min_distributable_profit, `${nestedPath}.min_distributable_profit`);
    if (BigInt(threshold) <= 0n) throw new Error(`${nestedPath}.min_distributable_profit 必须为正数（分）`);
    return { target_payout_bp: payout, min_distributable_profit: threshold, cycles_between_proposals: cycles };
  };
  const parseStock = (nested: unknown, nestedPath: string) => {
    const stock = record(nested, nestedPath);
    exact(stock, ["min_distributable_profit", "shares_per_existing_share_micros", "max_cumulative_expansion_micros", "cycles_between_proposals"], nestedPath);
    const threshold = money(stock.min_distributable_profit, `${nestedPath}.min_distributable_profit`);
    if (BigInt(threshold) <= 0n) throw new Error(`${nestedPath}.min_distributable_profit 必须为正数（分）`);
    const ratio = integer(stock.shares_per_existing_share_micros, `${nestedPath}.shares_per_existing_share_micros`, 1);
    if (ratio > 10_000_000) throw new Error(`${nestedPath}.shares_per_existing_share_micros 必须在 1..=10000000`);
    const expansion = integer(stock.max_cumulative_expansion_micros, `${nestedPath}.max_cumulative_expansion_micros`, 1);
    if (expansion > 1_000_000_000) throw new Error(`${nestedPath}.max_cumulative_expansion_micros 必须在 1..=1000000000`);
    const cycles = integer(stock.cycles_between_proposals, `${nestedPath}.cycles_between_proposals`, 1);
    if (cycles > 65535) throw new Error(`${nestedPath}.cycles_between_proposals 必须在 1..=65535`);
    return {
      min_distributable_profit: threshold,
      shares_per_existing_share_micros: ratio,
      max_cumulative_expansion_micros: expansion,
      cycles_between_proposals: cycles,
    };
  };
  return {
    cash_dividend: view.cash_dividend === null ? null : parseCash(view.cash_dividend, `${path}.cash_dividend`),
    stock_distribution: view.stock_distribution === null ? null : parseStock(view.stock_distribution, `${path}.stock_distribution`),
  };
}
