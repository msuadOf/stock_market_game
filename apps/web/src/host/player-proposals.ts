import { parseMoney } from "../utils/money.ts";

type ProposalInput =
  | { CashDividend: { company: number; gross_per_share: string } }
  | { StockDistribution: { company: number; kind: "BonusShares" | "CapitalReserveConversion"; shares_per_existing_share_micros: string } }
  | { RightsOffering: { company: number; price_per_share: string; shares_per_existing_share_micros: string; payment_days: number } }
  | { SecondaryOffering: { company: number; price_per_share: string; shares: string } }
  | { IssuerRepurchase: { company: number; price_cap_per_share: string; total_budget: string; max_shares: string; window_trading_days: number; purpose: "ReduceCapital" | "EmployeeIncentive" | "ConvertibleConversion" | "ValueMaintenance" } }
  | { ShareSplit: { company: number; direction: "Split" | "Consolidate"; ratio: string } };

const U64_MAX = 18_446_744_073_709_551_615n;
const CANONICAL_U64 = /^(?:0|[1-9]\d*)$/;
const U32_MAX = 4_294_967_295;

type ProposalTag = "CashDividend" | "StockDistribution" | "RightsOffering" | "SecondaryOffering" | "IssuerRepurchase" | "ShareSplit";

const FIELDS = {
  CashDividend: ["company", "gross_per_share"],
  StockDistribution: ["company", "kind", "shares_per_existing_share_micros"],
  RightsOffering: ["company", "price_per_share", "shares_per_existing_share_micros", "payment_days"],
  SecondaryOffering: ["company", "price_per_share", "shares"],
  IssuerRepurchase: ["company", "price_cap_per_share", "total_budget", "max_shares", "window_trading_days", "purpose"],
  ShareSplit: ["company", "direction", "ratio"],
} satisfies Record<ProposalTag, readonly string[]>;

const MONEY_FIELDS = new Set(["gross_per_share", "price_per_share", "price_cap_per_share", "total_budget"]);
const U64_FIELDS = new Set(["shares_per_existing_share_micros", "shares", "max_shares", "ratio"]);
const U32_FIELDS = new Set(["company", "payment_days", "window_trading_days"]);
const ENUMS: Partial<Record<ProposalTag, Readonly<Record<string, readonly string[]>>>> = {
  StockDistribution: { kind: ["BonusShares", "CapitalReserveConversion"] },
  IssuerRepurchase: { purpose: ["ReduceCapital", "EmployeeIncentive", "ConvertibleConversion", "ValueMaintenance"] },
  ShareSplit: { direction: ["Split", "Consolidate"] },
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function fail(path: string, expected: string): never {
  throw new Error(`${path} ${expected}`);
}

function parseU32(value: unknown, path: string): number {
  if (typeof value !== "number" || !Number.isInteger(value) || Object.is(value, -0) || value < 0 || value > U32_MAX) {
    return fail(path, "必须是 0..4294967295 范围内的 u32 数值");
  }
  return value;
}

function parseU64(value: unknown, path: string): string {
  if (typeof value !== "string" || value.length > 20 || !CANONICAL_U64.test(value) || BigInt(value) > U64_MAX) {
    return fail(path, "必须是规范十进制 u64 字符串");
  }
  return value;
}

function parseField(tag: ProposalTag, field: string, value: unknown): unknown {
  const path = `Proposal.${tag}.${field}`;
  if (MONEY_FIELDS.has(field)) return parseMoney(value, path);
  if (U64_FIELDS.has(field)) return parseU64(value, path);
  if (U32_FIELDS.has(field)) return parseU32(value, path);
  const choices = ENUMS[tag]?.[field];
  if (choices !== undefined) {
    if (typeof value !== "string" || !choices.includes(value)) return fail(path, `必须是 ${choices.join(" | ")} 之一`);
  }
  return value;
}

export function parsePlayerCompanyProposal(value: unknown) {
  if (!isRecord(value)) return fail("Proposal", "必须是单键对象");
  const tags = Object.keys(value);
  if (tags.length !== 1 || tags[0] === undefined || !Object.hasOwn(FIELDS, tags[0])) {
    return fail("Proposal", "必须只包含一个已知提案类型");
  }

  const tag = tags[0] as ProposalTag;
  const payload = value[tag];
  if (!isRecord(payload)) return fail(`Proposal.${tag}`, "必须是字段对象");

  const allowedFields = FIELDS[tag];
  for (const field of Object.keys(payload)) {
    if (!allowedFields.includes(field)) fail(`Proposal.${tag}.${field}`, "不是该提案类型的字段");
  }
  for (const field of allowedFields) {
    if (!Object.hasOwn(payload, field)) fail(`Proposal.${tag}.${field}`, "字段缺失");
    parseField(tag, field, payload[field]);
  }

  return value as ProposalInput;
}

export type PlayerCompanyProposalInput = ReturnType<typeof parsePlayerCompanyProposal>;
