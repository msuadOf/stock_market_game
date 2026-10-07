import type { CompanyCapabilities } from "../types/generated/CompanyCapabilities";
import type { OwnerRightsOfferingView } from "../types/generated/OwnerRightsOfferingView";
import type { PeriodChangeExplanation } from "../types/generated/PeriodChangeExplanation";
import { accountingAmount, array, boolean as booleanField, civilDate, decimal, exact, integer, money, nullable, oneOf, record, string } from "../save/schema/primitives.ts";

/**
 * F 批共同契约查询面的严格 parser：只接受 Engine 导出的当前契约形状，
 * 不用宽松 any 透传。金额口径与 engine 一致——`AccountingAmount` 元字符串、
 * `Money`/股数十进制分（规范字符串），不可用一律显式 reason，不填零。
 */

const ACTION_KINDS = ["CashDividend", "StockDistribution", "RightsOffering", "IssuerRepurchase", "ShareSplit"] as const;
const PLAN_STAGES = ["Approved", "Announced", "Registered", "Entitled", "Closed", "Payable", "PartiallyPaid", "Executing", "Completed"] as const;
const WINDOW_STATES = ["BeforeOpen", "Open", "Closed"] as const;
const SETTLEMENT_CYCLES = ["Monthly", "Quarterly", "HalfYear", "Annual"] as const;

type JsonRecord = Readonly<Record<string, unknown>>;

function requireVariant(value: unknown, path: string, variants: readonly string[]): { readonly key: string; readonly body: JsonRecord } {
  const wrapper = record(value, path);
  const keys = Object.keys(wrapper);
  if (keys.length !== 1 || !variants.includes(keys[0])) {
    throw new Error(`${path} 必须是单键枚举（${variants.join(" | ")}）`);
  }
  return { key: keys[0], body: record(wrapper[keys[0]], `${path}.${keys[0]}`) };
}

/** 不可用分支公共形状（三个金额事实同构）：显式非空 reason，不填零（铁律）。 */
function unavailableReason(body: JsonRecord, path: string): { Unavailable: { reason: string } } {
  exact(body, ["reason"], `${path}.Unavailable`);
  const reason = string(body.reason, `${path}.Unavailable.reason`);
  if (!reason.trim()) throw new Error(`${path}.Unavailable.reason 不可用原因必须非空`);
  return { Unavailable: { reason } };
}

function parseCapabilityAmount(value: unknown, path: string): CompanyCapabilities["registered_capital"] {
  const { key, body } = requireVariant(value, path, ["Available", "Unavailable"]);
  if (key === "Available") {
    exact(body, ["amount_yuan"], `${path}.Available`);
    return { Available: { amount_yuan: accountingAmount(body.amount_yuan, `${path}.Available.amount_yuan`) } };
  }
  return unavailableReason(body, path);
}

function parseCapabilityMoney(value: unknown, path: string): CompanyCapabilities["par_value_per_share"] {
  const { key, body } = requireVariant(value, path, ["Available", "Unavailable"]);
  if (key === "Available") {
    exact(body, ["cents"], `${path}.Available`);
    return { Available: { cents: decimal(body.cents, `${path}.Available.cents`) } };
  }
  return unavailableReason(body, path);
}

function parseDistributableProfitSnapshot(value: unknown, path: string): CompanyCapabilities["distributable_profit"] {
  const { key, body } = requireVariant(value, path, ["Available", "Unavailable"]);
  if (key === "Available") {
    exact(body, ["accumulated_after_loss_yuan", "statutory_reserve_yuan", "available_for_distribution_yuan", "reserve_basis_year"], `${path}.Available`);
    return {
      Available: {
        accumulated_after_loss_yuan: accountingAmount(body.accumulated_after_loss_yuan, `${path}.Available.accumulated_after_loss_yuan`),
        statutory_reserve_yuan: accountingAmount(body.statutory_reserve_yuan, `${path}.Available.statutory_reserve_yuan`),
        available_for_distribution_yuan: accountingAmount(body.available_for_distribution_yuan, `${path}.Available.available_for_distribution_yuan`),
        reserve_basis_year: nullable(body.reserve_basis_year, `${path}.Available.reserve_basis_year`, (nested, nestedPath) => integer(nested, nestedPath)),
      },
    };
  }
  return unavailableReason(body, path);
}

/** 公司能力面（owner 隔离：owner_rights 只含查询账户本人的事实）。 */
export function parseCompanyCapabilities(value: unknown, path = "company_capabilities"): CompanyCapabilities {
  const view = record(value, path);
  exact(view, [
    "revenue", "net_income", "equity", "cash_flow", "full_financial_statements", "cash_settlement",
    "unsupported_reason", "par_value_per_share", "issued_shares", "registered_capital",
    "distributable_profit", "active_plans", "action_readiness", "owner_rights",
  ], path);
  const active_plans = array(view.active_plans, `${path}.active_plans`).map((item, index) => {
    const rowPath = `${path}.active_plans[${index}]`;
    const plan = record(item, rowPath);
    exact(plan, ["kind", "identity", "stage", "key_dates"], rowPath);
    const identity = string(plan.identity, `${rowPath}.identity`);
    if (!identity.trim()) throw new Error(`${rowPath}.identity 方案身份不能为空`);
    return {
      kind: oneOf(plan.kind, `${rowPath}.kind`, ACTION_KINDS),
      identity,
      stage: oneOf(plan.stage, `${rowPath}.stage`, PLAN_STAGES),
      key_dates: array(plan.key_dates, `${rowPath}.key_dates`).map((entry, entryIndex) => {
        const keyDatePath = `${rowPath}.key_dates[${entryIndex}]`;
        const keyDate = record(entry, keyDatePath);
        exact(keyDate, ["label", "date"], keyDatePath);
        const label = string(keyDate.label, `${keyDatePath}.label`);
        if (!label.trim()) throw new Error(`${keyDatePath}.label 关键日期标签不能为空`);
        return { label, date: civilDate(keyDate.date, `${keyDatePath}.date`) };
      }),
    };
  });
  const action_readiness = array(view.action_readiness, `${path}.action_readiness`).map((item, index) => {
    const rowPath = `${path}.action_readiness[${index}]`;
    const readiness = record(item, rowPath);
    exact(readiness, ["kind", "ready", "blockers"], rowPath);
    return {
      kind: oneOf(readiness.kind, `${rowPath}.kind`, ACTION_KINDS),
      ready: booleanField(readiness.ready, `${rowPath}.ready`),
      blockers: array(readiness.blockers, `${rowPath}.blockers`).map((blocker, blockerIndex) => {
        const text = string(blocker, `${rowPath}.blockers[${blockerIndex}]`);
        if (!text.trim()) throw new Error(`${rowPath}.blockers[${blockerIndex}] 阻塞原因必须非空`);
        return text;
      }),
    };
  });
  if (action_readiness.length !== 5) throw new Error(`${path}.action_readiness 必须恰好五类各一条`);
  const owner_rights = array(view.owner_rights, `${path}.owner_rights`).map((item, index) => {
    const rowPath = `${path}.owner_rights[${index}]`;
    const summary = record(item, rowPath);
    exact(summary, ["event_id", "stock", "stage", "payment_window", "entitled_shares", "open_subscription_remaining"], rowPath);
    const event_id = string(summary.event_id, `${rowPath}.event_id`);
    const stock = string(summary.stock, `${rowPath}.stock`);
    if (!event_id.trim() || !stock.trim()) throw new Error(`${rowPath} 事件身份或证券代码不能为空`);
    return {
      event_id,
      stock,
      stage: oneOf(summary.stage, `${rowPath}.stage`, PLAN_STAGES),
      payment_window: oneOf(summary.payment_window, `${rowPath}.payment_window`, WINDOW_STATES),
      entitled_shares: nullable(summary.entitled_shares, `${rowPath}.entitled_shares`, decimal),
      open_subscription_remaining: nullable(summary.open_subscription_remaining, `${rowPath}.open_subscription_remaining`, decimal),
    };
  });
  const unsupported_reason = string(view.unsupported_reason, `${path}.unsupported_reason`);
  if (!unsupported_reason.trim()) throw new Error(`${path}.unsupported_reason 必须显式说明`);
  return {
    revenue: booleanField(view.revenue, `${path}.revenue`),
    net_income: booleanField(view.net_income, `${path}.net_income`),
    equity: booleanField(view.equity, `${path}.equity`),
    cash_flow: booleanField(view.cash_flow, `${path}.cash_flow`),
    full_financial_statements: booleanField(view.full_financial_statements, `${path}.full_financial_statements`),
    cash_settlement: booleanField(view.cash_settlement, `${path}.cash_settlement`),
    unsupported_reason,
    par_value_per_share: parseCapabilityMoney(view.par_value_per_share, `${path}.par_value_per_share`),
    issued_shares: decimal(view.issued_shares, `${path}.issued_shares`),
    registered_capital: parseCapabilityAmount(view.registered_capital, `${path}.registered_capital`),
    distributable_profit: parseDistributableProfitSnapshot(view.distributable_profit, `${path}.distributable_profit`),
    active_plans,
    action_readiness,
    owner_rights,
  };
}

/** 本人（owner）未完成配股方案视图：权证/额度/缴款窗口/认购进度。 */
export function parseOwnerRightsOfferings(value: unknown, path = "owner_rights_offerings"): readonly OwnerRightsOfferingView[] {
  return array(value, path).map((item, index) => {
    const rowPath = `${path}[${index}]`;
    const view = record(item, rowPath);
    exact(view, [
      "event_id", "stock", "issuer", "stage", "payment_window", "price_per_share",
      "payment_start_on", "payment_deadline_on", "ex_rights_on", "settlement_on",
      "owner_entitlement", "open_subscription_remaining_shares", "queued_subscription", "settled_subscription",
    ], rowPath);
    const event_id = string(view.event_id, `${rowPath}.event_id`);
    const stock = string(view.stock, `${rowPath}.stock`);
    const issuer = string(view.issuer, `${rowPath}.issuer`);
    if (!event_id.trim() || !stock.trim() || !issuer.trim()) throw new Error(`${rowPath} 事件身份、证券或发行人不能为空`);
    const owner_entitlement = nullable(view.owner_entitlement, `${rowPath}.owner_entitlement`, (nested, nestedPath) => {
      const entitlement = record(nested, nestedPath);
      exact(entitlement, ["rights_shares", "lock_until"], nestedPath);
      const rights_shares = decimal(entitlement.rights_shares, `${nestedPath}.rights_shares`);
      if (BigInt(rights_shares) <= 0n) throw new Error(`${nestedPath}.rights_shares 具名权利必须为正`);
      return {
        rights_shares,
        lock_until: nullable(entitlement.lock_until, `${nestedPath}.lock_until`, civilDate),
      };
    });
    const queued_subscription = nullable(view.queued_subscription, `${rowPath}.queued_subscription`, (nested, nestedPath) => {
      const queued = record(nested, nestedPath);
      exact(queued, ["requested_shares", "submitted_on"], nestedPath);
      const requested_shares = decimal(queued.requested_shares, `${nestedPath}.requested_shares`);
      if (BigInt(requested_shares) <= 0n) throw new Error(`${nestedPath}.requested_shares 排队认购必须为正`);
      return { requested_shares, submitted_on: civilDate(queued.submitted_on, `${nestedPath}.submitted_on`) };
    });
    const settled_subscription = nullable(view.settled_subscription, `${rowPath}.settled_subscription`, (nested, nestedPath) => {
      const settled = record(nested, nestedPath);
      exact(settled, ["requested_shares", "paid_shares", "paid_amount", "waived_shares"], nestedPath);
      const requested_shares = decimal(settled.requested_shares, `${nestedPath}.requested_shares`);
      const paid_shares = decimal(settled.paid_shares, `${nestedPath}.paid_shares`);
      const waived_shares = decimal(settled.waived_shares, `${nestedPath}.waived_shares`);
      if (BigInt(requested_shares) <= 0n) throw new Error(`${nestedPath}.requested_shares 已结算认购必须为正`);
      if (BigInt(paid_shares) + BigInt(waived_shares) !== BigInt(requested_shares)) {
        throw new Error(`${nestedPath} 划扣+弃配必须等于申请认购数`);
      }
      return {
        requested_shares,
        paid_shares,
        paid_amount: money(settled.paid_amount, `${nestedPath}.paid_amount`),
        waived_shares,
      };
    });
    return {
      event_id,
      stock,
      issuer,
      stage: oneOf(view.stage, `${rowPath}.stage`, PLAN_STAGES),
      payment_window: oneOf(view.payment_window, `${rowPath}.payment_window`, WINDOW_STATES),
      price_per_share: money(view.price_per_share, `${rowPath}.price_per_share`),
      payment_start_on: civilDate(view.payment_start_on, `${rowPath}.payment_start_on`),
      payment_deadline_on: civilDate(view.payment_deadline_on, `${rowPath}.payment_deadline_on`),
      ex_rights_on: civilDate(view.ex_rights_on, `${rowPath}.ex_rights_on`),
      settlement_on: civilDate(view.settlement_on, `${rowPath}.settlement_on`),
      owner_entitlement,
      open_subscription_remaining_shares: nullable(view.open_subscription_remaining_shares, `${rowPath}.open_subscription_remaining_shares`, decimal),
      queued_subscription,
      settled_subscription,
    };
  });
}

/** 期间变化解释（复用 engine history；金额元字符串，bp 为整数）。 */
export function parsePeriodChangeExplanation(value: unknown, path = "period_change_explanation"): PeriodChangeExplanation {
  const view = record(value, path);
  exact(view, [
    "previous", "cycle", "environment_change_bp", "demand_contribution_bp",
    "revenue_segments", "fixed_expense_segments", "variable_expense_segments",
    "revenue_noise_bp", "fixed_expense_noise_bp", "variable_expense_noise_bp",
    "restart_revenue", "restart_source",
  ], path);
  const previous = record(view.previous, `${path}.previous`);
  exact(previous, ["revenue", "fixed_expense", "variable_expense"], `${path}.previous`);
  const parseSegments = (segments: unknown, segmentsPath: string) => array(segments, segmentsPath).map((item, index) => {
    const rowPath = `${segmentsPath}[${index}]`;
    const segment = record(item, rowPath);
    exact(segment, ["annual_growth_bp", "months"], rowPath);
    return {
      annual_growth_bp: integer(segment.annual_growth_bp, `${rowPath}.annual_growth_bp`),
      months: integer(segment.months, `${rowPath}.months`, 1),
    };
  });
  return {
    previous: {
      revenue: accountingAmount(previous.revenue, `${path}.previous.revenue`),
      fixed_expense: accountingAmount(previous.fixed_expense, `${path}.previous.fixed_expense`),
      variable_expense: accountingAmount(previous.variable_expense, `${path}.previous.variable_expense`),
    },
    cycle: oneOf(view.cycle, `${path}.cycle`, SETTLEMENT_CYCLES),
    environment_change_bp: integer(view.environment_change_bp, `${path}.environment_change_bp`),
    demand_contribution_bp: integer(view.demand_contribution_bp, `${path}.demand_contribution_bp`),
    revenue_segments: parseSegments(view.revenue_segments, `${path}.revenue_segments`),
    fixed_expense_segments: parseSegments(view.fixed_expense_segments, `${path}.fixed_expense_segments`),
    variable_expense_segments: parseSegments(view.variable_expense_segments, `${path}.variable_expense_segments`),
    revenue_noise_bp: integer(view.revenue_noise_bp, `${path}.revenue_noise_bp`),
    fixed_expense_noise_bp: integer(view.fixed_expense_noise_bp, `${path}.fixed_expense_noise_bp`),
    variable_expense_noise_bp: integer(view.variable_expense_noise_bp, `${path}.variable_expense_noise_bp`),
    restart_revenue: nullable(view.restart_revenue, `${path}.restart_revenue`, accountingAmount),
    restart_source: nullable(view.restart_source, `${path}.restart_source`, string),
  };
}

/** 简税（FlatWithholding）代扣回执（owner 查询面）；与 engine
 * `FlatWithholdingReceipt` 的 serde 形态及存档 schema 同构。 */
export interface FlatWithholdingReceiptView {
  readonly payment_id: string;
  readonly plan_id: string;
  readonly account: string;
  readonly paid_on: string;
  readonly gross: string;
  readonly rate_bp: number;
  readonly withheld: string;
}

/** 简税代扣回执严格 parser（owner 过滤后仅含本人回执）。 */
export function parseFlatWithholdingReceipts(value: unknown, path = "flat_withholding_receipts"): readonly FlatWithholdingReceiptView[] {
  return array(value, path).map((item, index) => {
    const rowPath = `${path}[${index}]`;
    const receipt = record(item, rowPath);
    exact(receipt, ["payment_id", "plan_id", "account", "paid_on", "gross", "rate_bp", "withheld"], rowPath);
    const payment_id = string(receipt.payment_id, `${rowPath}.payment_id`);
    const plan_id = string(receipt.plan_id, `${rowPath}.plan_id`);
    const account = string(receipt.account, `${rowPath}.account`);
    const gross = money(receipt.gross, `${rowPath}.gross`);
    const withheld = money(receipt.withheld, `${rowPath}.withheld`);
    if (!payment_id.trim() || !plan_id.trim()) throw new Error(`${rowPath} 回执身份不能为空`);
    if (!/^(0|[1-9]\d*)$/.test(account)) throw new Error(`${rowPath}.account 必须是规范账户十进制字符串`);
    if (BigInt(gross) <= 0n || BigInt(withheld) < 0n) throw new Error(`${rowPath} 回执金额非法`);
    return {
      payment_id,
      plan_id,
      account,
      paid_on: civilDate(receipt.paid_on, `${rowPath}.paid_on`),
      gross,
      rate_bp: integer(receipt.rate_bp, `${rowPath}.rate_bp`, 0),
      withheld,
    };
  });
}
