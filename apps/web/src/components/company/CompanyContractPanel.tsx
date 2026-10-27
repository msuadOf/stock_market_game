/* oxlint-disable react/only-export-components -- 文案 helper 供 SSR 测试与面板共用（同 RightsSubscriptionRejectionPanel 模式） */
import { useCallback, useState } from "react";
import { useCompanyQuery } from "./use-company-query.ts";
import type { CorporateActionKind } from "../../types/generated/CorporateActionKind.ts";
import type { ActivePlanStage } from "../../types/generated/ActivePlanStage.ts";
import type { SettlementCycle } from "../../types/generated/SettlementCycle.ts";
import type { PaymentWindowState } from "../../types/generated/PaymentWindowState.ts";
import type { CompanyCapabilities } from "../../types/generated/CompanyCapabilities.ts";
import type { PeriodChangeExplanation } from "../../types/generated/PeriodChangeExplanation.ts";
import type { OwnerRightsOfferingView } from "../../host/company-contract-views.ts";
import type { FlatWithholdingReceipt } from "../../host/dividend-tax.ts";

interface CompanyContractPanelProps {
  readonly companyId: number;
  /** 公司能力面查询（owner 隔离：owner_rights 只含本人事实）；undefined 表示宿主明确不支持。 */
  readonly onCapabilitiesQuery: ((company: number) => Promise<CompanyCapabilities>) | undefined;
  /** 本人配股权益查询（owner 隔离）；undefined 表示宿主明确不支持。 */
  readonly onOwnerRightsQuery: (() => Promise<readonly OwnerRightsOfferingView[]>) | undefined;
  /** 期间变化解释查询；undefined 表示宿主明确不支持。 */
  readonly onExplanationQuery: ((company: number, periodEnd: string) => Promise<PeriodChangeExplanation>) | undefined;
  /** 简税代扣回执查询（owner 隔离）；undefined 表示宿主明确不支持（非 Flat 模式由 engine 报错展示）。 */
  readonly onFlatReceiptsQuery: (() => Promise<readonly FlatWithholdingReceipt[]>) | undefined;
  /** 刷新键：owner、会话 generation 或自然日变化时必须更新。 */
  readonly refreshKey: string;
}

const KIND_LABELS: Record<CorporateActionKind, string> = {
  CashDividend: "现金分红",
  StockDistribution: "送转",
  RightsOffering: "配股／增发",
  IssuerRepurchase: "回购",
  ShareSplit: "拆股／缩股",
};

const STAGE_LABELS: Record<ActivePlanStage, string> = {
  Approved: "已批准",
  Announced: "已公告",
  Registered: "已登记",
  Entitled: "权证已派发",
  Closed: "缴款已关窗",
  Payable: "待派息",
  PartiallyPaid: "部分派息",
  Executing: "执行中",
  Completed: "已完成待注销",
};

const CYCLE_LABELS: Record<SettlementCycle, string> = { Monthly: "自然月", Quarterly: "自然季", HalfYear: "自然半年", Annual: "自然年" };

const WINDOW_LABELS: Record<PaymentWindowState, string> = {
  BeforeOpen: "未开放",
  Open: "缴款期内",
  Closed: "已截止",
};

/** 行为类别中文文案（测试共用）。 */
export function contractKindLabel(kind: CorporateActionKind): string {
  return KIND_LABELS[kind];
}

/** 方案阶段中文文案（测试共用）。 */
export function contractStageLabel(stage: ActivePlanStage): string {
  return STAGE_LABELS[stage];
}

/** 缴款窗口中文文案（测试共用）。 */
export function contractWindowLabel(window: PaymentWindowState): string {
  return WINDOW_LABELS[window];
}

/** 金额事实文案：可用给元字符串，不可用给 reason（不填零）。 */
export function contractAmountText(
  fact: { readonly Available: { readonly amount_yuan: string } } | { readonly Unavailable: { readonly reason: string } },
): string {
  if ("Available" in fact) return `${fact.Available.amount_yuan} 元`;
  return `不可用：${fact.Unavailable.reason}`;
}

/** 可分配利润快照文案（同可用性，三字段整体展示）。 */
export function contractDistributableText(capabilities: CompanyCapabilities): string {
  const snapshot = capabilities.distributable_profit;
  if ("Available" in snapshot) {
    const { accumulated_after_loss_yuan, statutory_reserve_yuan, available_for_distribution_yuan } = snapshot.Available;
    return `可分配 ${available_for_distribution_yuan} 元（亏损弥补后累积 ${accumulated_after_loss_yuan} 元、法定公积金 ${statutory_reserve_yuan} 元）`;
  }
  return `不可用：${snapshot.Unavailable.reason}`;
}

/** 每股面值文案（Money 分字符串口径）。 */
export function contractParValueText(capabilities: CompanyCapabilities): string {
  const fact = capabilities.par_value_per_share;
  if ("Available" in fact) return `${fact.Available.cents} 分/股`;
  return `不可用：${fact.Unavailable.reason}`;
}

/**
 * 公司共同契约能力面（F 批收口）：只读展示公司当前事实、未完成方案阶段、
 * 各行为业务条件、本人配股权益与期间变化解释；owner 隔离查询，不新增写操作。
 */
export function CompanyContractPanel({ companyId, onCapabilitiesQuery, onOwnerRightsQuery, onExplanationQuery, onFlatReceiptsQuery, refreshKey }: CompanyContractPanelProps) {
  const capabilitiesQuery = useCallback(() => {
    if (onCapabilitiesQuery === undefined) throw new Error("当前宿主不支持公司能力面查询");
    return onCapabilitiesQuery(companyId);
  }, [onCapabilitiesQuery, companyId]);
  const [capabilities, capabilitiesError, capabilitiesLoading, refreshCapabilities] = useCompanyQuery(onCapabilitiesQuery === undefined ? undefined : capabilitiesQuery, [refreshKey, companyId]);
  const [rights, rightsError, rightsLoading, refreshRights] = useCompanyQuery(onOwnerRightsQuery, [refreshKey, companyId]);
  const [receipts, receiptsError, receiptsLoading, refreshReceipts] = useCompanyQuery(onFlatReceiptsQuery, [refreshKey, companyId]);
  const [selection, setSelection] = useState({ companyId, refreshKey, date: "", submitted: "" });
  const validDate = /^\d{4}-\d{2}-\d{2}$/.test(selection.date) && Number.isFinite(Date.parse(`${selection.date}T00:00:00Z`)) && new Date(`${selection.date}T00:00:00Z`).toISOString().slice(0, 10) === selection.date;
  const currentSelection = selection.companyId === companyId && selection.refreshKey === refreshKey;
  const explanationPeriodEnd = currentSelection ? selection.date : "";
  const submitted = currentSelection ? selection.submitted : "";
  const explanationQuery = useCallback(() => {
    if (onExplanationQuery === undefined) throw new Error("当前宿主不支持期间解释查询");
    return onExplanationQuery(companyId, submitted);
  }, [onExplanationQuery, companyId, submitted]);
  const [explanation, explanationError, explanationLoading, refreshExplanation] = useCompanyQuery(onExplanationQuery === undefined || submitted === "" ? undefined : explanationQuery, [refreshKey, companyId, submitted]);
  const queryExplanation = () => {
    if (submitted === explanationPeriodEnd) refreshExplanation();
    else setSelection({ companyId, refreshKey, date: explanationPeriodEnd, submitted: explanationPeriodEnd });
  };

  return (
    <section className="company-contract" aria-label="公司共同契约能力面">
      <h4>公司能力面与当前方案</h4>
      {onCapabilitiesQuery === undefined
        ? <p className="company-state">当前宿主不支持公司能力面查询。</p>
        : (
          <>
            {!capabilitiesLoading && capabilitiesError === null && capabilities === null && <p>尚未查询公司能力面。</p>}
            {capabilitiesLoading && <p className="company-state" role="status">正在查询公司能力面…</p>}
            {capabilitiesError !== null && <p className="company-state is-error" role="alert">公司能力面查询失败：{capabilitiesError}</p>}
            {capabilities !== null && (
              <div className="company-contract-facts">
                <p>现行总股本 <strong className="num">{capabilities.issued_shares}</strong> 股 · 每股面值 {contractParValueText(capabilities)} · 注册资本 {contractAmountText(capabilities.registered_capital)}</p>
                <p>只读财务能力：营收 {capabilities.revenue ? "可查询" : "不支持"}；净利润 {capabilities.net_income ? "可查询" : "不支持"}；权益 {capabilities.equity ? "可查询" : "不支持"}；现金流 {capabilities.cash_flow ? "可查询" : "不支持"}；完整四表 {capabilities.full_financial_statements ? "可查询" : "不支持"}。</p>
                <p>可分配利润快照：{contractDistributableText(capabilities)}</p>
                {!capabilities.cash_settlement && <p className="company-state">共同股本行为实际投资者结算未完成接线：{capabilities.unsupported_reason}</p>}
                <h5>当前未完成方案</h5>
                {capabilities.active_plans.length === 0
                  ? <p className="company-state">该公司没有未完成的公司行为方案。</p>
                  : (
                    <table className="grid-table">
                      <thead><tr><th>类别</th><th>方案</th><th>阶段</th><th>关键日期</th></tr></thead>
                      <tbody>
                        {capabilities.active_plans.map((plan) => (
                          <tr key={JSON.stringify([plan.kind, plan.identity])}>
                            <td>{contractKindLabel(plan.kind)}</td>
                            <td className="mono">{plan.identity}</td>
                            <td>{contractStageLabel(plan.stage)}</td>
                            <td>{plan.key_dates.map((entry) => `${entry.label} ${entry.date}`).join("；")}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  )}
                <h5>各行为业务条件</h5>
                <table className="grid-table">
                  <thead><tr><th>类别</th><th>当前条件</th><th>原因</th></tr></thead>
                  <tbody>
                    {capabilities.action_readiness.map((entry) => (
                      <tr key={entry.kind}>
                        <td>{contractKindLabel(entry.kind)}</td>
                        <td>{entry.ready ? "满足" : "不满足"}</td>
                        <td>{entry.blockers.length === 0 ? "—" : entry.blockers.join("；")}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
            <button type="button" disabled={capabilitiesLoading} onClick={refreshCapabilities}>刷新能力面</button>
          </>
        )}
      <h5>本人配股权益（全部公司）</h5>
      <p>以下为只读事实。已排队表示受理，日终缴款与登记按阶段记录；本视图无退款事实字段，不能据此认定已退款。</p>
      {onOwnerRightsQuery === undefined
        ? <p className="company-state">当前宿主不支持本人配股权益查询。</p>
        : (
          <>
            {!rightsLoading && rightsError === null && rights === null && <p>尚未查询本人配股权益。</p>}
            {rightsLoading && <p className="company-state" role="status">正在查询本人配股权益…</p>}
            {rightsError !== null && <p className="company-state is-error" role="alert">本人配股权益查询失败：{rightsError}</p>}
            {rights !== null && rights.length === 0 && <p className="company-state">本人当前没有未完成的配股方案。</p>}
            {rights !== null && rights.length > 0 && (
              <table className="grid-table">
                <thead><tr><th>事件</th><th>证券</th><th>阶段</th><th>缴款窗口</th><th>窗口状态</th><th>本人权利/额度</th><th>认购进度</th></tr></thead>
                <tbody>
                  {rights.map((view) => (
                    <tr key={view.event_id}>
                      <td className="mono">{view.event_id}</td>
                      <td className="mono">{view.stock}（公司编号 {view.issuer}）<br />认购价 {view.price_per_share} 分/股<br />除权日 {view.ex_rights_on}；登记结算日 {view.settlement_on}</td>
                      <td>{contractStageLabel(view.stage)}</td>
                      <td className="mono">{view.payment_start_on} ~ {view.payment_deadline_on}</td>
                      <td>{contractWindowLabel(view.payment_window)}</td>
                      <td className="num">
                        {view.owner_entitlement !== null
                          ? `权利 ${view.owner_entitlement.rights_shares} 股；${view.owner_entitlement.lock_until === null ? "无定向锁定期" : `锁定期截止 ${view.owner_entitlement.lock_until}`}`
                          : view.open_subscription_remaining_shares !== null
                            ? `公开配售剩余 ${view.open_subscription_remaining_shares} 股`
                            : "无权利"}
                      </td>
                      <td>
                        {view.queued_subscription !== null
                          ? `已排队 ${view.queued_subscription.requested_shares} 股（${view.queued_subscription.submitted_on} 提交）`
                          : view.settled_subscription !== null
                            ? `申请 ${view.settled_subscription.requested_shares} 股；已缴款认购 ${view.settled_subscription.paid_shares} 股、弃配 ${view.settled_subscription.waived_shares} 股、缴款 ${view.settled_subscription.paid_amount} 分`
                            : "无已排队或已结算认购记录"}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
            <button type="button" disabled={rightsLoading} onClick={refreshRights}>刷新配股权益</button>

          </>
        )}
      <h5>期间变化解释</h5>
      {onExplanationQuery === undefined
        ? <p className="company-state">当前宿主不支持期间解释查询。</p>
        : (
          <>
            <label className="company-picker"><span>期间末日</span>
              <input aria-label="期间解释期间末日" type="date" value={explanationPeriodEnd} onChange={(event) => setSelection({ companyId, refreshKey, date: event.currentTarget.value, submitted: "" })} />
            </label>
            <button type="button" disabled={explanationLoading || !currentSelection || !validDate} onClick={queryExplanation}>{explanationLoading ? "查询中…" : "查询期间解释"}</button>
            {submitted === "" && <p className="company-state">尚未查询期间解释。</p>}
            {explanationLoading && <p role="status">正在查询期间解释…</p>}
            {explanationError !== null && <p className="company-state is-error" role="alert">期间解释查询失败：{explanationError}</p>}
            {explanation !== null && (
              <div className="company-contract-explanation">
                <p>结算周期：{CYCLE_LABELS[explanation.cycle]}；查询期间末日：{submitted}。</p>
                <p>上一期：营收 {explanation.previous.revenue} 元、固定开支 {explanation.previous.fixed_expense} 元、变动开支 {explanation.previous.variable_expense} 元</p>
                <p>环境变化 {explanation.environment_change_bp}bp · 需求贡献 {explanation.demand_contribution_bp}bp · 噪声（营收/固定/变动）{explanation.revenue_noise_bp}/{explanation.fixed_expense_noise_bp}/{explanation.variable_expense_noise_bp}bp</p>
                <p>年化趋势分段（增长率 bp / 月数）：营收 {explanation.revenue_segments.map((part) => `${part.annual_growth_bp}bp / ${part.months}月`).join("；") || "无分段"}；固定开支 {explanation.fixed_expense_segments.map((part) => `${part.annual_growth_bp}bp / ${part.months}月`).join("；") || "无分段"}；变动开支 {explanation.variable_expense_segments.map((part) => `${part.annual_growth_bp}bp / ${part.months}月`).join("；") || "无分段"}。</p>
                <p>{explanation.restart_revenue === null ? "本期无复业" : `本期复业：基准 ${explanation.restart_revenue} 元（${explanation.restart_source === null ? "未提供来源" : explanation.restart_source}）`}</p>
              </div>
            )}
          </>
        )}
      <h5>本人简税代扣回执（全部公司）</h5>
      {onFlatReceiptsQuery === undefined
        ? <p className="company-state">当前宿主不支持简税代扣回执查询。</p>
        : (
          <>
            {!receiptsLoading && receiptsError === null && receipts === null && <p>尚未查询简税代扣回执。</p>}
            {receiptsLoading && <p className="company-state" role="status">正在查询简税代扣回执…</p>}
            {receiptsError !== null && <p className="company-state is-error" role="alert">简税代扣回执查询失败：{receiptsError}</p>}
            {receipts !== null && receipts.length === 0 && <p className="company-state">本人暂无简税代扣回执。</p>}
            {receipts !== null && receipts.length > 0 && (
              <table className="grid-table">
                <thead><tr><th>付款批次</th><th>分红计划</th><th>派息日</th><th>税前应得（分）</th><th>比例（bp）</th><th>代扣（分）</th></tr></thead>
                <tbody>
                  {receipts.map((receipt) => (
                    <tr key={JSON.stringify([receipt.payment_id, receipt.account])}>
                      <td className="mono">{receipt.payment_id}</td>
                      <td className="mono">{receipt.plan_id}</td>
                      <td className="mono">{receipt.paid_on}</td>
                      <td className="num">{receipt.gross}</td>
                      <td className="num">{receipt.rate_bp}</td>
                      <td className="num">{receipt.withheld}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
            <button type="button" disabled={receiptsLoading} onClick={refreshReceipts}>刷新简税回执</button>
          </>
        )}
    </section>
  );
}
