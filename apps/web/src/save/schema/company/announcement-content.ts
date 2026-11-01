import { SaveSchemaError, array, civilDate, decimal, exact, money, nullable, oneOf, record, string, type JsonRecord } from "../primitives.ts"
import { externalTag, i32, u32 } from "../personal/common.ts"
import { parseShockKind } from "./policies/shock.ts"

/** 与 Rust information/publication.rs 的 kind/value wire 一致；业务关系仍由 engine 恢复边界校验。 */
export type AnnouncementContent =
  | { readonly kind: "Shock"; readonly value: ReturnType<typeof parseAnnouncedEvent> }
  | { readonly kind: "CashDividend"; readonly value: { readonly plan: ReturnType<typeof parseCashDividendPlan>; readonly total_gross: string } }
  | { readonly kind: "RightsOffering"; readonly value: { readonly plan: ReturnType<typeof parseRightsOfferingPlan> } }
  | { readonly kind: "IssuerRepurchase"; readonly value: { readonly plan: ReturnType<typeof parseIssuerRepurchasePlan> } }
  | { readonly kind: "ShareSplit"; readonly value: ReturnType<typeof parseShareSplitPlan> }
  | { readonly kind: "StockDistribution"; readonly value: ReturnType<typeof parseStockDistributionPlan> }

type HolderId = "IssuerTreasury" | { readonly Account: string } | { readonly External: string }
type PlacementTarget =
  | { readonly NamedHolder: { readonly holder: HolderId; readonly shares: string; readonly lock_until: string | null } }
  | { readonly OpenPublicSubscription: { readonly shares: string } }
type OfferingMode =
  | { readonly RightsToAllShareholders: { readonly shares_per_existing_share_micros: string } }
  | { readonly DirectedPlacement: { readonly targets: readonly PlacementTarget[] } }

const eventIdentityKeys = ["event_id", "approval_reference", "issuer", "stock", "exchange"] as const
const approvalDateKeys = ["approved_on", "announced_on"] as const
const registrationDateKeys = [...approvalDateKeys, "registered_on", "ex_rights_on"] as const

function canonicalU64(value: unknown, path: string): string {
  const parsed = decimal(value, path)
  if (!/^(0|[1-9]\d*)$/.test(parsed)) throw new SaveSchemaError(path, "必须是规范 u64 非负十进制字符串")
  return parsed
}

function issuerIdentity(item: JsonRecord, path: string) {
  return {
    issuer: u32(item.issuer, `${path}.issuer`),
    stock: string(item.stock, `${path}.stock`),
    exchange: oneOf(item.exchange, `${path}.exchange`, ["sse", "szse"] as const),
  }
}

function eventIdentity(item: JsonRecord, path: string) {
  return { event_id: string(item.event_id, `${path}.event_id`), approval_reference: string(item.approval_reference, `${path}.approval_reference`), ...issuerIdentity(item, path) }
}

function approvalDates(item: JsonRecord, path: string) {
  return { approved_on: civilDate(item.approved_on, `${path}.approved_on`), announced_on: civilDate(item.announced_on, `${path}.announced_on`) }
}

function registrationDates(item: JsonRecord, path: string) {
  return { ...approvalDates(item, path), registered_on: civilDate(item.registered_on, `${path}.registered_on`), ex_rights_on: civilDate(item.ex_rights_on, `${path}.ex_rights_on`) }
}

function parseAnnouncedEvent(value: unknown, path: string) {
  const item = record(value, path)
  exact(item, ["kind", "amplitude_bp", "starts_on", "expires_on"], path)
  return { kind: parseShockKind(item.kind, `${path}.kind`), amplitude_bp: i32(item.amplitude_bp, `${path}.amplitude_bp`), starts_on: civilDate(item.starts_on, `${path}.starts_on`), expires_on: civilDate(item.expires_on, `${path}.expires_on`) }
}

function parseCashDividendPlan(value: unknown, path: string) {
  const item = record(value, path)
  exact(item, ["plan_id", "issuer", "stock", "exchange", "formula", ...approvalDateKeys, "registered_on", "ex_dividend_on", "payable_on", "gross_per_share", "distributable_amount"], path)
  return {
    plan_id: string(item.plan_id, `${path}.plan_id`), ...issuerIdentity(item, path),
    formula: oneOf(item.formula, `${path}.formula`, ["StandardCashOnly", "ExchangeApprovedAdjustment"] as const),
    ...approvalDates(item, path), registered_on: civilDate(item.registered_on, `${path}.registered_on`),
    ex_dividend_on: civilDate(item.ex_dividend_on, `${path}.ex_dividend_on`), payable_on: civilDate(item.payable_on, `${path}.payable_on`),
    gross_per_share: money(item.gross_per_share, `${path}.gross_per_share`), distributable_amount: money(item.distributable_amount, `${path}.distributable_amount`),
  }
}

function parseHolderId(value: unknown, path: string): HolderId {
  if (value === "IssuerTreasury") return value
  const [tag, body] = externalTag(value, path)
  if (tag === "Account") return { Account: canonicalU64(body, `${path}.Account`) }
  if (tag === "External") return { External: string(body, `${path}.External`) }
  throw new SaveSchemaError(path, "包含无效 HolderId")
}

function parsePlacementTarget(value: unknown, path: string): PlacementTarget {
  const [tag, body] = externalTag(value, path)
  const itemPath = `${path}.${tag}`
  const item = record(body, itemPath)
  if (tag === "NamedHolder") {
    // Option<CivilDate> 在当前存档契约中显式保留 nullable 字段，不补默认值。
    exact(item, ["holder", "shares", "lock_until"], itemPath)
    return { NamedHolder: { holder: parseHolderId(item.holder, `${itemPath}.holder`), shares: canonicalU64(item.shares, `${itemPath}.shares`), lock_until: nullable(item.lock_until, `${itemPath}.lock_until`, civilDate) } }
  }
  if (tag === "OpenPublicSubscription") {
    exact(item, ["shares"], itemPath)
    return { OpenPublicSubscription: { shares: canonicalU64(item.shares, `${itemPath}.shares`) } }
  }
  throw new SaveSchemaError(path, "包含无效定向增发对象")
}

function parseOfferingMode(value: unknown, path: string): OfferingMode {
  const [tag, body] = externalTag(value, path)
  const itemPath = `${path}.${tag}`
  const item = record(body, itemPath)
  if (tag === "RightsToAllShareholders") {
    exact(item, ["shares_per_existing_share_micros"], itemPath)
    return { RightsToAllShareholders: { shares_per_existing_share_micros: canonicalU64(item.shares_per_existing_share_micros, `${itemPath}.shares_per_existing_share_micros`) } }
  }
  if (tag === "DirectedPlacement") {
    exact(item, ["targets"], itemPath)
    return { DirectedPlacement: { targets: array(item.targets, `${itemPath}.targets`).map((target, index) => parsePlacementTarget(target, `${itemPath}.targets[${index}]`)) } }
  }
  throw new SaveSchemaError(path, "包含无效配股／增发模式")
}

function parseRightsOfferingPlan(value: unknown, path: string) {
  const item = record(value, path)
  exact(item, [...eventIdentityKeys, ...registrationDateKeys, "payment_start_on", "payment_deadline_on", "settlement_on", "listing_on", "price_per_share", "mode", "npc_subscription_strategy"], path)
  return {
    ...eventIdentity(item, path), ...registrationDates(item, path),
    payment_start_on: civilDate(item.payment_start_on, `${path}.payment_start_on`), payment_deadline_on: civilDate(item.payment_deadline_on, `${path}.payment_deadline_on`),
    settlement_on: civilDate(item.settlement_on, `${path}.settlement_on`), listing_on: civilDate(item.listing_on, `${path}.listing_on`),
    price_per_share: money(item.price_per_share, `${path}.price_per_share`), mode: parseOfferingMode(item.mode, `${path}.mode`),
    npc_subscription_strategy: oneOf(item.npc_subscription_strategy, `${path}.npc_subscription_strategy`, ["FullByDefault", "StrategyBased"] as const),
  }
}

function parseIssuerRepurchasePlan(value: unknown, path: string) {
  const item = record(value, path)
  exact(item, [...eventIdentityKeys, ...approvalDateKeys, "window_start_on", "window_deadline_on", "price_cap_per_share", "total_budget", "max_shares", "purpose", "completion_policy", "approval_unit_event_id"], path)
  return {
    ...eventIdentity(item, path), ...approvalDates(item, path), window_start_on: civilDate(item.window_start_on, `${path}.window_start_on`), window_deadline_on: civilDate(item.window_deadline_on, `${path}.window_deadline_on`),
    price_cap_per_share: money(item.price_cap_per_share, `${path}.price_cap_per_share`), total_budget: money(item.total_budget, `${path}.total_budget`), max_shares: canonicalU64(item.max_shares, `${path}.max_shares`),
    purpose: oneOf(item.purpose, `${path}.purpose`, ["ReduceCapital", "EmployeeIncentive", "ConvertibleConversion", "ValueMaintenance"] as const),
    completion_policy: oneOf(item.completion_policy, `${path}.completion_policy`, ["CancelOnCompletion"] as const),
    approval_unit_event_id: nullable(item.approval_unit_event_id, `${path}.approval_unit_event_id`, string),
  }
}

function parseShareSplitPlan(value: unknown, path: string) {
  const item = record(value, path)
  exact(item, [...eventIdentityKeys, ...registrationDateKeys, "direction", "ratio"], path)
  return { ...eventIdentity(item, path), ...registrationDates(item, path), direction: oneOf(item.direction, `${path}.direction`, ["Split", "Consolidate"] as const), ratio: canonicalU64(item.ratio, `${path}.ratio`) }
}

function parseStockDistributionPlan(value: unknown, path: string) {
  const item = record(value, path)
  exact(item, [...eventIdentityKeys, ...registrationDateKeys, "kind", "shares_per_existing_share_micros", "approved_total_new_shares"], path)
  return {
    ...eventIdentity(item, path), ...registrationDates(item, path), kind: oneOf(item.kind, `${path}.kind`, ["BonusShares", "CapitalReserveConversion"] as const),
    shares_per_existing_share_micros: canonicalU64(item.shares_per_existing_share_micros, `${path}.shares_per_existing_share_micros`), approved_total_new_shares: canonicalU64(item.approved_total_new_shares, `${path}.approved_total_new_shares`),
  }
}

export function parseAnnouncementContent(value: unknown, path: string): AnnouncementContent {
  const item = record(value, path)
  exact(item, ["kind", "value"], path)
  switch (item.kind) {
    case "Shock": return { kind: "Shock", value: parseAnnouncedEvent(item.value, `${path}.value`) }
    case "ShareSplit": return { kind: "ShareSplit", value: parseShareSplitPlan(item.value, `${path}.value`) }
    case "StockDistribution": return { kind: "StockDistribution", value: parseStockDistributionPlan(item.value, `${path}.value`) }
    case "CashDividend": {
      const body = record(item.value, `${path}.value`)
      exact(body, ["plan", "total_gross"], `${path}.value`)
      return { kind: "CashDividend", value: { plan: parseCashDividendPlan(body.plan, `${path}.value.plan`), total_gross: money(body.total_gross, `${path}.value.total_gross`) } }
    }
    case "RightsOffering": {
      const body = record(item.value, `${path}.value`)
      exact(body, ["plan"], `${path}.value`)
      return { kind: "RightsOffering", value: { plan: parseRightsOfferingPlan(body.plan, `${path}.value.plan`) } }
    }
    case "IssuerRepurchase": {
      const body = record(item.value, `${path}.value`)
      exact(body, ["plan"], `${path}.value`)
      return { kind: "IssuerRepurchase", value: { plan: parseIssuerRepurchasePlan(body.plan, `${path}.value.plan`) } }
    }
    default: throw new SaveSchemaError(`${path}.kind`, "包含无效公告内容种类")
  }
}
