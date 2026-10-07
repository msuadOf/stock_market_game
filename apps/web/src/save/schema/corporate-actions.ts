import { accountId, array, boolean, civilDate, decimal, exact, integer, money, oneOf, record, SaveSchemaError, string } from "./primitives.ts"

export type HolderId = { readonly Account: string } | { readonly External: string } | "IssuerTreasury"
export type AcquisitionSource = { readonly InitialAllocation: { readonly evidence: string } } | { readonly SecondaryMarket: { readonly settlement: string } } | { readonly CorporateAction: { readonly event: string } }
export type ShareRestriction = "Unrestricted" | { readonly Restricted: { readonly reason: string; readonly release_on: string } }
export type ShareLot = { readonly id: string; readonly qty: string; readonly acquired_on: string; readonly source: AcquisitionSource; readonly restriction: ShareRestriction }
export type ShareHolding = { readonly holder: HolderId; readonly lots: readonly ShareLot[] }
export type IssuerRepurchaseAccountFacts = { readonly account_reference: string; readonly source_evidence: string; readonly established_on: string }
export type RegistrationSnapshot = { readonly event_id: string; readonly stock: string; readonly issuer: string; readonly registered_on: string; readonly issued_shares: string; readonly settled_receipts: string; readonly issuer_repurchase_account: IssuerRepurchaseAccountFacts | null; readonly holdings: readonly ShareHolding[] }
export type ShareRegistry = { readonly stock: string; readonly issuer: string; readonly issued_shares: string; readonly issuer_repurchase_account: IssuerRepurchaseAccountFacts | null; readonly settled_on: string; readonly holdings: readonly ShareHolding[]; readonly receipts: readonly ShareDayReceipt[]; readonly registrations: readonly RegistrationSnapshot[] }
export type ShareDayReceipt = { readonly request: ShareDayRequest; readonly disposals: readonly DisposedLot[] }
export type MovementScope = "PublicMarket" | { readonly NonTradingTransfer: { readonly basis: string } } | { readonly IssuerRepurchaseCancellation: { readonly basis: string } } | { readonly ShareReDenomination: { readonly basis: string } }
export type ShareDayRequest = { readonly event_id: string; readonly day: string; readonly scope: MovementScope; readonly changes: readonly DayNetChange[] }
export type DayNetChange = { readonly holder: HolderId; readonly change: string; readonly acquisition: NetAcquisition | null }
export type NetAcquisition = { readonly lot_id: string; readonly source: AcquisitionSource; readonly restriction: ShareRestriction }
export type DisposedLot = { readonly holder: HolderId; readonly lot: ShareLot; readonly disposed_on: string }
export type CashDividendFormula = "StandardCashOnly" | "ExchangeApprovedAdjustment"
/// 公司行为 plan 的交易所枚举是 engine `CalendarExchange`（serde lowercase），
/// wire 真值为 `sse`／`szse`；与 setup 股票的 `StockExchange`
/// （wire 为 `Shanghai`／`Shenzhen`）分属不同枚举，交叉校验须显式映射。
export type CalendarExchange = "sse" | "szse"
export type CashDividendPlan = { readonly plan_id: string; readonly issuer: string; readonly stock: string; readonly exchange: CalendarExchange; readonly formula: CashDividendFormula; readonly approved_on: string; readonly announced_on: string; readonly registered_on: string; readonly ex_dividend_on: string; readonly payable_on: string; readonly gross_per_share: string; readonly distributable_amount: string }
export type CashDividendEntitlement = { readonly holder: HolderId; readonly shares: string; readonly gross: string }
export type HolderPaymentOutcome = { readonly Paid: { readonly holder: HolderId; readonly amount: string } } | { readonly Failed: { readonly holder: HolderId; readonly reason: string } }
export type CashDividendPaymentReceipt = { readonly payment_id: string; readonly paid_on: string; readonly within_six_month_deadline: boolean; readonly outcomes: readonly HolderPaymentOutcome[] }
export type CashDividendStatus = "Approved" | "Announced" | "Registered" | "Payable" | "PartiallyPaid" | "Paid"
export type CashDividendBook = { readonly plan: CashDividendPlan; readonly status: CashDividendStatus; readonly registration: RegistrationSnapshot | null; readonly entitlements: readonly CashDividendEntitlement[]; readonly paid: readonly (readonly [HolderId, string])[]; readonly failures: readonly (readonly [HolderId, string])[]; readonly payments: readonly CashDividendPaymentReceipt[] }
export type DividendTaxStatus = "IndividualPublicMarket" | "TreatmentNotConfigured"
export type TaxAcquisitionSource = { readonly InitialAllocation: { readonly evidence: string } } | { readonly SecondaryMarket: { readonly settlement: string } } | { readonly CorporateAction: { readonly event: string } }
export type TaxShareClass = "PublicMarket" | { readonly StatutoryRestricted: { readonly release_on: string; readonly basis: "FinanceTax2009167" | "FinanceTax201070"; readonly qualification_evidence: string } }
export type DividendTaxLot = { readonly id: string; readonly qty: string; readonly acquired_on: string; readonly source: TaxAcquisitionSource; readonly class: TaxShareClass }
export type ExactDividendTaxAmount = { readonly numerator: string; readonly denominator: string }
export type TaxDayReceipt = { readonly operation_seq: number; readonly event_id: string; readonly day: string; readonly net_change: string; readonly acquisition: DividendTaxLot | null; readonly dispositions: readonly { readonly lot: DividendTaxLot; readonly disposed_on: string }[] }
export type DividendCashReceipt = { readonly operation_seq: number; readonly payment_id: string; readonly paid_on: string; readonly received_gross: string; readonly evidence: string }
export type RegisteredTaxDividend = { readonly operation_seq: number; readonly event_id: string; readonly registered_on: string; readonly per_share: ExactDividendTaxAmount; readonly lots: readonly DividendTaxLot[]; readonly payments: readonly DividendCashReceipt[] }
export type TaxCollectionReceipt = { readonly operation_seq: number; readonly event_id: string; readonly day: string; readonly available_cash: string; readonly collected: string; readonly remaining_cash: string; readonly outstanding: ExactDividendTaxAmount; readonly needs_funds: boolean }
export type TaxRedenominationReceipt = { readonly operation_seq: number; readonly event_id: string; readonly day: string; readonly net_change: string; readonly removed: readonly { readonly lot: DividendTaxLot; readonly disposed_on: string }[] }
export type DividendTaxProfile = "IndividualPublicMarket" | "ResidentEnterprise" | "SecuritiesFund" | "NonResident"
export type CashDividendTaxBook = { readonly operation_seq: number; readonly account: string; readonly stock: string; readonly profile: DividendTaxProfile; readonly opened_on: string; readonly opening_lots: readonly DividendTaxLot[]; readonly settled_on: string; readonly lots: readonly DividendTaxLot[]; readonly days: readonly TaxDayReceipt[]; readonly dividends: readonly RegisteredTaxDividend[]; readonly collections: readonly TaxCollectionReceipt[]; readonly redenominations: readonly TaxRedenominationReceipt[] }
export type AccountDividendGrossReceipt = { readonly payment_id: string; readonly plan_id: string; readonly account: string; readonly paid_on: string; readonly gross: string; readonly tax_status: DividendTaxStatus }
export type ExternalDividendReceipt = { readonly payment_id: string; readonly plan_id: string; readonly holder: { readonly External: string }; readonly paid_on: string; readonly gross: string; readonly tax_status: DividendTaxStatus }
export type DividendTaxOutstandingCause = "Cleared" | "InsufficientAvailableCash"
export type DividendTaxOutstandingView = { readonly account: string; readonly stock: string; readonly outstanding: ExactDividendTaxAmount; readonly needs_funds: boolean; readonly cause: DividendTaxOutstandingCause }

// 解析 engine `DividendTaxOutstandingView` 查询投影：与 Rust 侧 ts-rs 正规导出的
// `apps/web/src/types/generated/DividendTaxOutstandingView.ts` 共享同一序列化形态。
// 该视图是运行时查询结果，不进入日终存档；宿主/UI 接线由后续批次完成。
export function parseDividendTaxOutstandingView(value: unknown, path = "dividend_tax_outstanding"): DividendTaxOutstandingView {
  const view = record(value, path)
  exact(view, ["account", "stock", "outstanding", "needs_funds", "cause"], path)
  const account = accountId(view.account, `${path}.account`)
  const stock = string(view.stock, `${path}.stock`)
  if (stock.trim() === "") throw new SaveSchemaError(`${path}.stock`, "税股代码不能为空")
  const outstanding = parseExactAmount(view.outstanding, `${path}.outstanding`)
  const needs_funds = boolean(view.needs_funds, `${path}.needs_funds`)
  const cause = oneOf(view.cause, `${path}.cause`, ["Cleared", "InsufficientAvailableCash"] as const)
  const hasOutstanding = BigInt(outstanding.numerator) > 0n
  if (needs_funds !== hasOutstanding) throw new SaveSchemaError(`${path}.needs_funds`, "税账资金不足标志与未划收税额不一致")
  if (cause !== (hasOutstanding ? "InsufficientAvailableCash" : "Cleared")) throw new SaveSchemaError(`${path}.cause`, "税账未划收原因与余额不一致")
  return { account, stock, outstanding, needs_funds, cause }
}

export type StockDistributionKind = "BonusShares" | "CapitalReserveConversion"
export type SourceLotAttribution = "SourceLotAttributionPending"
export type HolderDistribution = { readonly holder: HolderId; readonly original_shares: string; readonly whole_shares: string; readonly fractional_numerator: string; readonly original_lots: readonly ShareLot[] }
export type StockDistributionReceipt = { readonly event_id: string; readonly approval_reference: string; readonly registration_event_id: string; readonly stock: string; readonly issuer: string; readonly registered_on: string; readonly issued_shares_before: string; readonly issuer_treasury_shares_excluded: string; readonly kind: StockDistributionKind; readonly shares_per_existing_share_micros: string; readonly approved_total_new_shares: string; readonly tie_break_seed: string; readonly source_lot_attribution: SourceLotAttribution; readonly holders: readonly HolderDistribution[] }
export type StockDistributionEventPlan = { readonly event_id: string; readonly approval_reference: string; readonly issuer: string; readonly stock: string; readonly exchange: CalendarExchange; readonly kind: StockDistributionKind; readonly approved_on: string; readonly announced_on: string; readonly registered_on: string; readonly ex_rights_on: string; readonly shares_per_existing_share_micros: string; readonly approved_total_new_shares: string }
export type StockDistributionStatus = "Approved" | "Announced" | "Registered" | "Credited"
export type StockDistributionBook = { readonly plan: StockDistributionEventPlan; readonly status: StockDistributionStatus; readonly registration: RegistrationSnapshot | null; readonly receipt: StockDistributionReceipt | null; readonly credited_on: string | null }
export type ShareSplitDirection = "Split" | "Consolidate"
export type HolderSplitOutcome = { readonly holder: HolderId; readonly original_shares: string; readonly new_shares: string; readonly fractional_numerator: string; readonly tie_break_award: string; readonly original_lots: readonly ShareLot[] }
export type ShareSplitReceipt = { readonly event_id: string; readonly approval_reference: string; readonly registration_event_id: string; readonly stock: string; readonly issuer: string; readonly registered_on: string; readonly direction: ShareSplitDirection; readonly ratio: string; readonly issued_shares_before: string; readonly issued_shares_after: string; readonly tie_break_seed: string; readonly holders: readonly HolderSplitOutcome[] }
export type ShareSplitEventPlan = { readonly event_id: string; readonly approval_reference: string; readonly issuer: string; readonly stock: string; readonly exchange: CalendarExchange; readonly direction: ShareSplitDirection; readonly ratio: string; readonly approved_on: string; readonly announced_on: string; readonly registered_on: string; readonly ex_rights_on: string }
export type ShareSplitStatus = "Approved" | "Announced" | "Registered" | "Settled"
export type ShareSplitBook = { readonly plan: ShareSplitEventPlan; readonly status: ShareSplitStatus; readonly registration: RegistrationSnapshot | null; readonly receipt: ShareSplitReceipt | null; readonly settled_on: string | null }
export type RightsSubscriptionStrategy = "FullByDefault" | "StrategyBased"
export type DirectedPlacementTarget = { readonly NamedHolder: { readonly holder: HolderId; readonly shares: string; readonly lock_until: string | null } } | { readonly OpenPublicSubscription: { readonly shares: string } }
export type RightsOfferingMode = { readonly RightsToAllShareholders: { readonly shares_per_existing_share_micros: string } } | { readonly DirectedPlacement: { readonly targets: readonly DirectedPlacementTarget[] } }
export type RightsOfferingEventPlan = { readonly event_id: string; readonly approval_reference: string; readonly issuer: string; readonly stock: string; readonly exchange: CalendarExchange; readonly approved_on: string; readonly announced_on: string; readonly registered_on: string; readonly payment_start_on: string; readonly payment_deadline_on: string; readonly ex_rights_on: string; readonly settlement_on: string; readonly listing_on: string; readonly price_per_share: string; readonly mode: RightsOfferingMode; readonly npc_subscription_strategy: RightsSubscriptionStrategy }
export type RightsEntitlement = { readonly holder: HolderId; readonly rights_shares: string; readonly lock_until: string | null }
export type RightsEntitlementReceipt = { readonly event_id: string; readonly approval_reference: string; readonly registration_event_id: string; readonly stock: string; readonly issuer: string; readonly registered_on: string; readonly issued_shares_before: string; readonly issuer_treasury_shares_excluded: string; readonly price_per_share: string; readonly mode: RightsOfferingMode; readonly planned_total_rights_shares: string; readonly tie_break_seed: string; readonly entitlements: readonly RightsEntitlement[]; readonly open_subscription_shares: string }
export type SubscriptionOrigin = "Explicit" | "NpcFullByDefault"
export type RightsSubscriptionRecord = { readonly holder: HolderId; readonly requested_shares: string; readonly price_per_share: string; readonly submitted_on: string; readonly origin: SubscriptionOrigin; readonly paid_shares: string; readonly paid_amount: string; readonly waived_shares: string }
export type HolderRightsSettlement = { readonly holder: HolderId; readonly paid_shares: string; readonly paid_amount: string; readonly waived_shares: string; readonly refunded_amount: string }
export type RightsSettlementReceipt = { readonly event_id: string; readonly settlement_on: string; readonly failed: boolean; readonly total_paid_shares: string; readonly total_paid_amount: string; readonly refunded_total: string; readonly holders: readonly HolderRightsSettlement[] }
export type RightsOfferingStatus = "Approved" | "Announced" | "Entitled" | "Closed" | "Settled"
export type RightsOfferingBook = { readonly plan: RightsOfferingEventPlan; readonly status: RightsOfferingStatus; readonly registration: RegistrationSnapshot | null; readonly entitlement: RightsEntitlementReceipt | null; readonly subscriptions: readonly RightsSubscriptionRecord[]; readonly closed_on: string | null; readonly settlement: RightsSettlementReceipt | null; readonly credited_on: string | null }
export type QueuedRightsSubscription = { readonly event_id: string; readonly account: string; readonly requested_shares: string; readonly submitted_on: string }
export type RejectedRightsSubscription = { readonly event_id: string; readonly account: string; readonly requested_shares: string; readonly submitted_on: string; readonly rejected_on: string; readonly reason: string }
export type RepurchasePurpose = "ReduceCapital" | "EmployeeIncentive" | "ConvertibleConversion" | "ValueMaintenance"
export type IssuerRepurchasePlan = { readonly event_id: string; readonly approval_reference: string; readonly issuer: string; readonly stock: string; readonly exchange: CalendarExchange; readonly approved_on: string; readonly announced_on: string; readonly window_start_on: string; readonly window_deadline_on: string; readonly price_cap_per_share: string; readonly total_budget: string; readonly max_shares: string; readonly purpose: RepurchasePurpose }
export type RepurchaseFillRecord = { readonly stock: string; readonly day: string; readonly shares: string; readonly gross: string; readonly fees: string }
export type IssuerRepurchaseStatus = "Approved" | "Announced" | "Executing" | "Completed" | "Cancelled"
export type IssuerRepurchaseBook = { readonly plan: IssuerRepurchasePlan; readonly status: IssuerRepurchaseStatus; readonly fills: readonly RepurchaseFillRecord[]; readonly completed_on: string | null; readonly withdrawn_remainder: string | null; readonly cancelled_on: string | null; readonly cancelled_shares: string; readonly last_order_day: string | null }
export type SessionCorporateActions = { readonly registries: readonly ShareRegistry[]; readonly dividends: readonly CashDividendBook[]; readonly dividend_tax_books: readonly CashDividendTaxBook[]; readonly stock_distributions: readonly StockDistributionBook[]; readonly share_splits: readonly ShareSplitBook[]; readonly rights_offerings: readonly RightsOfferingBook[]; readonly rights_subscription_queue: readonly QueuedRightsSubscription[]; readonly rejected_rights_subscriptions: readonly RejectedRightsSubscription[]; readonly issuer_repurchases: readonly IssuerRepurchaseBook[]; readonly account_gross_receipts: readonly AccountDividendGrossReceipt[]; readonly external_receipts: readonly ExternalDividendReceipt[]; readonly applied_ex_reference_groups: readonly AppliedExReferenceGroup[] }
export type AppliedExReferenceGroup = { readonly date: string; readonly stock: string; readonly cash_plan_ids: readonly string[]; readonly stock_event_ids: readonly string[]; readonly rights_event_ids: readonly string[]; readonly split_event_ids: readonly string[]; readonly reference: import("./company/ex-reference-price.ts").ExReferencePrice }

export type Context = {
  readonly issuers: Readonly<Record<string, { readonly listed_stock: string | null; readonly issued_shares: string }>>
  readonly setup: { readonly stocks: readonly { readonly code: string; readonly total_shares: string; readonly exchange: "Shanghai" | "Shenzhen"; readonly tick: string }[] }
  readonly snapshot: { readonly markets: Readonly<Record<string, { readonly last_cash_ex_reference: { readonly ex_date: string; readonly reference_price: string } | null }>>; readonly accounts: Readonly<Record<string, { readonly positions: Readonly<Record<string, { readonly qty: number }>> }>> }
  readonly currentDate: string
}

const U64_MAX = (1n << 64n) - 1n
const I64_MAX = (1n << 63n) - 1n
const I128_MIN = -(1n << 127n)
const I128_MAX = (1n << 127n) - 1n

/// setup 股票的 `StockExchange`（wire：Shanghai／Shenzhen）到公司行为 plan 的
/// `CalendarExchange`（wire：sse／szse）映射。两者是 engine 侧两个不同枚举，
/// 由会话自然日时钟接线建立对应；交叉校验不得直接字符串比较。
function calendarExchangeOfSetup(exchange: "Shanghai" | "Shenzhen"): CalendarExchange {
  return exchange === "Shanghai" ? "sse" : "szse"
}

function positiveU64(value: unknown, path: string): string {
  const parsed = decimal(value, path)
  if (parsed === "0") throw new SaveSchemaError(path, "必须为正 u64")
  return parsed
}

function signedI128(value: unknown, path: string): string {
  const parsed = string(value, path)
  if (!/^(0|-?[1-9]\d*)$/.test(parsed)) throw new SaveSchemaError(path, "必须为规范有符号 i128 十进制字符串")
  const integer = BigInt(parsed)
  if (integer < I128_MIN || integer > I128_MAX) throw new SaveSchemaError(path, "超出 i128 范围")
  return parsed
}

function signedI128Fraction(value: unknown, path: string): string {
  const parsed = string(value, path)
  if (!/^(0|-?[1-9]\d*)$/.test(parsed)) throw new SaveSchemaError(path, "必须为规范有符号 i128 十进制字符串")
  return parsed
}

type ParsedHolder = { readonly kind: "Account" | "External"; readonly id: string } | { readonly kind: "IssuerTreasury" }

function parseHolder(value: unknown, path: string): ParsedHolder {
  if (value === "IssuerTreasury") return { kind: "IssuerTreasury" }
  const tagged = record(value, path)
  if (Object.keys(tagged).length !== 1) throw new SaveSchemaError(path, "必须是单一 HolderId 枚举变体")
  const variant = Object.keys(tagged)[0]!
  if (variant === "Account") return { kind: variant, id: accountId(tagged[variant], `${path}.Account`) }
  if (variant === "External") {
    const id = string(tagged[variant], `${path}.External`)
    if (id.trim() === "") throw new SaveSchemaError(`${path}.External`, "外部股东身份不能为空")
    return { kind: variant, id }
  }
  throw new SaveSchemaError(path, "包含无效 HolderId 枚举变体")
}

function parseSource(value: unknown, path: string): AcquisitionSource {
  const tagged = record(value, path)
  if (Object.keys(tagged).length !== 1) throw new SaveSchemaError(path, "必须是单一 AcquisitionSource 枚举变体")
  const variant = Object.keys(tagged)[0]!
  if (!["InitialAllocation", "SecondaryMarket", "CorporateAction"].includes(variant)) throw new SaveSchemaError(path, "包含无效 AcquisitionSource 枚举变体")
  const field = { InitialAllocation: "evidence", SecondaryMarket: "settlement", CorporateAction: "event" }[variant]!
  const payload = record(tagged[variant], `${path}.${variant}`)
  exact(payload, [field], `${path}.${variant}`)
  const identity = string(payload[field], `${path}.${variant}.${field}`)
  if (identity.trim() === "") throw new SaveSchemaError(`${path}.${variant}.${field}`, "来源身份不能为空")
  if (variant === "InitialAllocation") return { InitialAllocation: { evidence: identity } }
  if (variant === "SecondaryMarket") return { SecondaryMarket: { settlement: identity } }
  return { CorporateAction: { event: identity } }
}

function parseRestriction(value: unknown, path: string): ShareRestriction {
  if (value === "Unrestricted") return value
  const tagged = record(value, path)
  exact(tagged, ["Restricted"], path)
  const restricted = record(tagged.Restricted, `${path}.Restricted`)
  exact(restricted, ["reason", "release_on"], `${path}.Restricted`)
  const reason = string(restricted.reason, `${path}.Restricted.reason`)
  if (reason.trim() === "") throw new SaveSchemaError(`${path}.Restricted.reason`, "限制原因不能为空")
  const release_on = civilDate(restricted.release_on, `${path}.Restricted.release_on`)
  return { Restricted: { reason, release_on } }
}

function parseLot(value: unknown, path: string): ShareLot {
  const lot = record(value, path)
  exact(lot, ["id", "qty", "acquired_on", "source", "restriction"], path)
  const id = string(lot.id, `${path}.id`)
  if (id.trim() === "") throw new SaveSchemaError(`${path}.id`, "股份批次身份不能为空")
  const acquired_on = civilDate(lot.acquired_on, `${path}.acquired_on`)
  const restriction = parseRestriction(lot.restriction, `${path}.restriction`)
  if (typeof restriction === "object" && restriction.Restricted.release_on < acquired_on) throw new SaveSchemaError(`${path}.restriction.Restricted.release_on`, "限制释放日期早于取得日期")
  return { id, qty: positiveU64(lot.qty, `${path}.qty`), acquired_on, source: parseSource(lot.source, `${path}.source`), restriction }
}

function parseHolding(value: unknown, path: string): { readonly holder: ParsedHolder; readonly lots: readonly ShareLot[]; readonly shares: string } {
  const holding = record(value, path)
  exact(holding, ["holder", "lots"], path)
  const holder = parseHolder(holding.holder, `${path}.holder`)
  const lots = array(holding.lots, `${path}.lots`).map((lot, index) => parseLot(lot, `${path}.lots[${index}]`))
  const ids = new Set<string>()
  let prior: string | undefined
  let shares = 0n
  for (const [index, lot] of lots.entries()) {
    if (ids.has(lot.id)) throw new SaveSchemaError(`${path}.lots[${index}].id`, "同一持有人批次身份重复")
    ids.add(lot.id)
    if (prior !== undefined && prior > lot.acquired_on) throw new SaveSchemaError(`${path}.lots`, "股份批次未按取得日期 FIFO 排序")
    prior = lot.acquired_on
    shares += BigInt(lot.qty)
    if (shares > U64_MAX) throw new SaveSchemaError(`${path}.lots`, "股份数量合计超出 u64")
  }
  return { holder, lots, shares: shares.toString() }
}

function parseIssuerRepurchaseAccount(value: unknown, path: string, asOf: string): IssuerRepurchaseAccountFacts | null {
  if (value === null) return null
  const facts = record(value, path)
  exact(facts, ["account_reference", "source_evidence", "established_on"], path)
  const account_reference = string(facts.account_reference, `${path}.account_reference`)
  const source_evidence = string(facts.source_evidence, `${path}.source_evidence`)
  const established_on = civilDate(facts.established_on, `${path}.established_on`)
  const year = Number(established_on.slice(0, 4))
  if (year < 1900 || year > 2199) throw new SaveSchemaError(`${path}.established_on`, "超出 Engine CivilDate 算法验证范围 1900–2199")
  if (!account_reference.trim() || !source_evidence.trim() || established_on > asOf) throw new SaveSchemaError(path, "回购专户身份、来源证据不能为空且设立日不得晚于适用日期")
  return { account_reference, source_evidence, established_on }
}

function parseSnapshot(value: unknown, path: string): RegistrationSnapshot {
  const snapshot = record(value, path)
  exact(snapshot, ["event_id", "stock", "issuer", "registered_on", "issued_shares", "settled_receipts", "issuer_repurchase_account", "holdings"], path)
  const event_id = string(snapshot.event_id, `${path}.event_id`)
  const stock = string(snapshot.stock, `${path}.stock`)
  const issuer = string(snapshot.issuer, `${path}.issuer`)
  if (!event_id.trim() || !issuer.trim() || !stock.trim()) throw new SaveSchemaError(path, "登记快照身份不能为空")
  const issued_shares = positiveU64(snapshot.issued_shares, `${path}.issued_shares`)
  const settled_receipts = decimal(snapshot.settled_receipts, `${path}.settled_receipts`)
  if (!/^\d+$/.test(settled_receipts)) throw new SaveSchemaError(`${path}.settled_receipts`, "登记时已落账回执数量必须为规范 u64 十进制字符串")
  const registered_on = civilDate(snapshot.registered_on, `${path}.registered_on`)
  const issuer_repurchase_account = parseIssuerRepurchaseAccount(snapshot.issuer_repurchase_account, `${path}.issuer_repurchase_account`, registered_on)
  const holdings = parseHoldings(snapshot.holdings, `${path}.holdings`)
  if (holdings.total !== issued_shares) throw new SaveSchemaError(`${path}.holdings`, "登记快照股份总量与发行股数不守恒")
  for (const [index, holding] of holdings.items.entries()) for (const lot of holding.lots) if (lot.acquired_on > registered_on) throw new SaveSchemaError(`${path}.holdings[${index}].lots`, "批次取得日期晚于登记日期")
  return { event_id, stock, issuer, registered_on, issued_shares, settled_receipts, issuer_repurchase_account, holdings: holdings.items }
}

function parseHoldings(value: unknown, path: string): { readonly items: readonly ShareHolding[]; readonly total: string; readonly byHolder: ReadonlyMap<string, string> } {
  const items = array(value, path).map((item, index) => parseHolding(item, `${path}[${index}]`))
  const holders = new Set<string>()
  const lots = new Set<string>()
  let total = 0n
  for (const [index, holding] of items.entries()) {
    const key = JSON.stringify(holderValue(holding.holder))
    if (holders.has(key)) throw new SaveSchemaError(`${path}[${index}].holder`, "持有人重复")
    holders.add(key)
    for (const lot of holding.lots) {
      if (lots.has(lot.id)) throw new SaveSchemaError(`${path}[${index}].lots`, "股份批次身份重复")
      lots.add(lot.id)
    }
    total += BigInt(holding.shares)
  }
  if (total > U64_MAX) throw new SaveSchemaError(path, "股份数量合计超出 u64")
  return { items: items.map(({ holder, lots }) => ({ holder: holderValue(holder), lots })), total: total.toString(), byHolder: new Map(items.map(item => [JSON.stringify(holderValue(item.holder)), item.shares])) }
}

function holderValue(holder: ReturnType<typeof parseHolder>) {
  if (holder.kind === "Account" || holder.kind === "External") return { [holder.kind]: holder.id } as HolderId
  return "IssuerTreasury" as const
}

function parseDayReceipt(value: unknown, path: string) {
  const receipt = record(value, path)
  exact(receipt, ["request", "disposals"], path)
  const request = record(receipt.request, `${path}.request`)
  exact(request, ["event_id", "day", "scope", "changes"], `${path}.request`)
  const event_id = string(request.event_id, `${path}.request.event_id`)
  if (event_id.trim() === "") throw new SaveSchemaError(`${path}.request.event_id`, "事件身份不能为空")
  const scope = request.scope
  let parsedScope: ShareDayRequest["scope"]
  if (scope === "PublicMarket") parsedScope = scope
  else if (typeof scope === "object" && scope !== null && !Array.isArray(scope) && Object.keys(scope).length === 1 && Object.hasOwn(scope, "NonTradingTransfer")) {
    const detail = record((scope as Record<string, unknown>).NonTradingTransfer, `${path}.request.scope.NonTradingTransfer`)
    exact(detail, ["basis"], `${path}.request.scope.NonTradingTransfer`)
    parsedScope = { NonTradingTransfer: { basis: string(detail.basis, `${path}.request.scope.NonTradingTransfer.basis`) } }
  } else if (typeof scope === "object" && scope !== null && !Array.isArray(scope) && Object.keys(scope).length === 1 && Object.hasOwn(scope, "IssuerRepurchaseCancellation")) {
    const detail = record((scope as Record<string, unknown>).IssuerRepurchaseCancellation, `${path}.request.scope.IssuerRepurchaseCancellation`)
    exact(detail, ["basis"], `${path}.request.scope.IssuerRepurchaseCancellation`)
    const basis = string(detail.basis, `${path}.request.scope.IssuerRepurchaseCancellation.basis`)
    if (!basis.trim()) throw new SaveSchemaError(`${path}.request.scope.IssuerRepurchaseCancellation.basis`, "回购注销法律依据不能为空")
    parsedScope = { IssuerRepurchaseCancellation: { basis } }
  } else if (typeof scope === "object" && scope !== null && !Array.isArray(scope) && Object.keys(scope).length === 1 && Object.hasOwn(scope, "ShareReDenomination")) {
    const detail = record((scope as Record<string, unknown>).ShareReDenomination, `${path}.request.scope.ShareReDenomination`)
    exact(detail, ["basis"], `${path}.request.scope.ShareReDenomination`)
    const basis = string(detail.basis, `${path}.request.scope.ShareReDenomination.basis`)
    if (!basis.trim()) throw new SaveSchemaError(`${path}.request.scope.ShareReDenomination.basis`, "拆股／缩股法律依据不能为空")
    parsedScope = { ShareReDenomination: { basis } }
  } else throw new SaveSchemaError(`${path}.request.scope`, "包含无效 MovementScope 变体")
  const changes: DayNetChange[] = array(request.changes, `${path}.request.changes`).map((item, index) => {
    const changePath = `${path}.request.changes[${index}]`
    const change = record(item, changePath)
    exact(change, ["holder", "change", "acquisition"], changePath)
    let acquisition: NetAcquisition | null = null
    if (change.acquisition !== null) {
      const source = record(change.acquisition, `${changePath}.acquisition`)
      exact(source, ["lot_id", "source", "restriction"], `${changePath}.acquisition`)
      const lot_id = string(source.lot_id, `${changePath}.acquisition.lot_id`)
      if (lot_id.trim() === "") throw new SaveSchemaError(`${changePath}.acquisition.lot_id`, "股份批次身份不能为空")
      acquisition = { lot_id, source: parseSource(source.source, `${changePath}.acquisition.source`), restriction: parseRestriction(source.restriction, `${changePath}.acquisition.restriction`) }
    }
    const changeValue = signedI128(change.change, `${changePath}.change`)
    if ((BigInt(changeValue) > 0n) !== (acquisition !== null)) throw new SaveSchemaError(`${changePath}.acquisition`, "仅正向股份变动必须含收购事实")
    return { holder: holderValue(parseHolder(change.holder, `${changePath}.holder`)), change: changeValue, acquisition }
  })
  const day = civilDate(request.day, `${path}.request.day`)
  const disposals = array(receipt.disposals, `${path}.disposals`).map((item, index) => {
    const disposalPath = `${path}.disposals[${index}]`
    const disposal = record(item, disposalPath)
    exact(disposal, ["holder", "lot", "disposed_on"], disposalPath)
    const holder = holderValue(parseHolder(disposal.holder, `${disposalPath}.holder`))
    const lot = parseLot(disposal.lot, `${disposalPath}.lot`)
    const disposed_on = civilDate(disposal.disposed_on, `${disposalPath}.disposed_on`)
    if (disposed_on !== day || lot.acquired_on > day) throw new SaveSchemaError(disposalPath, "处置日期必须等于日结日且不得早于批次取得日期")
    if (typeof lot.restriction === "object" && lot.restriction.Restricted.release_on > day) throw new SaveSchemaError(disposalPath, "公开转让处置了尚未解除限制的股份")
    return { holder, lot, disposed_on }
  })
  const changedHolders = new Set<string>()
  let net = 0n
  // scope 专属规则与引擎 `share_registry::validate_request` 同构：非交易过户与
  // 重新计值的正向新增都必须来自公司行为来源；减持限制只属于非交易过户——
  // 重新计值（缩股）合法产生负向变动与 FIFO 处置；回购注销恰一条 IssuerTreasury
  // 负向核减、无 acquisition、净额严格为负（引擎不可能产生其他形态）。
  const isNonTradingTransfer = typeof parsedScope === "object" && "NonTradingTransfer" in parsedScope
  const isRedenomination = typeof parsedScope === "object" && "ShareReDenomination" in parsedScope
  const isCancellation = typeof parsedScope === "object" && "IssuerRepurchaseCancellation" in parsedScope
  if (isCancellation) {
    // 与引擎 validate_request 的 cancellation 分支同构：恰一条专户核减分录、
    // 持有人必须是 IssuerTreasury、严格负向。「无 acquisition」一腿由解析层
    // 既有不变量（acquisition ⇔ 正向变动）覆盖，负向分录带 acquisition 在
    // changes 解析处已被「仅正向股份变动必须含收购事实」拒绝。
    if (changes.length !== 1) throw new SaveSchemaError(`${path}.request.changes`, "回购注销必须恰含一条专户核减分录")
    const [sole] = changes
    if (sole.holder !== "IssuerTreasury") throw new SaveSchemaError(`${path}.request.changes[0].holder`, "回购注销只能核减发行人库藏股持有人")
    if (BigInt(sole.change) >= 0n) throw new SaveSchemaError(`${path}.request.changes[0].change`, "回购注销分录必须为负向核减")
  }
  const nonTradingAcquisition = typeof parsedScope === "object"
  for (const [index, change] of changes.entries()) {
    const holderKey = JSON.stringify(change.holder)
    if (changedHolders.has(holderKey)) throw new SaveSchemaError(`${path}.request.changes[${index}].holder`, "日结请求重复包含持有人")
    changedHolders.add(holderKey)
    net += BigInt(change.change)
    if (net < I128_MIN || net > I128_MAX) throw new SaveSchemaError(`${path}.request.changes`, "日变动数量合计超出 i128")
    if (BigInt(change.change) > U64_MAX || BigInt(change.change) < -U64_MAX) throw new SaveSchemaError(`${path}.request.changes[${index}].change`, "日变动绝对值超出 u64")
    if (isNonTradingTransfer && BigInt(change.change) < 0n) throw new SaveSchemaError(`${path}.request.changes[${index}].change`, "非交易过户不得减持持有人股份")
    const disposed = disposals.filter(item => JSON.stringify(item.holder) === holderKey).reduce((sum, item) => sum + BigInt(item.lot.qty), 0n)
    const expected = BigInt(change.change) < 0n ? -BigInt(change.change) : 0n
    if (disposed !== expected) throw new SaveSchemaError(`${path}.disposals`, "处置数量与持有人净减持不一致")
    if (BigInt(change.change) > 0n) {
      const acquisition = change.acquisition as { readonly source: AcquisitionSource; readonly restriction: ShareRestriction }
      if (nonTradingAcquisition) {
        if (Object.keys(acquisition.source)[0] !== "CorporateAction") throw new SaveSchemaError(`${path}.request.changes[${index}].acquisition`, "非交易过户新增股份必须来自公司行为来源")
      } else if (Object.keys(acquisition.source)[0] !== "SecondaryMarket" || acquisition.restriction !== "Unrestricted") throw new SaveSchemaError(`${path}.request.changes[${index}].acquisition`, "公开市场买入必须为 SecondaryMarket 且股份无限制")
    }
  }
  if (isNonTradingTransfer) {
    if (net <= 0n) throw new SaveSchemaError(`${path}.request.changes`, "非交易过户必须新增正数股份")
  } else if (isRedenomination) {
    // 拆股／缩股必须净放大或净缩小总股本，且全体持有人同向；混正负是持有人间
    // 转移，不是重新计值（与引擎修复轮新增的同号校验一致）。
    if (net === 0n) throw new SaveSchemaError(`${path}.request.changes`, "重新计值必须净放大或净缩小总股本")
    const hasPositive = changes.some(change => BigInt(change.change) > 0n)
    const hasNegative = changes.some(change => BigInt(change.change) < 0n)
    if (hasPositive && hasNegative) throw new SaveSchemaError(`${path}.request.changes`, "重新计值变动必须同向：混正负属于持有人间转移")
  } else if (isCancellation) {
    // 恰一条负向分录之上再核对净额口径：注销净减总股本（引擎 validate_request
    // 同构；上方逐条断言已排除多分录、非专户、非负与带 acquisition 的形态）。
    if (net >= 0n) throw new SaveSchemaError(`${path}.request.changes`, "回购注销净额必须严格为负")
  } else if (net !== 0n) throw new SaveSchemaError(`${path}.request.changes`, "公开市场股份变动必须守恒")
  if (disposals.some(item => !changes.some(change => JSON.stringify(change.holder) === JSON.stringify(item.holder) && BigInt(change.change) < 0n))) throw new SaveSchemaError(`${path}.disposals`, "处置事实没有对应净减持")
  return { request: { event_id, day, scope: parsedScope, changes }, disposals }
}

function addCivilDays(day: string, count: number): string {
  const date = new Date(`${day}T00:00:00Z`)
  date.setUTCDate(date.getUTCDate() + count)
  return date.toISOString().slice(0, 10)
}

function parseRegistry(value: unknown, path: string) {
  const registry = record(value, path)
  exact(registry, ["stock", "issuer", "issued_shares", "issuer_repurchase_account", "settled_on", "holdings", "receipts", "registrations"], path)
  const stock = string(registry.stock, `${path}.stock`), issuer = string(registry.issuer, `${path}.issuer`)
  if (stock.trim() === "" || issuer.trim() === "") throw new SaveSchemaError(path, "证券和发行人身份不能为空")
  const issued_shares = positiveU64(registry.issued_shares, `${path}.issued_shares`)
  const settled_on = civilDate(registry.settled_on, `${path}.settled_on`)
  const issuer_repurchase_account = parseIssuerRepurchaseAccount(registry.issuer_repurchase_account, `${path}.issuer_repurchase_account`, settled_on)
  const holdings = parseHoldings(registry.holdings, `${path}.holdings`)
  if (holdings.total !== issued_shares) throw new SaveSchemaError(`${path}.holdings`, "持仓股份总量与发行股数不守恒")
  for (const [index, holding] of holdings.items.entries()) for (const lot of holding.lots) if (lot.acquired_on > settled_on) throw new SaveSchemaError(`${path}.holdings[${index}].lots`, "批次取得日期晚于名册结算日期")
  const receipts = array(registry.receipts, `${path}.receipts`).map((item, index) => parseDayReceipt(item, `${path}.receipts[${index}]`))
  const registrations = array(registry.registrations, `${path}.registrations`).map((item, index) => parseSnapshot(item, `${path}.registrations[${index}]`))
  const registrationEvents = new Set<string>()
  for (const [index, snapshot] of registrations.entries()) {
    if (registrationEvents.has(snapshot.event_id)) throw new SaveSchemaError(`${path}.registrations[${index}].event_id`, "登记快照事件身份重复")
    registrationEvents.add(snapshot.event_id)
  }
  const events = new Set<string>()
  const acquisitionIds = new Set<string>()
  let previousDay = ""
  for (const [index, receipt] of receipts.entries()) {
    if (events.has(receipt.request.event_id)) throw new SaveSchemaError(`${path}.receipts[${index}].request.event_id`, "事件身份重复")
    events.add(receipt.request.event_id)
    if (receipt.request.day > settled_on) throw new SaveSchemaError(`${path}.receipts[${index}].request.day`, "日结事件日期晚于股东名册日")
    const nonTrading = typeof receipt.request.scope === "object"
    if (previousDay !== "") {
      const consecutive = nonTrading ? receipt.request.day === previousDay : addCivilDays(previousDay, 1) === receipt.request.day
      if (!consecutive) throw new SaveSchemaError(`${path}.receipts[${index}].request.day`, "日结回执日期必须逐自然日连续，非交易过户与公开市场日结同日追加")
    }
    for (const [changeIndex, change] of receipt.request.changes.entries()) if (change.acquisition !== null) {
      if (acquisitionIds.has(change.acquisition.lot_id)) throw new SaveSchemaError(`${path}.receipts[${index}].request.changes[${changeIndex}].acquisition.lot_id`, "历史收购批次身份重复")
      acquisitionIds.add(change.acquisition.lot_id)
    }
    const disposalIds = new Set<string>()
    const lastDisposedDate = new Map<string, string>()
    for (const [disposalIndex, disposal] of receipt.disposals.entries()) {
      const holderKey = JSON.stringify(disposal.holder)
      const identity = `${holderKey}:${disposal.lot.id}`
      if (disposalIds.has(identity)) throw new SaveSchemaError(`${path}.receipts[${index}].disposals[${disposalIndex}]`, "同一日重复处置相同持有人批次")
      disposalIds.add(identity)
      const previous = lastDisposedDate.get(holderKey)
      if (previous !== undefined && previous > disposal.lot.acquired_on) throw new SaveSchemaError(`${path}.receipts[${index}].disposals[${disposalIndex}]`, "同一持有人的处置批次未按 FIFO 取得日期排序")
      lastDisposedDate.set(holderKey, disposal.lot.acquired_on)
    }
    previousDay = receipt.request.day
  }
  for (const [index, snapshot] of registrations.entries()) {
    // 历史登记快照的发行股数等于当前发行股数扣除其冻结之后（按回执序号）的
    // 非交易过户增发合计；同日先后顺序按 settled_receipts 回放。
    const settledIndex = Number(snapshot.settled_receipts)
    if (!Number.isSafeInteger(settledIndex) || settledIndex < 0 || settledIndex > receipts.length) throw new SaveSchemaError(`${path}.registrations[${index}].settled_receipts`, "登记时已落账回执数量超出名册结算历史")
    let issuedAfter = 0n
    for (const receipt of receipts.slice(settledIndex)) {
      if (typeof receipt.request.scope !== "object") continue
      for (const change of receipt.request.changes) issuedAfter += BigInt(change.change)
    }
    const issuedAtRegistration = BigInt(issued_shares) - issuedAfter
    if (snapshot.stock !== stock || snapshot.issuer !== issuer || BigInt(snapshot.issued_shares) !== issuedAtRegistration || snapshot.registered_on > settled_on) throw new SaveSchemaError(`${path}.registrations[${index}]`, "登记快照与股东名册身份、日期或按非交易过户回放的发行股数不一致")
    const expectedFacts = issuer_repurchase_account !== null && issuer_repurchase_account.established_on <= snapshot.registered_on ? issuer_repurchase_account : null
    if (JSON.stringify(snapshot.issuer_repurchase_account) !== JSON.stringify(expectedFacts)) throw new SaveSchemaError(`${path}.registrations[${index}].issuer_repurchase_account`, "登记快照回购专户事实与名册历史不一致")
  }
  if (receipts.length > 0 && receipts.at(-1)?.request.day !== settled_on) throw new SaveSchemaError(`${path}.receipts`, "最新日结回执日期必须等于股东名册结算日")
  return { stock, issuer, issued_shares, issuer_repurchase_account, settled_on, holdings: holdings.items, receipts, registrations, holderShares: holdings.byHolder }
}

function parseDividend(value: unknown, path: string): CashDividendBook {
  const book = record(value, path)
  exact(book, ["plan", "status", "registration", "entitlements", "paid", "failures", "payments"], path)
  const plan = record(book.plan, `${path}.plan`)
  exact(plan, ["plan_id", "issuer", "stock", "exchange", "formula", "approved_on", "announced_on", "registered_on", "ex_dividend_on", "payable_on", "gross_per_share", "distributable_amount"], `${path}.plan`)
  const plan_id = string(plan.plan_id, `${path}.plan.plan_id`), issuer = string(plan.issuer, `${path}.plan.issuer`), stock = string(plan.stock, `${path}.plan.stock`)
  if (!plan_id.trim() || !issuer.trim() || !stock.trim()) throw new SaveSchemaError(`${path}.plan`, "分红方案身份不能为空")
  const dates = ["approved_on", "announced_on", "registered_on", "ex_dividend_on", "payable_on"] as const
  const exchange = oneOf(plan.exchange, `${path}.plan.exchange`, ["sse", "szse"] as const)
  const formula = oneOf(plan.formula, `${path}.plan.formula`, ["StandardCashOnly", "ExchangeApprovedAdjustment"] as const)
  if (formula !== "StandardCashOnly") throw new SaveSchemaError(`${path}.plan.formula`, "当前 Session 不支持交易所批准的特殊除息调整方案")
  const parsedDates = Object.fromEntries(dates.map(key => [key, civilDate(plan[key], `${path}.plan.${key}`)])) as Record<typeof dates[number], string>
  if (!(parsedDates.approved_on <= parsedDates.announced_on && parsedDates.announced_on <= parsedDates.registered_on && parsedDates.registered_on < parsedDates.ex_dividend_on && parsedDates.ex_dividend_on <= parsedDates.payable_on)) throw new SaveSchemaError(`${path}.plan`, "分红方案日期顺序非法")
  const gross_per_share = money(plan.gross_per_share, `${path}.plan.gross_per_share`)
  const distributable_amount = money(plan.distributable_amount, `${path}.plan.distributable_amount`)
  if (!withinSixMonths(parsedDates.approved_on, parsedDates.payable_on) || BigInt(gross_per_share) <= 0n || BigInt(distributable_amount) < 0n) throw new SaveSchemaError(`${path}.plan`, "分红日期超期、每股金额非正或授权分配额为负")
  const status = string(book.status, `${path}.status`)
  if (!["Approved", "Announced", "Registered", "Payable", "PartiallyPaid", "Paid"].includes(status)) throw new SaveSchemaError(`${path}.status`, "包含无效分红状态")
  const registration = book.registration === null ? null : parseSnapshot(book.registration, `${path}.registration`)
  const entitlements = array(book.entitlements, `${path}.entitlements`).map((item, index) => {
    const entryPath = `${path}.entitlements[${index}]`, entry = record(item, entryPath)
    exact(entry, ["holder", "shares", "gross"], entryPath)
    const gross = money(entry.gross, `${entryPath}.gross`)
    if (BigInt(gross) < 0n) throw new SaveSchemaError(`${entryPath}.gross`, "分红权利金额不得为负")
    return { holder: holderValue(parseHolder(entry.holder, `${entryPath}.holder`)), shares: positiveU64(entry.shares, `${entryPath}.shares`), gross }
  })
  const paid = array(book.paid, `${path}.paid`).map((item, index) => parseHolderAmount(item, `${path}.paid[${index}]`))
  const failures = array(book.failures, `${path}.failures`).map((item, index) => {
    const pair = array(item, `${path}.failures[${index}]`)
    if (pair.length !== 2) throw new SaveSchemaError(`${path}.failures[${index}]`, "必须为二元组")
    const reason = string(pair[1], `${path}.failures[${index}][1]`)
    if (!reason.trim()) throw new SaveSchemaError(`${path}.failures[${index}][1]`, "失败原因不能为空")
    return [holderValue(parseHolder(pair[0], `${path}.failures[${index}][0]`)), reason] as const
  })
  const payments = array(book.payments, `${path}.payments`).map((item, index) => parsePayment(item, `${path}.payments[${index}]`))
  if ((status === "Approved" || status === "Announced") !== (registration === null)) throw new SaveSchemaError(`${path}.registration`, "登记快照 nullable 与分红状态不一致")
  if (registration !== null && (registration.event_id !== plan_id || registration.stock !== stock || registration.issuer !== issuer || registration.registered_on !== parsedDates.registered_on)) throw new SaveSchemaError(`${path}.registration`, "登记快照与分红方案不一致")
  if (registration === null && (entitlements.length || paid.length || failures.length || payments.length)) throw new SaveSchemaError(path, "登记前不得存在权利、支付或失败事实")
  if (registration !== null) {
    let total = 0n
    const expected = new Map<string, string>()
    for (const holding of registration.holdings) {
      const key = JSON.stringify(holding.holder)
      const shares = holding.lots.reduce((sum, lot) => sum + BigInt(lot.qty), 0n)
      if (shares > 0n && JSON.stringify(holding.holder) !== JSON.stringify("IssuerTreasury")) expected.set(key, shares.toString())
    }
    for (const [index, entitlement] of entitlements.entries()) {
      const key = JSON.stringify(entitlement.holder)
      if (expected.get(key) !== entitlement.shares || BigInt(entitlement.gross) !== BigInt(gross_per_share) * BigInt(entitlement.shares)) throw new SaveSchemaError(`${path}.entitlements[${index}]`, "分红权利与登记快照或每股金额不一致")
      expected.delete(key); total += BigInt(entitlement.gross)
    }
    if (expected.size || total > BigInt(distributable_amount) || total > I64_MAX) throw new SaveSchemaError(`${path}.entitlements`, "分红权利不完整、超过授权分配额或溢出 Money")
    if (expectedEligible(registration).length === 0) throw new SaveSchemaError(`${path}.entitlements`, "登记快照没有符合分红资格的持有人")
  }
  const parsedPlan: CashDividendPlan = { plan_id, issuer, stock, exchange, formula, ...parsedDates, gross_per_share, distributable_amount }
  const result: CashDividendBook = { plan: parsedPlan, status: status as CashDividendStatus, registration, entitlements, paid, failures, payments }
  validateDividendReplay(result, path)
  return result
}

function parseTaxSource(value: unknown, path: string): TaxAcquisitionSource {
  return parseSource(value, path)
}

function parseStockDistributionBook(value: unknown, path: string): StockDistributionBook {
  const book = record(value, path)
  exact(book, ["plan", "status", "registration", "receipt", "credited_on"], path)
  const planValue = record(book.plan, `${path}.plan`)
  exact(planValue, ["event_id", "approval_reference", "issuer", "stock", "exchange", "kind", "approved_on", "announced_on", "registered_on", "ex_rights_on", "shares_per_existing_share_micros", "approved_total_new_shares"], `${path}.plan`)
  const event_id = string(planValue.event_id, `${path}.plan.event_id`), approval_reference = string(planValue.approval_reference, `${path}.plan.approval_reference`), issuer = string(planValue.issuer, `${path}.plan.issuer`), stock = string(planValue.stock, `${path}.plan.stock`)
  if (!event_id.trim() || !approval_reference.trim() || !issuer.trim() || !stock.trim()) throw new SaveSchemaError(`${path}.plan`, "送转方案身份不能为空")
  const exchange = oneOf(planValue.exchange, `${path}.plan.exchange`, ["sse", "szse"] as const)
  const kind = oneOf(planValue.kind, `${path}.plan.kind`, ["BonusShares", "CapitalReserveConversion"] as const)
  const dates = ["approved_on", "announced_on", "registered_on", "ex_rights_on"] as const
  const parsedDates = Object.fromEntries(dates.map(key => [key, civilDate(planValue[key], `${path}.plan.${key}`)])) as Record<typeof dates[number], string>
  if (!(parsedDates.approved_on <= parsedDates.announced_on && parsedDates.announced_on <= parsedDates.registered_on && parsedDates.registered_on < parsedDates.ex_rights_on)) throw new SaveSchemaError(`${path}.plan`, "送转方案日期顺序非法")
  const shares_per_existing_share_micros = decimal(planValue.shares_per_existing_share_micros, `${path}.plan.shares_per_existing_share_micros`)
  const approved_total_new_shares = decimal(planValue.approved_total_new_shares, `${path}.plan.approved_total_new_shares`)
  if (shares_per_existing_share_micros === "0" || approved_total_new_shares === "0") throw new SaveSchemaError(`${path}.plan`, "送转比例与获批新增股数必须为正")
  const status = oneOf(book.status, `${path}.status`, ["Approved", "Announced", "Registered", "Credited"] as const)
  const registration = book.registration === null ? null : parseSnapshot(book.registration, `${path}.registration`)
  const receiptValue = book.receipt === null ? null : record(book.receipt, `${path}.receipt`)
  if ((registration === null) !== (receiptValue === null)) throw new SaveSchemaError(`${path}.receipt`, "登记快照与分配回执必须同时存在或同时为空")
  let receipt: StockDistributionReceipt | null = null
  if (receiptValue !== null) {
    exact(receiptValue, ["event_id", "approval_reference", "registration_event_id", "stock", "issuer", "registered_on", "issued_shares_before", "issuer_treasury_shares_excluded", "kind", "shares_per_existing_share_micros", "approved_total_new_shares", "tie_break_seed", "source_lot_attribution", "holders"], `${path}.receipt`)
    const holders = array(receiptValue.holders, `${path}.receipt.holders`).map((item, index) => {
      const holderPath = `${path}.receipt.holders[${index}]`, holder = record(item, holderPath)
      exact(holder, ["holder", "original_shares", "whole_shares", "fractional_numerator", "original_lots"], holderPath)
      const original_lots = array(holder.original_lots, `${holderPath}.original_lots`).map((lot, lotIndex) => parseLot(lot, `${holderPath}.original_lots[${lotIndex}]`))
      return {
        holder: holderValue(parseHolder(holder.holder, `${holderPath}.holder`)),
        original_shares: positiveU64(holder.original_shares, `${holderPath}.original_shares`),
        whole_shares: decimal(holder.whole_shares, `${holderPath}.whole_shares`),
        fractional_numerator: decimal(holder.fractional_numerator, `${holderPath}.fractional_numerator`),
        original_lots,
      }
    })
    if (receiptValue.source_lot_attribution !== "SourceLotAttributionPending") throw new SaveSchemaError(`${path}.receipt.source_lot_attribution`, "送转回执来源批次归属必须保持待定语义")
    receipt = {
      event_id: string(receiptValue.event_id, `${path}.receipt.event_id`),
      approval_reference: string(receiptValue.approval_reference, `${path}.receipt.approval_reference`),
      registration_event_id: string(receiptValue.registration_event_id, `${path}.receipt.registration_event_id`),
      stock: string(receiptValue.stock, `${path}.receipt.stock`),
      issuer: string(receiptValue.issuer, `${path}.receipt.issuer`),
      registered_on: civilDate(receiptValue.registered_on, `${path}.receipt.registered_on`),
      issued_shares_before: positiveU64(receiptValue.issued_shares_before, `${path}.receipt.issued_shares_before`),
      issuer_treasury_shares_excluded: decimal(receiptValue.issuer_treasury_shares_excluded, `${path}.receipt.issuer_treasury_shares_excluded`),
      kind: oneOf(receiptValue.kind, `${path}.receipt.kind`, ["BonusShares", "CapitalReserveConversion"] as const),
      shares_per_existing_share_micros: decimal(receiptValue.shares_per_existing_share_micros, `${path}.receipt.shares_per_existing_share_micros`),
      approved_total_new_shares: decimal(receiptValue.approved_total_new_shares, `${path}.receipt.approved_total_new_shares`),
      tie_break_seed: decimal(receiptValue.tie_break_seed, `${path}.receipt.tie_break_seed`),
      source_lot_attribution: "SourceLotAttributionPending",
      holders,
    }
    const awarded = holders.reduce((sum, holder) => sum + BigInt(holder.whole_shares), 0n)
    if (awarded !== BigInt(receipt.approved_total_new_shares)) throw new SaveSchemaError(`${path}.receipt.holders`, "每户整股合计与获批新增股数不一致")
    if (receipt.event_id !== event_id || receipt.approval_reference !== approval_reference || receipt.stock !== stock || receipt.issuer !== issuer || receipt.kind !== kind || receipt.shares_per_existing_share_micros !== shares_per_existing_share_micros || receipt.approved_total_new_shares !== approved_total_new_shares) throw new SaveSchemaError(`${path}.receipt`, "分配回执与送转方案不一致")
  }
  const credited_on = book.credited_on === null ? null : civilDate(book.credited_on, `${path}.credited_on`)
  if ((status === "Registered" || status === "Credited") !== (registration !== null)) throw new SaveSchemaError(`${path}.status`, "送转状态与冻结登记不一致")
  if (registration !== null && (registration.event_id !== event_id || registration.stock !== stock || registration.issuer !== issuer || registration.registered_on !== parsedDates.registered_on)) throw new SaveSchemaError(`${path}.registration`, "登记快照与送转方案不一致")
  if (credited_on !== null && (status !== "Credited" || credited_on !== parsedDates.ex_rights_on)) throw new SaveSchemaError(`${path}.credited_on`, "入账事实必须为 Credited 状态且等于 R+1 日期")
  if (registration === null && receipt === null && credited_on === null && !["Approved", "Announced"].includes(status)) throw new SaveSchemaError(`${path}.status`, "登记前不得出现入账事实")
  return { plan: { event_id, approval_reference, issuer, stock, exchange, kind, ...parsedDates, shares_per_existing_share_micros, approved_total_new_shares }, status, registration, receipt, credited_on }
}

/// 拆股／缩股方案条款的权威解析（账簿 plan 与公开公告共用；engine
/// `ShareSplitEventPlan` 的序列化形态）。
export function parseShareSplitEventPlanValue(value: unknown, path: string): ShareSplitEventPlan {
  const planValue = record(value, path)
  exact(planValue, ["event_id", "approval_reference", "issuer", "stock", "exchange", "direction", "ratio", "approved_on", "announced_on", "registered_on", "ex_rights_on"], path)
  const event_id = string(planValue.event_id, `${path}.event_id`), approval_reference = string(planValue.approval_reference, `${path}.approval_reference`), issuer = string(planValue.issuer, `${path}.issuer`), stock = string(planValue.stock, `${path}.stock`)
  if (!event_id.trim() || !approval_reference.trim() || !issuer.trim() || !stock.trim()) throw new SaveSchemaError(path, "拆股／缩股方案身份不能为空")
  const exchange = oneOf(planValue.exchange, `${path}.exchange`, ["sse", "szse"] as const)
  const direction = oneOf(planValue.direction, `${path}.direction`, ["Split", "Consolidate"] as const)
  const ratio = decimal(planValue.ratio, `${path}.ratio`)
  if (BigInt(ratio) < 2n) throw new SaveSchemaError(`${path}.ratio`, "拆股／缩股比例必须为 ≥2 的整数")
  const dates = ["approved_on", "announced_on", "registered_on", "ex_rights_on"] as const
  const parsedDates = Object.fromEntries(dates.map(key => [key, civilDate(planValue[key], `${path}.${key}`)])) as Record<typeof dates[number], string>
  if (!(parsedDates.approved_on <= parsedDates.announced_on && parsedDates.announced_on <= parsedDates.registered_on && parsedDates.registered_on < parsedDates.ex_rights_on)) throw new SaveSchemaError(path, "拆股／缩股方案日期顺序非法")
  return { event_id, approval_reference, issuer, stock, exchange, direction, ratio, ...parsedDates }
}

function parseShareSplitBook(value: unknown, path: string): ShareSplitBook {
  const book = record(value, path)
  exact(book, ["plan", "status", "registration", "receipt", "settled_on"], path)
  const plan = parseShareSplitEventPlanValue(book.plan, `${path}.plan`)
  const status = oneOf(book.status, `${path}.status`, ["Approved", "Announced", "Registered", "Settled"] as const)
  const registration = book.registration === null ? null : parseSnapshot(book.registration, `${path}.registration`)
  const receiptValue = book.receipt === null ? null : record(book.receipt, `${path}.receipt`)
  if ((registration === null) !== (receiptValue === null)) throw new SaveSchemaError(`${path}.receipt`, "登记快照与换算回执必须同时存在或同时为空")
  let receipt: ShareSplitReceipt | null = null
  if (receiptValue !== null) {
    exact(receiptValue, ["event_id", "approval_reference", "registration_event_id", "stock", "issuer", "registered_on", "direction", "ratio", "issued_shares_before", "issued_shares_after", "tie_break_seed", "holders"], `${path}.receipt`)
    const holders = array(receiptValue.holders, `${path}.receipt.holders`).map((item, index) => {
      const holderPath = `${path}.receipt.holders[${index}]`, holder = record(item, holderPath)
      exact(holder, ["holder", "original_shares", "new_shares", "fractional_numerator", "tie_break_award", "original_lots"], holderPath)
      const original_lots = array(holder.original_lots, `${holderPath}.original_lots`).map((lot, lotIndex) => parseLot(lot, `${holderPath}.original_lots[${lotIndex}]`))
      const original_shares = positiveU64(holder.original_shares, `${holderPath}.original_shares`)
      const new_shares = decimal(holder.new_shares, `${holderPath}.new_shares`)
      const fractional_numerator = decimal(holder.fractional_numerator, `${holderPath}.fractional_numerator`)
      const tie_break_award = decimal(holder.tie_break_award, `${holderPath}.tie_break_award`)
      if (BigInt(new_shares) === 0n && BigInt(original_shares) > 0n && BigInt(fractional_numerator) === 0n && BigInt(tie_break_award) === 0n) throw new SaveSchemaError(holderPath, "缩股归零换算必须携带碎股分子或整股奖励")
      return { holder: holderValue(parseHolder(holder.holder, `${holderPath}.holder`)), original_shares, new_shares, fractional_numerator, tie_break_award, original_lots }
    })
    receipt = {
      event_id: string(receiptValue.event_id, `${path}.receipt.event_id`),
      approval_reference: string(receiptValue.approval_reference, `${path}.receipt.approval_reference`),
      registration_event_id: string(receiptValue.registration_event_id, `${path}.receipt.registration_event_id`),
      stock: string(receiptValue.stock, `${path}.receipt.stock`),
      issuer: string(receiptValue.issuer, `${path}.receipt.issuer`),
      registered_on: civilDate(receiptValue.registered_on, `${path}.receipt.registered_on`),
      direction: oneOf(receiptValue.direction, `${path}.receipt.direction`, ["Split", "Consolidate"] as const),
      ratio: decimal(receiptValue.ratio, `${path}.receipt.ratio`),
      issued_shares_before: positiveU64(receiptValue.issued_shares_before, `${path}.receipt.issued_shares_before`),
      issued_shares_after: positiveU64(receiptValue.issued_shares_after, `${path}.receipt.issued_shares_after`),
      tie_break_seed: decimal(receiptValue.tie_break_seed, `${path}.receipt.tie_break_seed`),
      holders,
    }
    const originalTotal = holders.reduce((sum, holder) => sum + BigInt(holder.original_shares), 0n)
    const afterTotal = holders.reduce((sum, holder) => sum + BigInt(holder.new_shares), 0n)
    if (originalTotal !== BigInt(receipt.issued_shares_before) || afterTotal !== BigInt(receipt.issued_shares_after)) throw new SaveSchemaError(`${path}.receipt.holders`, "换算回执持有人合计与前后总股数不一致")
    if (receipt.event_id !== plan.event_id || receipt.approval_reference !== plan.approval_reference || receipt.stock !== plan.stock || receipt.issuer !== plan.issuer || receipt.direction !== plan.direction || receipt.ratio !== plan.ratio) throw new SaveSchemaError(`${path}.receipt`, "换算回执与拆股／缩股方案不一致")
    if (plan.direction === "Split") {
      if (afterTotal !== originalTotal * BigInt(plan.ratio)) throw new SaveSchemaError(`${path}.receipt.holders`, "拆股换算必须精确放大整数倍")
      if (holders.some(holder => BigInt(holder.fractional_numerator) !== 0n || BigInt(holder.tie_break_award) !== 0n)) throw new SaveSchemaError(`${path}.receipt.holders`, "拆股不产生碎股分子或整股奖励")
    } else if (afterTotal > originalTotal) {
      throw new SaveSchemaError(`${path}.receipt.holders`, "缩股换算不得放大总股数")
    }
  }
  const settled_on = book.settled_on === null ? null : civilDate(book.settled_on, `${path}.settled_on`)
  if ((status === "Registered" || status === "Settled") !== (registration !== null)) throw new SaveSchemaError(`${path}.status`, "拆股／缩股状态与冻结登记不一致")
  if (registration !== null && (registration.event_id !== plan.event_id || registration.stock !== plan.stock || registration.issuer !== plan.issuer || registration.registered_on !== plan.registered_on)) throw new SaveSchemaError(`${path}.registration`, "登记快照与拆股／缩股方案不一致")
  if (settled_on !== null && (status !== "Settled" || settled_on !== plan.ex_rights_on)) throw new SaveSchemaError(`${path}.settled_on`, "入账事实必须为 Settled 状态且等于 R+1 日期")
  if (registration === null && receipt === null && settled_on === null && !["Approved", "Announced"].includes(status)) throw new SaveSchemaError(`${path}.status`, "登记前不得出现入账事实")
  return { plan, status, registration, receipt, settled_on }
}

function parseTaxClass(value: unknown, path: string): TaxShareClass {
  if (value === "PublicMarket") return value
  const tagged = record(value, path)
  exact(tagged, ["StatutoryRestricted"], path)
  const restricted = record(tagged.StatutoryRestricted, `${path}.StatutoryRestricted`)
  exact(restricted, ["release_on", "basis", "qualification_evidence"], `${path}.StatutoryRestricted`)
  return {
    StatutoryRestricted: {
      release_on: civilDate(restricted.release_on, `${path}.StatutoryRestricted.release_on`),
      basis: oneOf(restricted.basis, `${path}.StatutoryRestricted.basis`, ["FinanceTax2009167", "FinanceTax201070"] as const),
      qualification_evidence: string(restricted.qualification_evidence, `${path}.StatutoryRestricted.qualification_evidence`),
    },
  }
}

function parseTaxLot(value: unknown, path: string): DividendTaxLot {
  const lot = record(value, path)
  exact(lot, ["id", "qty", "acquired_on", "source", "class"], path)
  const id = string(lot.id, `${path}.id`)
  if (id.trim() === "") throw new SaveSchemaError(`${path}.id`, "税股批次身份不能为空")
  return {
    id,
    qty: positiveU64(lot.qty, `${path}.qty`),
    acquired_on: civilDate(lot.acquired_on, `${path}.acquired_on`),
    source: parseTaxSource(lot.source, `${path}.source`),
    class: parseTaxClass(lot.class, `${path}.class`),
  }
}

function greatestCommonDivisor(left: bigint, right: bigint): bigint {
  while (right !== 0n) {
    const remainder = left % right
    left = right
    right = remainder
  }
  return left
}

function parseExactAmount(value: unknown, path: string): ExactDividendTaxAmount {
  const amount = record(value, path)
  exact(amount, ["numerator", "denominator"], path)
  const numerator = signedI128Fraction(amount.numerator, `${path}.numerator`)
  const denominator = decimal(amount.denominator, `${path}.denominator`)
  if (denominator === "0" || BigInt(numerator) < 0n || greatestCommonDivisor(BigInt(numerator), BigInt(denominator)) !== 1n) throw new SaveSchemaError(path, "精确税额必须为已约简非负分数")
  return { numerator, denominator }
}

function parseTaxBook(value: unknown, path: string): CashDividendTaxBook {
  const book = record(value, path)
  exact(book, ["operation_seq", "account", "stock", "profile", "opened_on", "opening_lots", "settled_on", "lots", "days", "dividends", "collections", "redenominations"], path)
  const profile = oneOf(book.profile, `${path}.profile`, ["IndividualPublicMarket", "ResidentEnterprise", "SecuritiesFund", "NonResident"] as const)
  if (profile !== "IndividualPublicMarket") throw new SaveSchemaError(`${path}.profile`, "当前仅实现个人公开市场现金分红税")
  const account = accountId(book.account, `${path}.account`)
  const stock = string(book.stock, `${path}.stock`)
  if (stock.trim() === "") throw new SaveSchemaError(`${path}.stock`, "税股代码不能为空")
  const opened_on = civilDate(book.opened_on, `${path}.opened_on`)
  const settled_on = civilDate(book.settled_on, `${path}.settled_on`)
  if (settled_on < opened_on) throw new SaveSchemaError(`${path}.settled_on`, "税账结算日早于开账日")
  const days: TaxDayReceipt[] = array(book.days, `${path}.days`).map((item, index) => {
    const dayPath = `${path}.days[${index}]`, receipt = record(item, dayPath)
    exact(receipt, ["operation_seq", "event_id", "day", "net_change", "acquisition", "dispositions"], dayPath)
    const event_id = string(receipt.event_id, `${dayPath}.event_id`)
    if (event_id.trim() === "") throw new SaveSchemaError(`${dayPath}.event_id`, "税账日结事件身份不能为空")
    return {
      operation_seq: integer(receipt.operation_seq, `${dayPath}.operation_seq`, 1),
      event_id,
      day: civilDate(receipt.day, `${dayPath}.day`),
      net_change: signedI128(receipt.net_change, `${dayPath}.net_change`),
      acquisition: receipt.acquisition === null ? null : parseTaxLot(receipt.acquisition, `${dayPath}.acquisition`),
      dispositions: array(receipt.dispositions, `${dayPath}.dispositions`).map((entry, entryIndex) => {
        const dispositionPath = `${dayPath}.dispositions[${entryIndex}]`, disposition = record(entry, dispositionPath)
        exact(disposition, ["lot", "disposed_on"], dispositionPath)
        return { lot: parseTaxLot(disposition.lot, `${dispositionPath}.lot`), disposed_on: civilDate(disposition.disposed_on, `${dispositionPath}.disposed_on`) }
      }),
    }
  })
  const dividends: RegisteredTaxDividend[] = array(book.dividends, `${path}.dividends`).map((item, index) => {
    const dividendPath = `${path}.dividends[${index}]`, dividend = record(item, dividendPath)
    exact(dividend, ["operation_seq", "event_id", "registered_on", "per_share", "lots", "payments"], dividendPath)
    const event_id = string(dividend.event_id, `${dividendPath}.event_id`)
    if (event_id.trim() === "") throw new SaveSchemaError(`${dividendPath}.event_id`, "税账分红事件身份不能为空")
    return {
      operation_seq: integer(dividend.operation_seq, `${dividendPath}.operation_seq`, 1),
      event_id,
      registered_on: civilDate(dividend.registered_on, `${dividendPath}.registered_on`),
      per_share: parseExactAmount(dividend.per_share, `${dividendPath}.per_share`),
      lots: array(dividend.lots, `${dividendPath}.lots`).map((lot, lotIndex) => parseTaxLot(lot, `${dividendPath}.lots[${lotIndex}]`)),
      payments: array(dividend.payments, `${dividendPath}.payments`).map((payment, paymentIndex) => {
        const paymentPath = `${dividendPath}.payments[${paymentIndex}]`, receipt = record(payment, paymentPath)
        exact(receipt, ["operation_seq", "payment_id", "paid_on", "received_gross", "evidence"], paymentPath)
        const payment_id = string(receipt.payment_id, `${paymentPath}.payment_id`)
        const received_gross = money(receipt.received_gross, `${paymentPath}.received_gross`)
        const evidence = string(receipt.evidence, `${paymentPath}.evidence`)
        if (!payment_id.trim() || !evidence.trim() || BigInt(received_gross) <= 0n) throw new SaveSchemaError(paymentPath, "税账分红到账身份、证据或金额非法")
        return {
          operation_seq: integer(receipt.operation_seq, `${paymentPath}.operation_seq`, 1),
          payment_id,
          paid_on: civilDate(receipt.paid_on, `${paymentPath}.paid_on`),
          received_gross,
          evidence,
        }
      }),
    }
  })
  const collections: TaxCollectionReceipt[] = array(book.collections, `${path}.collections`).map((item, index) => {
    const collectionPath = `${path}.collections[${index}]`, receipt = record(item, collectionPath)
    exact(receipt, ["operation_seq", "event_id", "day", "available_cash", "collected", "remaining_cash", "outstanding", "needs_funds"], collectionPath)
    const event_id = string(receipt.event_id, `${collectionPath}.event_id`)
    if (event_id.trim() === "") throw new SaveSchemaError(`${collectionPath}.event_id`, "税账收缴事件身份不能为空")
    return {
      operation_seq: integer(receipt.operation_seq, `${collectionPath}.operation_seq`, 1),
      event_id,
      day: civilDate(receipt.day, `${collectionPath}.day`),
      available_cash: money(receipt.available_cash, `${collectionPath}.available_cash`),
      collected: money(receipt.collected, `${collectionPath}.collected`),
      remaining_cash: money(receipt.remaining_cash, `${collectionPath}.remaining_cash`),
      outstanding: parseExactAmount(receipt.outstanding, `${collectionPath}.outstanding`),
      needs_funds: boolean(receipt.needs_funds, `${collectionPath}.needs_funds`),
    }
  })
  const opening_lots = array(book.opening_lots, `${path}.opening_lots`).map((lot, index) => parseTaxLot(lot, `${path}.opening_lots[${index}]`))
  const lots = array(book.lots, `${path}.lots`).map((lot, index) => parseTaxLot(lot, `${path}.lots[${index}]`))
  const redenominations: TaxRedenominationReceipt[] = array(book.redenominations, `${path}.redenominations`).map((item, index) => {
    const reductionPath = `${path}.redenominations[${index}]`, receipt = record(item, reductionPath)
    exact(receipt, ["operation_seq", "event_id", "day", "net_change", "removed"], reductionPath)
    const event_id = string(receipt.event_id, `${reductionPath}.event_id`)
    if (event_id.trim() === "") throw new SaveSchemaError(`${reductionPath}.event_id`, "税账重新计值事件身份不能为空")
    const net_change = signedI128(receipt.net_change, `${reductionPath}.net_change`)
    if (BigInt(net_change) >= 0n) throw new SaveSchemaError(`${reductionPath}.net_change`, "税账重新计值核减必须为净负")
    const removed = array(receipt.removed, `${reductionPath}.removed`).map((entry, entryIndex) => {
      const removalPath = `${reductionPath}.removed[${entryIndex}]`, removal = record(entry, removalPath)
      exact(removal, ["lot", "disposed_on"], removalPath)
      return { lot: parseTaxLot(removal.lot, `${removalPath}.lot`), disposed_on: civilDate(removal.disposed_on, `${removalPath}.disposed_on`) }
    })
    const total = removed.reduce((sum, entry) => sum + BigInt(entry.lot.qty), 0n)
    if (total !== -BigInt(net_change)) throw new SaveSchemaError(`${reductionPath}.removed`, "税账重新计值核减片段合计与净额不一致")
    return { operation_seq: integer(receipt.operation_seq, `${reductionPath}.operation_seq`, 1), event_id, day: civilDate(receipt.day, `${reductionPath}.day`), net_change, removed }
  })
  const parsed: CashDividendTaxBook = { operation_seq: integer(book.operation_seq, `${path}.operation_seq`, 0), account, stock, profile, opened_on, opening_lots, settled_on, lots, days, dividends, collections, redenominations }
  validateTaxBookReplay(parsed, path)
  return parsed
}

function validateTaxBookReplay(book: CashDividendTaxBook, path: string): void {
  const operations = [
    ...book.days.map(receipt => ({ operation_seq: receipt.operation_seq, path: "days", day: receipt.day })),
    ...book.dividends.flatMap(dividend => [
      { operation_seq: dividend.operation_seq, path: "dividends", day: dividend.registered_on },
      ...dividend.payments.map(payment => ({ operation_seq: payment.operation_seq, path: "dividends payments", day: payment.paid_on })),
    ]),
    ...book.collections.map(receipt => ({ operation_seq: receipt.operation_seq, path: "collections", day: receipt.day })),
    ...book.redenominations.map(receipt => ({ operation_seq: receipt.operation_seq, path: "redenominations", day: receipt.day })),
  ].sort((left, right) => left.operation_seq - right.operation_seq)
  if (book.operation_seq !== operations.length) throw new SaveSchemaError(`${path}.operation_seq`, "税账操作序列与事实数量不一致")
  let previousDay = book.opened_on
  for (const [index, operation] of operations.entries()) {
    if (operation.operation_seq !== index + 1) throw new SaveSchemaError(`${path}.${operation.path.replaceAll(" ", ".")}[${index}]`, "税账操作序列必须从 1 连续递增")
    if (operation.day < previousDay || operation.day > book.settled_on) throw new SaveSchemaError(`${path}.${operation.path.replaceAll(" ", ".")}[${index}]`, "税账事实日期不在前序事实与结算日之间")
    previousDay = operation.day
  }
  const eventIds = new Set<string>()
  const paymentIds = new Set<string>()
  for (const fact of [...book.days, ...book.dividends, ...book.collections, ...book.redenominations]) {
    if (eventIds.has(fact.event_id)) throw new SaveSchemaError(path, "税账事件身份重复")
    eventIds.add(fact.event_id)
  }
  for (const dividend of book.dividends) {
    for (const payment of dividend.payments) {
      if (paymentIds.has(payment.payment_id)) throw new SaveSchemaError(path, "税账付款身份重复")
      paymentIds.add(payment.payment_id)
      if (payment.paid_on < dividend.registered_on) throw new SaveSchemaError(path, "税账付款早于分红登记")
    }
  }
  let dayCursor = book.opened_on
  let seenAnyDay = false
  for (const [index, day] of book.days.entries()) {
    const dayPath = `${path}.days[${index}]`
    // 同日正向续记：R+1 送转到账在当日公开市场日结之后追加一条正向净增
    // （财税〔2012〕85号第三条每日期末净增事实）；除此之外日结仍须逐日连续，
    // 首条日结必须紧邻开账日的下一自然日（seenAnyDay=false 时续记不可用）。
    const sameDayContinuation = seenAnyDay && day.day === dayCursor && BigInt(day.net_change) > 0n
    if (!sameDayContinuation && addCivilDays(dayCursor, 1) !== day.day) throw new SaveSchemaError(`${dayPath}.day`, "税账日结必须紧邻开账日或前一日结的下一自然日（同日仅允许正向公司行为续记）")
    if ((BigInt(day.net_change) > 0n) !== (day.acquisition !== null)) throw new SaveSchemaError(`${dayPath}.acquisition`, "税账日净变动与新增批次不一致")
    if (day.acquisition !== null && (day.acquisition.acquired_on !== day.day || day.acquisition.qty !== day.net_change)) throw new SaveSchemaError(`${dayPath}.acquisition`, "税账新增批次日期或数量不一致")
    let disposed = 0n
    for (const disposition of day.dispositions) {
      if (disposition.disposed_on !== day.day) throw new SaveSchemaError(`${dayPath}.dispositions`, "税账处置日期与日结日期不一致")
      disposed += BigInt(disposition.lot.qty)
    }
    // 处置数量只对净减持日等于减持量；净增日（市场买入或送转到账续记）处置必须为零，
    // 与引擎 validate 的 `expected = net < 0 ? -net : 0` 同构（附带修复：原实现对
    // 正向净增日恒要求负处置量，任何含买入日结的真实存档都会被误拒）。
    const expectedDisposed = BigInt(day.net_change) < 0n ? -BigInt(day.net_change) : 0n
    if (disposed !== expectedDisposed) throw new SaveSchemaError(`${dayPath}.dispositions`, "税账处置数量与日净减持不一致")
    seenAnyDay = true
    dayCursor = day.day
  }
  const replayed = book.opening_lots.map(lot => ({ ...lot }))
  // 与引擎 validate 同构：按 operation_seq 交织回放日结与重新计值核减——缩股的
  // FIFO 核减片段会真实减少 lot 数量（非应税、不进处置口径），不应用核减就无法
  // 重构缩股后的当前批次。
  const sequenceOf = (fact: { kind: "day"; day: typeof book.days[number] } | { kind: "redenomination"; receipt: TaxRedenominationReceipt }): number =>
    fact.kind === "day" ? fact.day.operation_seq : fact.receipt.operation_seq
  const sequencedFacts: ({ kind: "day"; day: typeof book.days[number] } | { kind: "redenomination"; receipt: TaxRedenominationReceipt })[] = [
    ...book.days.map((day): { kind: "day"; day: typeof book.days[number] } => ({ kind: "day", day })),
    ...book.redenominations.map((receipt): { kind: "redenomination"; receipt: TaxRedenominationReceipt } => ({ kind: "redenomination", receipt })),
  ].sort((left, right) => sequenceOf(left) - sequenceOf(right))
  for (const fact of sequencedFacts) {
    if (fact.kind === "day") {
      const day = fact.day
      if (day.acquisition !== null) replayed.push({ ...day.acquisition })
      for (const disposition of day.dispositions) {
        const index = replayed.findIndex(lot => lot.id === disposition.lot.id)
        if (index === -1 || BigInt(replayed[index]!.qty) < BigInt(disposition.lot.qty)) throw new SaveSchemaError(path, "税账处置引用不存在的批次或数量")
        const remaining = BigInt(replayed[index]!.qty) - BigInt(disposition.lot.qty)
        if (remaining === 0n) replayed.splice(index, 1)
        else replayed[index] = { ...replayed[index]!, qty: remaining.toString() }
      }
    } else {
      for (const entry of fact.receipt.removed) {
        const index = replayed.findIndex(lot => lot.id === entry.lot.id)
        if (index === -1 || BigInt(replayed[index]!.qty) < BigInt(entry.lot.qty)) throw new SaveSchemaError(path, "税账重新计值核减引用不存在的批次或数量")
        const remaining = BigInt(replayed[index]!.qty) - BigInt(entry.lot.qty)
        if (remaining === 0n) replayed.splice(index, 1)
        else replayed[index] = { ...replayed[index]!, qty: remaining.toString() }
      }
    }
  }
  if (JSON.stringify(replayed) !== JSON.stringify(book.lots)) throw new SaveSchemaError(`${path}.lots`, "税账当前批次与开账批次及日结 replay 不一致")
  for (const [index, collection] of book.collections.entries()) {
    const collectionPath = `${path}.collections[${index}]`
    if (BigInt(collection.available_cash) !== BigInt(collection.collected) + BigInt(collection.remaining_cash)) throw new SaveSchemaError(collectionPath, "税账收缴前后现金不守恒")
    if (collection.needs_funds !== BigInt(collection.outstanding.numerator) > 0n) throw new SaveSchemaError(`${collectionPath}.needs_funds`, "税账资金不足标志与剩余税额不一致")
  }
}

function expectedEligible(registration: RegistrationSnapshot) {
  return registration.holdings.filter(holding => holding.holder !== "IssuerTreasury" && holding.lots.length > 0)
}

function holderCompare(left: HolderId, right: HolderId): number {
  const rank = (holder: HolderId): number => typeof holder === "string" ? 2 : "Account" in holder ? 0 : 1
  const leftRank = rank(left), rightRank = rank(right)
  if (leftRank !== rightRank) return leftRank - rightRank
  const id = (holder: HolderId): string => typeof holder === "string" ? "" : "Account" in holder ? holder.Account : holder.External
  const leftId = id(left), rightId = id(right)
  if (leftRank === 0) {
    const leftNumber = BigInt(leftId), rightNumber = BigInt(rightId)
    return leftNumber < rightNumber ? -1 : leftNumber > rightNumber ? 1 : 0
  }
  const leftBytes = new TextEncoder().encode(leftId), rightBytes = new TextEncoder().encode(rightId)
  for (let index = 0; index < Math.min(leftBytes.length, rightBytes.length); index += 1) if (leftBytes[index] !== rightBytes[index]) return leftBytes[index]! - rightBytes[index]!
  return leftBytes.length - rightBytes.length
}

function withinSixMonths(start: string, end: string): boolean {
  const monthDelta = (Number(end.slice(0, 4)) - Number(start.slice(0, 4))) * 12 + Number(end.slice(5, 7)) - Number(start.slice(5, 7))
  return monthDelta < 6 || (monthDelta === 6 && Number(end.slice(8, 10)) <= Number(start.slice(8, 10)))
}

function validateDividendReplay(book: CashDividendBook, path: string): void {
  const { plan, registration, entitlements, payments } = book
  const entitlementsByHolder = new Map(entitlements.map(item => [JSON.stringify(item.holder), item.gross]))
  const paidHolders = new Set<string>()
  const failedHolders = new Map<string, string>()
  const paidSerialized = new Map(book.paid.map(([holder, amount]) => [JSON.stringify(holder), amount]))
  const failuresSerialized = new Map(book.failures.map(([holder, reason]) => [JSON.stringify(holder), reason]))
  if (paidSerialized.size !== book.paid.length || failuresSerialized.size !== book.failures.length) throw new SaveSchemaError(path, "支付或失败持有人重复")
  for (const [holder, amount] of book.paid) {
    if (entitlementsByHolder.get(JSON.stringify(holder)) !== amount || book.failures.some(([failed]) => JSON.stringify(failed) === JSON.stringify(holder))) throw new SaveSchemaError(`${path}.paid`, "已支付持有人金额或失败事实不一致")
  }
  for (const [holder, reason] of book.failures) if (!entitlementsByHolder.has(JSON.stringify(holder)) || paidSerialized.has(JSON.stringify(holder)) || !reason.trim()) throw new SaveSchemaError(`${path}.failures`, "失败事实须对应未支付权利且原因非空")
  const ids = new Set<string>()
  let previousDate: string | undefined
  for (const [index, payment] of payments.entries()) {
    const paymentPath = `${path}.payments[${index}]`
    if (!payment.payment_id.trim() || ids.has(payment.payment_id) || payment.paid_on < plan.payable_on || (previousDate !== undefined && payment.paid_on < previousDate) || payment.within_six_month_deadline !== withinSixMonths(plan.approved_on, payment.paid_on)) throw new SaveSchemaError(paymentPath, "支付回执身份、日期或六个月时限标志非法")
    ids.add(payment.payment_id)
    previousDate = payment.paid_on
    for (let item = 1; item < payment.outcomes.length; item += 1) if (holderCompare(outcomeHolder(payment.outcomes[item - 1]!), outcomeHolder(payment.outcomes[item]!)) >= 0) throw new SaveSchemaError(`${paymentPath}.outcomes`, "持有人结果必须按身份严格排序且不重复")
    const outstanding = [...entitlementsByHolder].filter(([key]) => !paidHolders.has(key))
    if (payment.outcomes.length !== outstanding.length) throw new SaveSchemaError(`${paymentPath}.outcomes`, "支付回执必须覆盖每个尚未支付持有人")
    const receiptHolders = new Set<string>()
    for (const outcome of payment.outcomes) {
      const holder = outcomeHolder(outcome), key = JSON.stringify(holder)
      if (receiptHolders.has(key)) throw new SaveSchemaError(`${paymentPath}.outcomes`, "支付回执中的持有人重复")
      receiptHolders.add(key)
      const expected = entitlementsByHolder.get(key)
      if (expected === undefined) throw new SaveSchemaError(`${paymentPath}.outcomes`, "支付回执持有人没有未支付权利")
      if ("Paid" in outcome) {
        if (outcome.Paid.amount !== expected) throw new SaveSchemaError(`${paymentPath}.outcomes`, "支付金额与冻结分红权利不一致")
        paidHolders.add(key)
        failedHolders.delete(key)
      } else {
        failedHolders.set(key, outcome.Failed.reason)
      }
    }
  }
  const replayedPaid = [...paidHolders].sort((left, right) => holderCompare(JSON.parse(left) as HolderId, JSON.parse(right) as HolderId))
  const replayedFailures = [...failedHolders].sort(([left], [right]) => holderCompare(JSON.parse(left) as HolderId, JSON.parse(right) as HolderId))
  const actualPaid = [...paidSerialized.keys()].sort((left, right) => holderCompare(JSON.parse(left) as HolderId, JSON.parse(right) as HolderId))
  const actualFailures = [...failuresSerialized].sort(([left], [right]) => holderCompare(JSON.parse(left) as HolderId, JSON.parse(right) as HolderId))
  if (JSON.stringify(replayedPaid) !== JSON.stringify(actualPaid) || JSON.stringify(replayedFailures) !== JSON.stringify(actualFailures)) throw new SaveSchemaError(path, "支付回执重放结果与 paid／failures 不一致")
  const expectedStatus = payments.length > 0 && book.paid.length === entitlements.length && registration !== null
    ? "Paid"
    : book.paid.length > 0 ? "PartiallyPaid"
    : payments.length > 0 || book.status === "Payable" ? "Payable"
    : registration !== null ? "Registered"
    : book.status === "Announced" ? "Announced" : "Approved"
  if (book.status !== expectedStatus) throw new SaveSchemaError(`${path}.status`, "分红状态与登记及付款事实重放结果不一致")
}

function outcomeHolder(outcome: HolderPaymentOutcome): HolderId {
  return "Paid" in outcome ? outcome.Paid.holder : outcome.Failed.holder
}

function parseHolderAmount(value: unknown, path: string) {
  const pair = array(value, path)
  if (pair.length !== 2) throw new SaveSchemaError(path, "必须为二元组")
  return [holderValue(parseHolder(pair[0], `${path}[0]`)), money(pair[1], `${path}[1]`)] as const
}

function parsePayment(value: unknown, path: string): CashDividendPaymentReceipt {
  const payment = record(value, path)
  exact(payment, ["payment_id", "paid_on", "within_six_month_deadline", "outcomes"], path)
  const outcomes: HolderPaymentOutcome[] = array(payment.outcomes, `${path}.outcomes`).map((item, index) => {
    const outcomePath = `${path}.outcomes[${index}]`, tagged = record(item, outcomePath)
    if (Object.keys(tagged).length !== 1) throw new SaveSchemaError(outcomePath, "必须是单一 HolderPaymentOutcome 变体")
    const variant = Object.keys(tagged)[0]!
    const payload = record(tagged[variant], `${outcomePath}.${variant}`)
    if (variant === "Paid") {
      exact(payload, ["holder", "amount"], `${outcomePath}.Paid`)
      return { Paid: { holder: holderValue(parseHolder(payload.holder, `${outcomePath}.Paid.holder`)), amount: money(payload.amount, `${outcomePath}.Paid.amount`) } }
    }
    if (variant === "Failed") {
      exact(payload, ["holder", "reason"], `${outcomePath}.Failed`)
      const reason = string(payload.reason, `${outcomePath}.Failed.reason`)
      if (!reason.trim()) throw new SaveSchemaError(`${outcomePath}.Failed.reason`, "失败原因不能为空")
      return { Failed: { holder: holderValue(parseHolder(payload.holder, `${outcomePath}.Failed.holder`)), reason } }
    }
    throw new SaveSchemaError(outcomePath, "包含无效 HolderPaymentOutcome 变体")
  })
  return { payment_id: string(payment.payment_id, `${path}.payment_id`), paid_on: civilDate(payment.paid_on, `${path}.paid_on`), within_six_month_deadline: boolean(payment.within_six_month_deadline, `${path}.within_six_month_deadline`), outcomes }
}

export function parseSessionCorporateActions(value: unknown, context: Context, path = "corporate_actions") {
  const root = record(value, path)
  exact(root, ["registries", "dividends", "dividend_tax_books", "stock_distributions", "share_splits", "rights_offerings", "rights_subscription_queue", "rejected_rights_subscriptions", "issuer_repurchases", "account_gross_receipts", "external_receipts", "applied_ex_reference_groups"], path)
  const registries = array(root.registries, `${path}.registries`).map((item, index) => parseRegistry(item, `${path}.registries[${index}]`))
  const dividends = array(root.dividends, `${path}.dividends`).map((item, index) => parseDividend(item, `${path}.dividends[${index}]`))
  const dividend_tax_books = array(root.dividend_tax_books, `${path}.dividend_tax_books`).map((item, index) => parseTaxBook(item, `${path}.dividend_tax_books[${index}]`))
  const stock_distributions = array(root.stock_distributions, `${path}.stock_distributions`).map((item, index) => parseStockDistributionBook(item, `${path}.stock_distributions[${index}]`))
  const share_splits = array(root.share_splits, `${path}.share_splits`).map((item, index) => parseShareSplitBook(item, `${path}.share_splits[${index}]`))
  const rights_offerings = array(root.rights_offerings, `${path}.rights_offerings`).map((item, index) => parseRightsOfferingBook(item, `${path}.rights_offerings[${index}]`))
  const rights_subscription_queue: QueuedRightsSubscription[] = array(root.rights_subscription_queue, `${path}.rights_subscription_queue`).map((item, index) => {
    const itemPath = `${path}.rights_subscription_queue[${index}]`, entry = record(item, itemPath)
    exact(entry, ["event_id", "account", "requested_shares", "submitted_on"], itemPath)
    const event_id = string(entry.event_id, `${itemPath}.event_id`), account = accountId(entry.account, `${itemPath}.account`), requested_shares = decimal(entry.requested_shares, `${itemPath}.requested_shares`), submitted_on = civilDate(entry.submitted_on, `${itemPath}.submitted_on`)
    if (!event_id.trim() || BigInt(requested_shares) <= 0n) throw new SaveSchemaError(itemPath, "配股认购排队身份或数量非法")
    if (!Object.hasOwn(context.snapshot.accounts, account)) throw new SaveSchemaError(`${itemPath}.account`, "认购排队账户不存在")
    if (submitted_on > context.currentDate) throw new SaveSchemaError(`${itemPath}.submitted_on`, "认购提交日期晚于存档日")
    return { event_id, account, requested_shares, submitted_on }
  })
  // 日终公开配售超额认购的显式拒绝回执：受理侧额度校验后的极端竞态兜底，
  // 与 engine `RejectedRightsSubscription` 共享同一序列化形态。
  const rejected_rights_subscriptions: RejectedRightsSubscription[] = array(root.rejected_rights_subscriptions, `${path}.rejected_rights_subscriptions`).map((item, index) => {
    const itemPath = `${path}.rejected_rights_subscriptions[${index}]`, entry = record(item, itemPath)
    exact(entry, ["event_id", "account", "requested_shares", "submitted_on", "rejected_on", "reason"], itemPath)
    const event_id = string(entry.event_id, `${itemPath}.event_id`), account = accountId(entry.account, `${itemPath}.account`), requested_shares = decimal(entry.requested_shares, `${itemPath}.requested_shares`)
    const submitted_on = civilDate(entry.submitted_on, `${itemPath}.submitted_on`), rejected_on = civilDate(entry.rejected_on, `${itemPath}.rejected_on`), reason = string(entry.reason, `${itemPath}.reason`)
    if (!event_id.trim() || BigInt(requested_shares) <= 0n || !reason.trim()) throw new SaveSchemaError(itemPath, "配股认购拒绝回执身份、数量或原因非法")
    if (!Object.hasOwn(context.snapshot.accounts, account)) throw new SaveSchemaError(`${itemPath}.account`, "拒绝回执账户不存在")
    if (rejected_on < submitted_on || rejected_on > context.currentDate) throw new SaveSchemaError(`${itemPath}.rejected_on`, "拒绝回执日期不在提交日与存档日之间")
    return { event_id, account, requested_shares, submitted_on, rejected_on, reason }
  })
  const issuer_repurchases = array(root.issuer_repurchases, `${path}.issuer_repurchases`).map((item, index) => parseIssuerRepurchaseBook(item, `${path}.issuer_repurchases[${index}]`))
  const account_gross_receipts: AccountDividendGrossReceipt[] = array(root.account_gross_receipts, `${path}.account_gross_receipts`).map((item, index) => {
    const itemPath = `${path}.account_gross_receipts[${index}]`, receipt = record(item, itemPath)
    exact(receipt, ["payment_id", "plan_id", "account", "paid_on", "gross", "tax_status"], itemPath)
    const payment_id = string(receipt.payment_id, `${itemPath}.payment_id`), plan_id = string(receipt.plan_id, `${itemPath}.plan_id`), account = accountId(receipt.account, `${itemPath}.account`), paid_on = civilDate(receipt.paid_on, `${itemPath}.paid_on`), gross = money(receipt.gross, `${itemPath}.gross`)
    const tax_status = oneOf(receipt.tax_status, `${itemPath}.tax_status`, ["IndividualPublicMarket", "TreatmentNotConfigured"] as const)
    if (!payment_id.trim() || !plan_id.trim() || BigInt(gross) <= 0n) throw new SaveSchemaError(itemPath, "账户到账身份或金额非法")
    if (paid_on > context.currentDate) throw new SaveSchemaError(`${itemPath}.paid_on`, "实际到账日期晚于存档日")
    if (!Object.hasOwn(context.snapshot.accounts, account)) throw new SaveSchemaError(`${itemPath}.account`, "到账账户不存在")
    return { payment_id, plan_id, account, paid_on, gross, tax_status }
  })
  const external_receipts = array(root.external_receipts, `${path}.external_receipts`).map((item, index) => {
    const itemPath = `${path}.external_receipts[${index}]`, receipt = record(item, itemPath)
    exact(receipt, ["payment_id", "plan_id", "holder", "paid_on", "gross", "tax_status"], itemPath)
    const holder = parseHolder(receipt.holder, `${itemPath}.holder`)
    if (holder.kind !== "External") throw new SaveSchemaError(`${itemPath}.holder`, "外部付款回执必须属于 External 持有人")
    const payment_id = string(receipt.payment_id, `${itemPath}.payment_id`), plan_id = string(receipt.plan_id, `${itemPath}.plan_id`), paid_on = civilDate(receipt.paid_on, `${itemPath}.paid_on`), gross = money(receipt.gross, `${itemPath}.gross`), tax_status = oneOf(receipt.tax_status, `${itemPath}.tax_status`, ["IndividualPublicMarket", "TreatmentNotConfigured"] as const)
    if (!payment_id.trim() || !plan_id.trim() || BigInt(gross) <= 0n) throw new SaveSchemaError(itemPath, "外部收款身份或金额非法")
    if (paid_on > context.currentDate) throw new SaveSchemaError(`${itemPath}.paid_on`, "实际到账日期晚于存档日")
    return { payment_id, plan_id, holder: { External: holder.id }, paid_on, gross, tax_status }
  })
  const applied_ex_reference_groups: AppliedExReferenceGroup[] = array(root.applied_ex_reference_groups, `${path}.applied_ex_reference_groups`).map((item, index) => {
    const itemPath = `${path}.applied_ex_reference_groups[${index}]`, group = record(item, itemPath)
    exact(group, ["date", "stock", "cash_plan_ids", "stock_event_ids", "rights_event_ids", "split_event_ids", "reference"], itemPath)
    const date = civilDate(group.date, `${itemPath}.date`), stock = string(group.stock, `${itemPath}.stock`)
    const cash_plan_ids = array(group.cash_plan_ids, `${itemPath}.cash_plan_ids`).map((id, planIndex) => string(id, `${itemPath}.cash_plan_ids[${planIndex}]`))
    const stock_event_ids = array(group.stock_event_ids, `${itemPath}.stock_event_ids`).map((id, eventIndex) => string(id, `${itemPath}.stock_event_ids[${eventIndex}]`))
    const rights_event_ids = array(group.rights_event_ids, `${itemPath}.rights_event_ids`).map((id, eventIndex) => string(id, `${itemPath}.rights_event_ids[${eventIndex}]`))
    const split_event_ids = array(group.split_event_ids, `${itemPath}.split_event_ids`).map((id, eventIndex) => string(id, `${itemPath}.split_event_ids[${eventIndex}]`))
    const reference = record(group.reference, `${itemPath}.reference`)
    exact(reference, ["ex_date", "reference_price"], `${itemPath}.reference`)
    const ex_date = civilDate(reference.ex_date, `${itemPath}.reference.ex_date`), reference_price = money(reference.reference_price, `${itemPath}.reference.reference_price`)
    if (date > context.currentDate || ex_date !== date || !stock.trim() || BigInt(reference_price) <= 0n || (cash_plan_ids.length === 0 && stock_event_ids.length === 0 && rights_event_ids.length === 0 && split_event_ids.length === 0) || new Set(cash_plan_ids).size !== cash_plan_ids.length || new Set(stock_event_ids).size !== stock_event_ids.length || new Set(rights_event_ids).size !== rights_event_ids.length || new Set(split_event_ids).size !== split_event_ids.length) throw new SaveSchemaError(itemPath, "已应用除权除息组事实非法")
    return { date, stock, cash_plan_ids, stock_event_ids, rights_event_ids, split_event_ids, reference: { ex_date, reference_price } }
  })
  const registryByStock = new Map<string, ReturnType<typeof parseRegistry>>()
  const planIds = new Set<string>()
  for (const [index, registry] of registries.entries()) {
    if (registryByStock.has(registry.stock)) throw new SaveSchemaError(`${path}.registries[${index}].stock`, "证券股东名册重复")
    registryByStock.set(registry.stock, registry)
    const issuer = context.issuers[registry.issuer]
    if (issuer === undefined || issuer.listed_stock !== registry.stock || issuer.issued_shares !== registry.issued_shares) throw new SaveSchemaError(`${path}.registries[${index}]`, "股东名册证券、发行人或发行股数与公司身份不一致")
    const stock = context.setup.stocks.find(item => item.code === registry.stock)
    // 送转等公司行为会真实增加已发行股数；名册当前股数必须等于 setup 初始股数
    // 加上非交易过户增发回执合计，不允许无法解释的差额。
    // 净额 = 非交易过户增发 − 回购注销核减（两周年内发生的公司行为都会留下回执）。
    let nonTradingIssuance = 0n
    for (const receipt of registry.receipts) {
      if (typeof receipt.request.scope !== "object") continue
      for (const change of receipt.request.changes) nonTradingIssuance += BigInt(change.change)
    }
    const expectedIssued = BigInt(stock?.total_shares ?? "0") + nonTradingIssuance
    if (stock === undefined || BigInt(registry.issued_shares) !== expectedIssued || expectedIssued <= 0n) throw new SaveSchemaError(`${path}.registries[${index}]`, "股东名册与 setup 股票身份或初始股数加公司行为净增发不一致")
    if (registry.settled_on > context.currentDate) throw new SaveSchemaError(`${path}.registries[${index}].settled_on`, "股东名册日期晚于存档日")
    for (const [account, accountState] of Object.entries(context.snapshot.accounts)) {
      const position = accountState.positions[registry.stock]?.qty ?? 0
      if (!Number.isSafeInteger(position) || position < 0) throw new SaveSchemaError(`snapshot.accounts.${account}.positions.${registry.stock}.qty`, "账户股份必须是非负安全整数")
      const recorded = registry.holderShares.get(JSON.stringify({ Account: account })) ?? "0"
      if (BigInt(recorded) !== BigInt(position)) throw new SaveSchemaError(`${path}.registries[${index}].holdings`, `账户 ${account} 的持仓与股东名册不一致`)
    }
    for (const holding of registry.holdings) {
      if (typeof holding.holder === "object" && "Account" in holding.holder && !Object.hasOwn(context.snapshot.accounts, holding.holder.Account)) throw new SaveSchemaError(`${path}.registries[${index}].holdings`, "股东名册引用不存在的账户")
    }
  }
  const paymentFacts = new Map<string, { readonly plan_id: string; readonly holder: string; readonly paid_on: string; readonly gross: string; readonly kind: "Account" | "External" }>()
  for (const [index, dividend] of dividends.entries()) {
    const itemPath = `${path}.dividends[${index}]`
    if (planIds.has(dividend.plan.plan_id)) throw new SaveSchemaError(`${itemPath}.plan.plan_id`, "分红方案身份重复")
    planIds.add(dividend.plan.plan_id)
    const registry = registryByStock.get(dividend.plan.stock)
    if (registry === undefined || registry.issuer !== dividend.plan.issuer) throw new SaveSchemaError(`${itemPath}.plan`, "分红方案没有匹配的发行人股东名册")
    const setupStock = context.setup.stocks.find(stock => stock.code === dividend.plan.stock)
    if (setupStock === undefined || calendarExchangeOfSetup(setupStock.exchange) !== dividend.plan.exchange) throw new SaveSchemaError(`${itemPath}.plan.exchange`, "分红方案交易所与证券不一致")
    if (dividend.plan.approved_on > context.currentDate || (dividend.status !== "Approved" && dividend.plan.announced_on > context.currentDate) || (dividend.registration !== null && dividend.plan.registered_on > context.currentDate) || dividend.payments.some(payment => payment.paid_on > context.currentDate)) throw new SaveSchemaError(`${itemPath}.plan`, "已发生分红或付款事实日期晚于存档日")
    if (dividend.registration !== null) {
      const savedSnapshot = registry.registrations.find(snapshot => snapshot.event_id === dividend.plan.plan_id)
      if (savedSnapshot === undefined || JSON.stringify(savedSnapshot) !== JSON.stringify(dividend.registration)) throw new SaveSchemaError(`${itemPath}.registration`, "分红冻结快照与股东名册对应登记事实不一致")
    }
    for (const payment of dividend.payments) for (const outcome of payment.outcomes) {
      if ("Paid" in outcome) {
        const holder = outcome.Paid.holder
        const key = JSON.stringify(holder)
        if (typeof holder === "object" && ("External" in holder || "Account" in holder)) {
          const paymentKey = `${payment.payment_id}\0${dividend.plan.plan_id}\0${key}`
          if (paymentFacts.has(paymentKey)) throw new SaveSchemaError(`${itemPath}.payments`, "同一持有人支付事实重复")
          paymentFacts.set(paymentKey, { plan_id: dividend.plan.plan_id, holder: key, paid_on: payment.paid_on, gross: outcome.Paid.amount, kind: "Account" in holder ? "Account" : "External" })
        }
      }
    }
  }
  const accountReceiptIdentityKeys = new Set<string>()
  const accountPaymentFactKeys = new Set<string>()
  for (const [index, receipt] of account_gross_receipts.entries()) {
    const itemPath = `${path}.account_gross_receipts[${index}]`, identityKey = `${receipt.payment_id}\0${JSON.stringify({ Account: receipt.account })}`, factKey = `${receipt.payment_id}\0${receipt.plan_id}\0${JSON.stringify({ Account: receipt.account })}`
    if (accountReceiptIdentityKeys.has(identityKey)) throw new SaveSchemaError(itemPath, "账户到账身份重复")
    accountReceiptIdentityKeys.add(identityKey)
    accountPaymentFactKeys.add(factKey)
    const fact = paymentFacts.get(factKey)
    if (fact === undefined || fact.kind !== "Account" || receipt.plan_id !== fact.plan_id || receipt.paid_on !== fact.paid_on || receipt.gross !== fact.gross) throw new SaveSchemaError(itemPath, "账户到账回执与分红支付成功事实不一致")
  }
  const externalIdentityKeys = new Set<string>()
  const externalPaymentFactKeys = new Set<string>()
  for (const [index, receipt] of external_receipts.entries()) {
    const itemPath = `${path}.external_receipts[${index}]`, key = `${receipt.payment_id}\0${receipt.plan_id}\0${JSON.stringify(receipt.holder)}`, identityKey = `${receipt.payment_id}\0${JSON.stringify(receipt.holder)}`
    if (!planIds.has(receipt.plan_id)) throw new SaveSchemaError(`${itemPath}.plan_id`, "外部付款引用的分红方案不存在")
    if (externalIdentityKeys.has(identityKey)) throw new SaveSchemaError(itemPath, "外部付款身份重复")
    externalIdentityKeys.add(identityKey)
    externalPaymentFactKeys.add(key)
    const fact = paymentFacts.get(key)
    if (fact === undefined || fact.kind !== "External" || receipt.plan_id !== fact.plan_id || JSON.stringify(receipt.holder) !== fact.holder || receipt.paid_on !== fact.paid_on || receipt.gross !== fact.gross) throw new SaveSchemaError(itemPath, "外部付款回执与分红支付成功事实不一致")
  }
  for (const [key, fact] of paymentFacts) {
    if (fact.kind === "Account" && !accountPaymentFactKeys.has(key)) throw new SaveSchemaError(`${path}.account_gross_receipts`, "缺少已到账账户回执")
    if (fact.kind === "External" && !externalPaymentFactKeys.has(key)) throw new SaveSchemaError(`${path}.external_receipts`, "缺少已支付 External 持有人回执")
  }
  // 认购排队与回购账簿的引用勾稽：排队必须指向存在的配股事件（正负例由
  // parseSessionCorporateActions 的严格字段校验与 engine 恢复校验共同承担）。
  const rightsEventIds = new Set(rights_offerings.map(book => book.plan.event_id))
  for (const [index, queued] of rights_subscription_queue.entries()) {
    if (!rightsEventIds.has(queued.event_id)) throw new SaveSchemaError(`${path}.rights_subscription_queue[${index}].event_id`, "认购排队引用不存在的配股事件")
  }
  // 拒绝回执勾稽：事件存在、同一 (event, account) 唯一、不得与排队或已入账
  // 认购并存（净认购唯一性：一个持有人要么入账、要么在队、要么被拒）。
  const rejectedKeys = new Set<string>()
  for (const [index, receipt] of rejected_rights_subscriptions.entries()) {
    const itemPath = `${path}.rejected_rights_subscriptions[${index}]`
    if (!rightsEventIds.has(receipt.event_id)) throw new SaveSchemaError(`${itemPath}.event_id`, "拒绝回执引用不存在的配股事件")
    const key = `${receipt.event_id}\0${receipt.account}`
    if (rejectedKeys.has(key)) throw new SaveSchemaError(itemPath, "同一配股事件同一账户存在重复拒绝回执")
    rejectedKeys.add(key)
    if (rights_subscription_queue.some(queued => queued.event_id === receipt.event_id && queued.account === receipt.account)) throw new SaveSchemaError(itemPath, "拒绝回执与仍在排队的认购并存")
    const book = rights_offerings.find(book => book.plan.event_id === receipt.event_id)
    if (book?.subscriptions.some(record => typeof record.holder === "object" && "Account" in record.holder && record.holder.Account === receipt.account)) throw new SaveSchemaError(itemPath, "拒绝回执与已入账认购并存")
  }
  for (const [index, book] of issuer_repurchases.entries()) {
    const registry = registryByStock.get(book.plan.stock)
    if (registry === undefined || registry.issuer !== book.plan.issuer) throw new SaveSchemaError(`${path}.issuer_repurchases[${index}].plan`, "回购方案没有匹配的发行人股东名册")
    if (book.plan.approved_on > context.currentDate) throw new SaveSchemaError(`${path}.issuer_repurchases[${index}].plan.approved_on`, "回购批准日期晚于存档日")
  }
  const groupKeys = new Set<string>()
  for (const [index, group] of applied_ex_reference_groups.entries()) {
    const key = `${group.date}\0${group.stock}`
    if (groupKeys.has(key)) throw new SaveSchemaError(`${path}.applied_ex_reference_groups[${index}]`, "同一证券日期存在重复除权除息组")
    groupKeys.add(key)
    const setupStock = context.setup.stocks.find(stock => stock.code === group.stock)
    if (setupStock === undefined) throw new SaveSchemaError(`${path}.applied_ex_reference_groups[${index}].stock`, "除权除息组引用 setup 中不存在的证券")
    if (BigInt(setupStock.tick) <= 0n || BigInt(group.reference.reference_price) % BigInt(setupStock.tick) !== 0n) throw new SaveSchemaError(`${path}.applied_ex_reference_groups[${index}].reference.reference_price`, "除权除息组参考价不符合证券最小价位单位")
    const anchor = context.snapshot.markets[group.stock]?.last_cash_ex_reference
    if (anchor?.ex_date === group.date && anchor.reference_price !== group.reference.reference_price) throw new SaveSchemaError(`${path}.applied_ex_reference_groups[${index}].reference`, "已应用除权除息组与 MarketSnap 锚点不一致")
    const expectedPlanIds = dividends.filter(dividend => dividend.plan.ex_dividend_on === group.date && dividend.plan.stock === group.stock && dividend.registration !== null).map(dividend => dividend.plan.plan_id).sort()
    const expectedEventIds = stock_distributions.filter(book => book.plan.ex_rights_on === group.date && book.plan.stock === group.stock && book.registration !== null).map(book => book.plan.event_id).sort()
    const expectedSplitIds = share_splits.filter(book => book.plan.ex_rights_on === group.date && book.plan.stock === group.stock && book.registration !== null).map(book => book.plan.event_id).sort()
    if (JSON.stringify(expectedPlanIds) !== JSON.stringify([...group.cash_plan_ids].sort()) || JSON.stringify(expectedEventIds) !== JSON.stringify([...group.stock_event_ids].sort()) || JSON.stringify(expectedSplitIds) !== JSON.stringify([...group.split_event_ids].sort())) throw new SaveSchemaError(`${path}.applied_ex_reference_groups[${index}]`, "已应用除权除息组与登记分红方案、送转或拆股／缩股事件不一致")
  }
  const latestGroupByStock = new Map<string, AppliedExReferenceGroup>()
  for (const group of applied_ex_reference_groups) {
    const latest = latestGroupByStock.get(group.stock)
    if (latest === undefined || latest.date < group.date) latestGroupByStock.set(group.stock, group)
  }
  for (const [stock, latest] of latestGroupByStock) {
    const anchor = context.snapshot.markets[stock]?.last_cash_ex_reference
    if (anchor === null || anchor === undefined || anchor.ex_date !== latest.date || anchor.reference_price !== latest.reference.reference_price) throw new SaveSchemaError(`${path}.applied_ex_reference_groups`, `证券 ${stock} 的 MarketSnap 锚点必须匹配最新已应用除权除息组`)
  }
  for (const [stock, market] of Object.entries(context.snapshot.markets)) {
    const anchor = market.last_cash_ex_reference
    if (anchor !== null && !groupKeys.has(`${anchor.ex_date}\0${stock}`)) throw new SaveSchemaError(`${path}.applied_ex_reference_groups`, `MarketSnap ${stock} 的除权除息锚点缺少对应已应用组`)
  }
  const distributionEventIds = new Set<string>()
  for (const [index, distribution] of stock_distributions.entries()) {
    const itemPath = `${path}.stock_distributions[${index}]`
    if (distributionEventIds.has(distribution.plan.event_id)) throw new SaveSchemaError(`${itemPath}.plan.event_id`, "送转事件身份重复")
    distributionEventIds.add(distribution.plan.event_id)
    const registry = registryByStock.get(distribution.plan.stock)
    if (registry === undefined || registry.issuer !== distribution.plan.issuer) throw new SaveSchemaError(`${itemPath}.plan`, "送转方案没有匹配的发行人股东名册")
    const setupStock = context.setup.stocks.find(stock => stock.code === distribution.plan.stock)
    if (setupStock === undefined || calendarExchangeOfSetup(setupStock.exchange) !== distribution.plan.exchange) throw new SaveSchemaError(`${itemPath}.plan.exchange`, "送转方案交易所与证券不一致")
    if (distribution.plan.approved_on > context.currentDate) throw new SaveSchemaError(`${itemPath}.plan.approved_on`, "送转批准日期晚于存档日")
    if (distribution.registration !== null) {
      const savedSnapshot = registry.registrations.find(snapshot => snapshot.event_id === distribution.plan.event_id)
      if (savedSnapshot === undefined || JSON.stringify(savedSnapshot) !== JSON.stringify(distribution.registration)) throw new SaveSchemaError(`${itemPath}.registration`, "送转冻结快照与股东名册对应登记事实不一致")
    }
    if (distribution.credited_on !== null) {
      const expectedEvent = `stock-distribution:${distribution.plan.event_id}`
      const receipt = registry.receipts.find(receipt => receipt.request.event_id === expectedEvent)
      let issued = 0n
      if (receipt !== undefined) {
        if (receipt.request.day !== distribution.credited_on || typeof receipt.request.scope !== "object") throw new SaveSchemaError(itemPath, "送转入账与非交易过户回执日期或范围不一致")
        for (const change of receipt.request.changes) issued += BigInt(change.change)
      }
      if (issued !== BigInt(distribution.plan.approved_total_new_shares)) throw new SaveSchemaError(itemPath, "送转入账股数与非交易过户增发合计不一致")
    }
  }
  // 拆股／缩股账簿勾稽：身份唯一、名册匹配、交易所一致、冻结快照与名册登记
  // 事实一致、入账回执（ShareReDenomination）的日期与净额与换算回执一致；
  // 反向：名册中每条重新计值回执必须映射到已入账账簿。
  const splitEventIds = new Set<string>()
  for (const [index, split] of share_splits.entries()) {
    const itemPath = `${path}.share_splits[${index}]`
    if (splitEventIds.has(split.plan.event_id)) throw new SaveSchemaError(`${itemPath}.plan.event_id`, "拆股／缩股事件身份重复")
    splitEventIds.add(split.plan.event_id)
    const registry = registryByStock.get(split.plan.stock)
    if (registry === undefined || registry.issuer !== split.plan.issuer) throw new SaveSchemaError(`${itemPath}.plan`, "拆股／缩股方案没有匹配的发行人股东名册")
    const setupStock = context.setup.stocks.find(stock => stock.code === split.plan.stock)
    if (setupStock === undefined || calendarExchangeOfSetup(setupStock.exchange) !== split.plan.exchange) throw new SaveSchemaError(`${itemPath}.plan.exchange`, "拆股／缩股方案交易所与证券不一致")
    if (split.plan.approved_on > context.currentDate) throw new SaveSchemaError(`${itemPath}.plan.approved_on`, "拆股／缩股批准日期晚于存档日")
    if (split.registration !== null) {
      const savedSnapshot = registry.registrations.find(snapshot => snapshot.event_id === split.plan.event_id)
      if (savedSnapshot === undefined || JSON.stringify(savedSnapshot) !== JSON.stringify(split.registration)) throw new SaveSchemaError(`${itemPath}.registration`, "拆股／缩股冻结快照与股东名册对应登记事实不一致")
    }
    if (split.settled_on !== null) {
      const expectedEvent = `share-split:${split.plan.event_id}`
      const receipt = registry.receipts.find(receipt => receipt.request.event_id === expectedEvent)
      if (receipt === undefined) throw new SaveSchemaError(itemPath, "拆股／缩股入账缺少重新计值回执")
      const scope = receipt.request.scope
      if (receipt.request.day !== split.settled_on || typeof scope !== "object" || !Object.hasOwn(scope, "ShareReDenomination")) throw new SaveSchemaError(itemPath, "拆股／缩股入账与重新计值回执日期或范围不一致")
      let net = 0n
      for (const change of receipt.request.changes) net += BigInt(change.change)
      const expectedNet = BigInt(split.receipt?.issued_shares_after ?? "0") - BigInt(split.receipt?.issued_shares_before ?? "0")
      if (net !== expectedNet) throw new SaveSchemaError(itemPath, "重新计值回执净额与换算回执前后总股数不一致")
    }
  }
  for (const registry of registryByStock.values()) {
    for (const receipt of registry.receipts) {
      const scope = receipt.request.scope
      if (typeof scope !== "object" || !Object.hasOwn(scope, "ShareReDenomination")) continue
      const split = share_splits.find(split => `share-split:${split.plan.event_id}` === receipt.request.event_id)
      if (split === undefined || split.settled_on === null || split.settled_on !== receipt.request.day) throw new SaveSchemaError(`${path}.share_splits`, "名册存在无对应已入账账簿的重新计值回执")
    }
  }
  const taxBookKeys = new Set<string>()
  const configuredTaxAccounts = new Set<string>()
  for (const [index, taxBook] of dividend_tax_books.entries()) {
    const itemPath = `${path}.dividend_tax_books[${index}]`
    const key = `${taxBook.account}\0${taxBook.stock}`
    if (taxBookKeys.has(key)) throw new SaveSchemaError(itemPath, "同一账户证券存在重复股息税账")
    taxBookKeys.add(key)
    configuredTaxAccounts.add(`${taxBook.account}\0${taxBook.stock}`)
    if (!Object.hasOwn(context.snapshot.accounts, taxBook.account)) throw new SaveSchemaError(`${itemPath}.account`, "股息税账引用不存在的账户")
    const registry = registryByStock.get(taxBook.stock)
    if (registry === undefined || !registry.holdings.some(holding => typeof holding.holder === "object" && "Account" in holding.holder && holding.holder.Account === taxBook.account)) throw new SaveSchemaError(itemPath, "股息税账缺少对应股东名册持有人")
    if (taxBook.settled_on !== registry.settled_on) throw new SaveSchemaError(`${itemPath}.settled_on`, "股息税账结算日与股东名册不一致")
    // 税账↔名册回执覆盖勾稽（与 engine `SessionCorporateActions::validate` 的
    // 「送转×税账交互未入账」检查同构）：名册每个涉及该账户变动的日结回执都必须
    // 已按 scope 派生的事件 id 进入税账日结。缺失说明同步被静默跳过或存档被篡改，
    // 恢复时显式失败而不是让税基悄悄缺股。
    for (const receipt of registry.receipts) {
      const accountKey = JSON.stringify({ Account: taxBook.account })
      if (!receipt.request.changes.some(change => JSON.stringify(change.holder) === accountKey)) continue
      // 事件 id 按 scope 分列：公开市场日结沿用「证券+账户+自然日」；非交易过户
      // （送转到账）以回执自身事件身份派生 `{event_id}:{account}`；回购注销只核减
      // IssuerTreasury、不含 Account 持有人分录，与引擎侧一致显式跳过。
      // 穷尽分支 + never 断言：未来给 MovementScope 新增变体时在此编译期强制
      // 显式决策（同引擎 match），不允许 Web 侧静默跳过。
      const scope = receipt.request.scope
      if (scope === "PublicMarket") {
        const expectedEventId = `session-market:${registry.stock}:${taxBook.account}:${receipt.request.day}`
        if (!taxBook.days.some(day => day.event_id === expectedEventId)) {
          throw new SaveSchemaError(itemPath, `账户 ${taxBook.account} 的股息税账缺少名册回执 ${receipt.request.event_id} 的日结事实：送转×税账交互未入账，拒绝静默缺股`)
        }
      } else if ("NonTradingTransfer" in scope) {
        const expectedEventId = `${receipt.request.event_id}:${taxBook.account}`
        if (!taxBook.days.some(day => day.event_id === expectedEventId)) {
          throw new SaveSchemaError(itemPath, `账户 ${taxBook.account} 的股息税账缺少名册回执 ${receipt.request.event_id} 的日结事实：送转×税账交互未入账，拒绝静默缺股`)
        }
      } else if ("ShareReDenomination" in scope) {
        // 拆股／缩股重新计值：与引擎分支同构以 `{event_id}:{account}` 派生；拆股
        // 正向增量走日结续记、缩股核减走 redenominations 回执（非应税、不进处置
        // 口径），二者任一即构成覆盖，缺失仍显式拒绝。
        const expectedEventId = `${receipt.request.event_id}:${taxBook.account}`
        if (!taxBook.days.some(day => day.event_id === expectedEventId) && !taxBook.redenominations.some(reduction => reduction.event_id === expectedEventId)) {
          throw new SaveSchemaError(itemPath, `账户 ${taxBook.account} 的股息税账缺少名册回执 ${receipt.request.event_id} 的重新计值事实：拆股／缩股×税账交互未入账，拒绝静默缺股`)
        }
      } else if ("IssuerRepurchaseCancellation" in scope) {
        continue
      } else {
        const exhaustive: never = scope
        throw new SaveSchemaError(itemPath, `未知名册回执 scope 变体：${String(exhaustive)}`)
      }
    }
  }
  for (const [index, receipt] of account_gross_receipts.entries()) {
    const plan = dividends.find(dividend => dividend.plan.plan_id === receipt.plan_id)
    const expectedStatus = plan !== undefined && configuredTaxAccounts.has(`${receipt.account}\0${plan.plan.stock}`) ? "IndividualPublicMarket" : "TreatmentNotConfigured"
    if (receipt.tax_status !== expectedStatus) throw new SaveSchemaError(`${path}.account_gross_receipts[${index}].tax_status`, "账户分红税身份与股息税账配置不一致")
  }
  return { registries: registries.map(({ holderShares: _shares, ...registry }) => registry), dividends, dividend_tax_books, stock_distributions, share_splits, rights_offerings, rights_subscription_queue, rejected_rights_subscriptions, issuer_repurchases, account_gross_receipts, external_receipts, applied_ex_reference_groups }
}

function holderWire(holder: ReturnType<typeof parseHolder>): HolderId {
  if (holder.kind === "IssuerTreasury") return "IssuerTreasury"
  if (holder.kind === "External") return { External: holder.id }
  return { Account: holder.id }
}

function parseRightsOfferingMode(value: unknown, path: string): RightsOfferingMode {
  const tagged = record(value, path)
  if (Object.keys(tagged).length !== 1) throw new SaveSchemaError(path, "配股模式必须是单一枚举变体")
  const variant = Object.keys(tagged)[0]!
  if (variant === "RightsToAllShareholders") {
    const detail = record(tagged[variant], `${path}.${variant}`)
    exact(detail, ["shares_per_existing_share_micros"], `${path}.${variant}`)
    const shares_per_existing_share_micros = decimal(detail.shares_per_existing_share_micros, `${path}.${variant}.shares_per_existing_share_micros`)
    if (BigInt(shares_per_existing_share_micros) <= 0n || BigInt(shares_per_existing_share_micros) > 500_000n) throw new SaveSchemaError(`${path}.${variant}.shares_per_existing_share_micros`, "配股比例必须为正且不超过 50%（206号令第 53 条）")
    return { RightsToAllShareholders: { shares_per_existing_share_micros } }
  }
  if (variant === "DirectedPlacement") {
    const detail = record(tagged[variant], `${path}.${variant}`)
    exact(detail, ["targets"], `${path}.${variant}`)
    const targets = array(detail.targets, `${path}.${variant}.targets`).map((item, index) => {
      const targetPath = `${path}.${variant}.targets[${index}]`, target = record(item, targetPath)
      if (Object.keys(target).length !== 1) throw new SaveSchemaError(targetPath, "定向对象必须是单一枚举变体")
      const targetVariant = Object.keys(target)[0]!
      if (targetVariant === "NamedHolder") {
        const named = record(target[targetVariant], `${targetPath}.NamedHolder`)
        exact(named, ["holder", "shares", "lock_until"], `${targetPath}.NamedHolder`)
        const holder = parseHolder(named.holder, `${targetPath}.NamedHolder.holder`)
        const shares = decimal(named.shares, `${targetPath}.NamedHolder.shares`)
        const lock_until = named.lock_until === null ? null : civilDate(named.lock_until, `${targetPath}.NamedHolder.lock_until`)
        if (BigInt(shares) <= 0n) throw new SaveSchemaError(`${targetPath}.NamedHolder.shares`, "定向股数必须为正")
        return { NamedHolder: { holder: holderWire(holder), shares, lock_until } }
      }
      if (targetVariant === "OpenPublicSubscription") {
        const open = record(target[targetVariant], `${targetPath}.OpenPublicSubscription`)
        exact(open, ["shares"], `${targetPath}.OpenPublicSubscription`)
        const shares = decimal(open.shares, `${targetPath}.OpenPublicSubscription.shares`)
        if (BigInt(shares) <= 0n) throw new SaveSchemaError(`${targetPath}.OpenPublicSubscription.shares`, "公开配售额度必须为正")
        return { OpenPublicSubscription: { shares } }
      }
      throw new SaveSchemaError(targetPath, "包含无效定向对象变体")
    })
    if (targets.length === 0) throw new SaveSchemaError(`${path}.${variant}.targets`, "定向对象清单不能为空")
    return { DirectedPlacement: { targets } }
  }
  throw new SaveSchemaError(path, "包含无效配股模式变体")
}

export function parseRightsOfferingEventPlanValue(value: unknown, path: string): RightsOfferingEventPlan {
  const plan = record(value, path)
  exact(plan, ["event_id", "approval_reference", "issuer", "stock", "exchange", "approved_on", "announced_on", "registered_on", "payment_start_on", "payment_deadline_on", "ex_rights_on", "settlement_on", "listing_on", "price_per_share", "mode", "npc_subscription_strategy"], path)
  const event_id = string(plan.event_id, `${path}.event_id`), approval_reference = string(plan.approval_reference, `${path}.approval_reference`)
  const issuer = string(plan.issuer, `${path}.issuer`), stock = string(plan.stock, `${path}.stock`)
  const exchange = oneOf(plan.exchange, `${path}.exchange`, ["sse", "szse"] as const)
  const approved_on = civilDate(plan.approved_on, `${path}.approved_on`), announced_on = civilDate(plan.announced_on, `${path}.announced_on`)
  const registered_on = civilDate(plan.registered_on, `${path}.registered_on`), payment_start_on = civilDate(plan.payment_start_on, `${path}.payment_start_on`)
  const payment_deadline_on = civilDate(plan.payment_deadline_on, `${path}.payment_deadline_on`), ex_rights_on = civilDate(plan.ex_rights_on, `${path}.ex_rights_on`)
  const settlement_on = civilDate(plan.settlement_on, `${path}.settlement_on`), listing_on = civilDate(plan.listing_on, `${path}.listing_on`)
  const price_per_share = money(plan.price_per_share, `${path}.price_per_share`)
  const mode = parseRightsOfferingMode(plan.mode, `${path}.mode`)
  const npc_subscription_strategy = oneOf(plan.npc_subscription_strategy, `${path}.npc_subscription_strategy`, ["FullByDefault", "StrategyBased"] as const)
  if (!event_id.trim() || !approval_reference.trim() || !issuer.trim() || !stock.trim() || BigInt(price_per_share) <= 0n) throw new SaveSchemaError(path, "配股方案身份或价格非法")
  if (!(approved_on <= announced_on && announced_on <= registered_on && registered_on < payment_start_on && payment_start_on <= payment_deadline_on && payment_deadline_on < ex_rights_on && ex_rights_on < settlement_on && settlement_on === listing_on)) throw new SaveSchemaError(path, "配股日程日期顺序非法")
  return { event_id, approval_reference, issuer, stock, exchange, approved_on, announced_on, registered_on, payment_start_on, payment_deadline_on, ex_rights_on, settlement_on, listing_on, price_per_share, mode, npc_subscription_strategy }
}

export function parseIssuerRepurchasePlanValue(value: unknown, path: string): IssuerRepurchasePlan {
  const plan = record(value, path)
  exact(plan, ["event_id", "approval_reference", "issuer", "stock", "exchange", "approved_on", "announced_on", "window_start_on", "window_deadline_on", "price_cap_per_share", "total_budget", "max_shares", "purpose"], path)
  const event_id = string(plan.event_id, `${path}.event_id`), approval_reference = string(plan.approval_reference, `${path}.approval_reference`)
  const issuer = string(plan.issuer, `${path}.issuer`), stock = string(plan.stock, `${path}.stock`)
  const exchange = oneOf(plan.exchange, `${path}.exchange`, ["sse", "szse"] as const)
  const approved_on = civilDate(plan.approved_on, `${path}.approved_on`), announced_on = civilDate(plan.announced_on, `${path}.announced_on`)
  const window_start_on = civilDate(plan.window_start_on, `${path}.window_start_on`), window_deadline_on = civilDate(plan.window_deadline_on, `${path}.window_deadline_on`)
  const price_cap_per_share = money(plan.price_cap_per_share, `${path}.price_cap_per_share`), total_budget = money(plan.total_budget, `${path}.total_budget`)
  const max_shares = decimal(plan.max_shares, `${path}.max_shares`)
  const purpose = oneOf(plan.purpose, `${path}.purpose`, ["ReduceCapital", "EmployeeIncentive", "ConvertibleConversion", "ValueMaintenance"] as const)
  if (!event_id.trim() || !approval_reference.trim() || !issuer.trim() || !stock.trim() || BigInt(price_cap_per_share) <= 0n || BigInt(total_budget) <= 0n || BigInt(max_shares) <= 0n) throw new SaveSchemaError(path, "回购方案身份、价格或额度非法")
  if (!(approved_on <= announced_on && announced_on <= window_start_on && window_start_on <= window_deadline_on)) throw new SaveSchemaError(path, "回购日程日期顺序非法")
  return { event_id, approval_reference, issuer, stock, exchange, approved_on, announced_on, window_start_on, window_deadline_on, price_cap_per_share, total_budget, max_shares, purpose }
}

function parseRightsOfferingBook(value: unknown, path: string): RightsOfferingBook {
  const book = record(value, path)
  exact(book, ["plan", "status", "registration", "entitlement", "subscriptions", "closed_on", "settlement", "credited_on"], path)
  const parsedPlan = parseRightsOfferingEventPlanValue(book.plan, `${path}.plan`)
  const { registered_on, payment_start_on, payment_deadline_on, settlement_on, listing_on } = parsedPlan
  const status = oneOf(book.status, `${path}.status`, ["Approved", "Announced", "Entitled", "Closed", "Settled"] as const)
  const registration = book.registration === null ? null : parseSnapshot(book.registration, `${path}.registration`)
  const entitlement = book.entitlement === null ? null : ((): RightsEntitlementReceipt => {
    const itemPath = `${path}.entitlement`, receipt = record(book.entitlement, itemPath)
    exact(receipt, ["event_id", "approval_reference", "registration_event_id", "stock", "issuer", "registered_on", "issued_shares_before", "issuer_treasury_shares_excluded", "price_per_share", "mode", "planned_total_rights_shares", "tie_break_seed", "entitlements", "open_subscription_shares"], itemPath)
    const entitlements = array(receipt.entitlements, `${itemPath}.entitlements`).map((item, index) => {
      const rowPath = `${itemPath}.entitlements[${index}]`, entry = record(item, rowPath)
      exact(entry, ["holder", "rights_shares", "lock_until"], rowPath)
      const holder = parseHolder(entry.holder, `${rowPath}.holder`)
      const rights_shares = decimal(entry.rights_shares, `${rowPath}.rights_shares`)
      const lock_until = entry.lock_until === null ? null : civilDate(entry.lock_until, `${rowPath}.lock_until`)
      if (BigInt(rights_shares) <= 0n) throw new SaveSchemaError(`${rowPath}.rights_shares`, "权利股数必须为正")
      return { holder: holderWire(holder), rights_shares, lock_until }
    })
    return { event_id: string(receipt.event_id, `${itemPath}.event_id`), approval_reference: string(receipt.approval_reference, `${itemPath}.approval_reference`), registration_event_id: string(receipt.registration_event_id, `${itemPath}.registration_event_id`), stock: string(receipt.stock, `${itemPath}.stock`), issuer: string(receipt.issuer, `${itemPath}.issuer`), registered_on: civilDate(receipt.registered_on, `${itemPath}.registered_on`), issued_shares_before: decimal(receipt.issued_shares_before, `${itemPath}.issued_shares_before`), issuer_treasury_shares_excluded: decimal(receipt.issuer_treasury_shares_excluded, `${itemPath}.issuer_treasury_shares_excluded`), price_per_share: money(receipt.price_per_share, `${itemPath}.price_per_share`), mode: parseRightsOfferingMode(receipt.mode, `${itemPath}.mode`), planned_total_rights_shares: decimal(receipt.planned_total_rights_shares, `${itemPath}.planned_total_rights_shares`), tie_break_seed: decimal(receipt.tie_break_seed, `${itemPath}.tie_break_seed`), entitlements, open_subscription_shares: decimal(receipt.open_subscription_shares, `${itemPath}.open_subscription_shares`) }
  })()
  const subscriptions = array(book.subscriptions, `${path}.subscriptions`).map((item, index) => {
    const itemPath = `${path}.subscriptions[${index}]`, row = record(item, itemPath)
    exact(row, ["holder", "requested_shares", "price_per_share", "submitted_on", "origin", "paid_shares", "paid_amount", "waived_shares"], itemPath)
    const holder = parseHolder(row.holder, `${itemPath}.holder`)
    const requested_shares = decimal(row.requested_shares, `${itemPath}.requested_shares`)
    const rowPrice = money(row.price_per_share, `${itemPath}.price_per_share`)
    const submitted_on = civilDate(row.submitted_on, `${itemPath}.submitted_on`)
    const origin = oneOf(row.origin, `${itemPath}.origin`, ["Explicit", "NpcFullByDefault"] as const)
    const paid_shares = decimal(row.paid_shares, `${itemPath}.paid_shares`)
    const paid_amount = money(row.paid_amount, `${itemPath}.paid_amount`)
    const waived_shares = decimal(row.waived_shares, `${itemPath}.waived_shares`)
    if (BigInt(requested_shares) <= 0n || BigInt(paid_shares) > BigInt(requested_shares) || BigInt(paid_shares) + BigInt(waived_shares) !== BigInt(requested_shares) || BigInt(paid_shares) * BigInt(rowPrice) !== BigInt(paid_amount)) throw new SaveSchemaError(itemPath, "认购记录数量或金额勾稽不一致")
    if (submitted_on < payment_start_on || submitted_on > payment_deadline_on) throw new SaveSchemaError(`${itemPath}.submitted_on`, "认购提交日期不在缴款期内")
    return { holder: holderWire(holder), requested_shares, price_per_share: rowPrice, submitted_on, origin, paid_shares, paid_amount, waived_shares }
  })
  const closed_on = book.closed_on === null ? null : civilDate(book.closed_on, `${path}.closed_on`)
  const settlement = book.settlement === null ? null : ((): RightsSettlementReceipt => {
    const itemPath = `${path}.settlement`, receipt = record(book.settlement, itemPath)
    exact(receipt, ["event_id", "settlement_on", "failed", "total_paid_shares", "total_paid_amount", "refunded_total", "holders"], itemPath)
    const holders = array(receipt.holders, `${itemPath}.holders`).map((item, index) => {
      const rowPath = `${itemPath}.holders[${index}]`, row = record(item, rowPath)
      exact(row, ["holder", "paid_shares", "paid_amount", "waived_shares", "refunded_amount"], rowPath)
      const holder = parseHolder(row.holder, `${rowPath}.holder`)
      return { holder: holderWire(holder), paid_shares: decimal(row.paid_shares, `${rowPath}.paid_shares`), paid_amount: money(row.paid_amount, `${rowPath}.paid_amount`), waived_shares: decimal(row.waived_shares, `${rowPath}.waived_shares`), refunded_amount: money(row.refunded_amount, `${rowPath}.refunded_amount`) }
    })
    return { event_id: string(receipt.event_id, `${itemPath}.event_id`), settlement_on: civilDate(receipt.settlement_on, `${itemPath}.settlement_on`), failed: boolean(receipt.failed, `${itemPath}.failed`), total_paid_shares: decimal(receipt.total_paid_shares, `${itemPath}.total_paid_shares`), total_paid_amount: money(receipt.total_paid_amount, `${itemPath}.total_paid_amount`), refunded_total: money(receipt.refunded_total, `${itemPath}.refunded_total`), holders }
  })()
  const credited_on = book.credited_on === null ? null : civilDate(book.credited_on, `${path}.credited_on`)
  const entitled = entitlement !== null
  if (entitled !== (status === "Entitled" || status === "Closed" || status === "Settled")) throw new SaveSchemaError(`${path}.status`, "配股状态与权证事实不一致")
  if (entitled !== (registration !== null)) throw new SaveSchemaError(`${path}.registration`, "配股登记快照与权证事实不一致")
  if (subscriptions.length > 0 && !entitled) throw new SaveSchemaError(`${path}.subscriptions`, "认购记录要求已派发权证")
  if (credited_on !== null && (settlement === null || settlement.failed || credited_on !== listing_on)) throw new SaveSchemaError(`${path}.credited_on`, "入账事实非法")
  if (settlement !== null && (status !== "Settled" || settlement.settlement_on !== settlement_on)) throw new SaveSchemaError(`${path}.settlement`, "结算事实与状态或日期不一致")
  if (closed_on !== null && !(status === "Closed" || status === "Settled")) throw new SaveSchemaError(`${path}.closed_on`, "关窗事实与状态不一致")
  void registered_on; void payment_start_on; void payment_deadline_on; void settlement_on; void listing_on
  return { plan: parsedPlan, status, registration, entitlement, subscriptions, closed_on, settlement, credited_on }
}

function parseIssuerRepurchaseBook(value: unknown, path: string): IssuerRepurchaseBook {
  const book = record(value, path)
  exact(book, ["plan", "status", "fills", "completed_on", "withdrawn_remainder", "cancelled_on", "cancelled_shares", "last_order_day"], path)
  const parsedPlan = parseIssuerRepurchasePlanValue(book.plan, `${path}.plan`)
  const { stock, window_start_on, window_deadline_on, total_budget, max_shares } = parsedPlan
  const status = oneOf(book.status, `${path}.status`, ["Approved", "Announced", "Executing", "Completed", "Cancelled"] as const)
  const fills = array(book.fills, `${path}.fills`).map((item, index) => {
    const itemPath = `${path}.fills[${index}]`, fill = record(item, itemPath)
    exact(fill, ["stock", "day", "shares", "gross", "fees"], itemPath)
    const fillStock = string(fill.stock, `${itemPath}.stock`), day = civilDate(fill.day, `${itemPath}.day`)
    const shares = decimal(fill.shares, `${itemPath}.shares`), gross = money(fill.gross, `${itemPath}.gross`), fees = money(fill.fees, `${itemPath}.fees`)
    if (BigInt(shares) <= 0n || BigInt(gross) <= 0n || BigInt(fees) < 0n) throw new SaveSchemaError(itemPath, "回购成交数量或金额非法")
    if (fillStock !== stock) throw new SaveSchemaError(`${itemPath}.stock`, "回购成交证券与方案不一致")
    if (day < window_start_on || day > window_deadline_on) throw new SaveSchemaError(`${itemPath}.day`, "回购成交日期不在执行窗口内")
    return { stock: fillStock, day, shares, gross, fees }
  })
  const completed_on = book.completed_on === null ? null : civilDate(book.completed_on, `${path}.completed_on`)
  const withdrawn_remainder = book.withdrawn_remainder === null ? null : money(book.withdrawn_remainder, `${path}.withdrawn_remainder`)
  const cancelled_on = book.cancelled_on === null ? null : civilDate(book.cancelled_on, `${path}.cancelled_on`)
  const cancelled_shares = decimal(book.cancelled_shares, `${path}.cancelled_shares`)
  const last_order_day = book.last_order_day === null ? null : civilDate(book.last_order_day, `${path}.last_order_day`)
  const totalShares = fills.reduce((sum, fill) => sum + BigInt(fill.shares), 0n)
  const totalSpent = fills.reduce((sum, fill) => sum + BigInt(fill.gross) + BigInt(fill.fees), 0n)
  if (totalShares > BigInt(max_shares)) throw new SaveSchemaError(`${path}.fills`, "回购成交合计超过数量上限")
  if (totalSpent > BigInt(total_budget)) throw new SaveSchemaError(`${path}.fills`, "回购成交支出超过获批额度")
  if (completed_on === null && withdrawn_remainder !== null) throw new SaveSchemaError(`${path}.withdrawn_remainder`, "回收事实缺少完成日期")
  if (completed_on !== null && (withdrawn_remainder === null || BigInt(withdrawn_remainder) !== BigInt(total_budget) - totalSpent)) throw new SaveSchemaError(`${path}.withdrawn_remainder`, "回收差额必须等于额度减实际支出")
  if ((status === "Completed" || status === "Cancelled") !== (completed_on !== null)) throw new SaveSchemaError(`${path}.status`, "回购状态与完成事实不一致")
  if ((cancelled_on !== null || BigInt(cancelled_shares) !== 0n) && (status !== "Cancelled" || cancelled_on === null || BigInt(cancelled_shares) === 0n || BigInt(cancelled_shares) > totalShares)) throw new SaveSchemaError(`${path}.cancelled_shares`, "回购注销事实非法")
  return { plan: parsedPlan, status, fills, completed_on, withdrawn_remainder, cancelled_on, cancelled_shares, last_order_day }
}
