/* oxlint-disable react/only-export-components -- 文案 helper 供 SSR 测试与面板共用（同 CompanyContractPanel 模式） */
import { useEffect, useRef, useState } from "react";
import type { CompanyCapabilities } from "../../types/generated/CompanyCapabilities.ts";
import type { SimpleCompanyPreferences } from "../../types/generated/SimpleCompanyPreferences.ts";
import type { PlayerProposalResultView, PlayerProposalWire } from "../../host/player-proposals.ts";
import {
  buildCashDividendProposal,
  buildIssuerRepurchaseProposal,
  buildRightsOfferingProposal,
  buildSecondaryOfferingProposal,
  buildShareSplitProposal,
  buildStockDistributionProposal,
} from "../../host/player-proposals.ts";
import { centsToYuanText } from "./CompanyPreferencesInput.tsx";
import { useQueried } from "./CompanyContractPanel.tsx";

/**
 * 玩家提案与偏好局内编辑面板（N2b，2026-10-08 用户决策「持仓即可、直接生效」
 * 「偏好局内编辑即时生效、下周期评估」）。真实 A 股的公司决议由发行人治理
 * 程序作出、投资者不能发起——本入口为登记在案的游戏化简化（trading-rules
 * 「玩家提案」节）；提案前置只有账户持仓（任意数量 > 0），其余制度校验全部
 * 由 engine 既有 approve_* 入口权威执行，三类结果（受理／制度拒绝／无持仓
 * 拒绝）显式呈现。
 */

const KIND_LABELS: Record<string, string> = {
  CashDividend: "现金分红",
  StockDistribution: "送转",
  RightsOffering: "配股",
  SecondaryOffering: "增发（定向本人）",
  IssuerRepurchase: "回购",
  ShareSplit: "拆股／缩股",
};

const CLASS_LABELS: Record<string, string> = {
  InvalidInput: "参数非法",
  BusinessCondition: "制度条件不满足",
  UnsupportedOperation: "本局未支持",
  SystemState: "引擎状态错误",
};

/** 提案类别中文文案（测试共用）。 */
export function proposalKindLabel(kind: string): string {
  return KIND_LABELS[kind] ?? kind;
}

/** 制度拒绝分类中文文案（测试共用）。 */
export function proposalClassLabel(classification: string): string {
  return CLASS_LABELS[classification] ?? classification;
}

/** 提案类别 → 能力面行为类别（配股/增发共用配股机制面）。 */
function readinessKind(kind: ProposalCategory): "CashDividend" | "StockDistribution" | "RightsOffering" | "IssuerRepurchase" | "ShareSplit" {
  if (kind === "RightsOffering" || kind === "SecondaryOffering") return "RightsOffering";
  return kind;
}

type ProposalCategory = "CashDividend" | "StockDistribution" | "RightsOffering" | "SecondaryOffering" | "IssuerRepurchase" | "ShareSplit";
const CATEGORIES: readonly ProposalCategory[] = ["CashDividend", "StockDistribution", "RightsOffering", "SecondaryOffering", "IssuerRepurchase", "ShareSplit"];

interface PlayerProposalPanelProps {
  readonly companyId: string;
  /** 玩家提案提交；undefined 表示宿主明确不支持，面板显式提示。 */
  readonly onProposalSubmit: ((proposal: PlayerProposalWire) => Promise<PlayerProposalResultView>) | undefined;
  /** 公司能力面查询（前置条件与 blocker 展示，复用 F 批契约）；undefined 表示宿主不支持。 */
  readonly onCapabilitiesQuery: ((company: string) => Promise<CompanyCapabilities>) | undefined;
  /** 刷新键：自然日或会话 generation 变化时重查能力面。 */
  readonly refreshKey: string;
}

/**
 * 「发起提案」区：六类行为表单（复用各行为 plan 构造的最小参数集）+
 * F 批能力面前置展示 + 三类结果显式。金额输入为元（最多两位小数），
 * 比例/股数为整数；非法输入禁用提交并在本地显式提示，权威校验在 engine。
 */
export function PlayerProposalPanel({ companyId, onProposalSubmit, onCapabilitiesQuery, refreshKey }: PlayerProposalPanelProps) {
  const [category, setCategory] = useState<ProposalCategory>("CashDividend");
  const [result, setResult] = useState<PlayerProposalResultView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const submitSequence = useRef(0);
  const [capabilities, capabilitiesError, capabilitiesLoading, refreshCapabilities] = useQueried(
    onCapabilitiesQuery === undefined ? undefined : () => onCapabilitiesQuery(companyId),
    `${refreshKey}:${companyId}:proposal`,
  );

  // 现金分红
  const [dividendYuan, setDividendYuan] = useState("");
  // 送转
  const [distributionKind, setDistributionKind] = useState<"BonusShares" | "CapitalReserveConversion">("BonusShares");
  const [distributionRatio, setDistributionRatio] = useState("");
  // 配股
  const [rightsPriceYuan, setRightsPriceYuan] = useState("");
  const [rightsRatio, setRightsRatio] = useState("");
  const [paymentDays, setPaymentDays] = useState("1");
  // 增发
  const [offeringPriceYuan, setOfferingPriceYuan] = useState("");
  const [offeringShares, setOfferingShares] = useState("");
  // 回购
  const [repurchasePriceCapYuan, setRepurchasePriceCapYuan] = useState("");
  const [repurchaseBudgetYuan, setRepurchaseBudgetYuan] = useState("");
  const [repurchaseMaxShares, setRepurchaseMaxShares] = useState("");
  const [repurchaseWindowDays, setRepurchaseWindowDays] = useState("5");
  const [repurchasePurpose, setRepurchasePurpose] = useState<"ReduceCapital" | "EmployeeIncentive" | "ConvertibleConversion" | "ValueMaintenance">("ValueMaintenance");
  // 拆股／缩股
  const [splitDirection, setSplitDirection] = useState<"Split" | "Consolidate">("Split");
  const [splitRatio, setSplitRatio] = useState("");

  useEffect(() => {
    setResult(null);
    setError(null);
    submitSequence.current += 1;
    setSubmitting(false);
  }, [companyId, category, refreshKey]);

  if (onProposalSubmit === undefined) {
    return (
      <section className="player-proposal" aria-label="发起提案">
        <h4>发起提案</h4>
        <p className="company-state">当前宿主不支持玩家提案。</p>
      </section>
    );
  }

  const readiness = capabilities?.action_readiness.find((entry) => entry.kind === readinessKind(category));

  const submit = () => {
    const ticket = ++submitSequence.current;
    setError(null);
    setResult(null);
    let proposal: PlayerProposalWire;
    try {
      proposal = category === "CashDividend"
        ? buildCashDividendProposal(companyId, dividendYuan)
        : category === "StockDistribution"
          ? buildStockDistributionProposal(companyId, distributionKind, distributionRatio)
          : category === "RightsOffering"
            ? buildRightsOfferingProposal(companyId, rightsPriceYuan, rightsRatio, Number(paymentDays))
            : category === "SecondaryOffering"
              ? buildSecondaryOfferingProposal(companyId, offeringPriceYuan, offeringShares)
              : category === "IssuerRepurchase"
                ? buildIssuerRepurchaseProposal(companyId, repurchasePriceCapYuan, repurchaseBudgetYuan, repurchaseMaxShares, Number(repurchaseWindowDays), repurchasePurpose)
                : buildShareSplitProposal(companyId, splitDirection, splitRatio);
    } catch (invalid) {
      setError(invalid instanceof Error ? invalid.message : String(invalid));
      return;
    }
    setSubmitting(true);
    onProposalSubmit(proposal).then(
      (outcome) => {
        if (submitSequence.current === ticket) { setResult(outcome); setSubmitting(false); }
      },
      (failure) => {
        if (submitSequence.current === ticket) {
          setError(failure instanceof Error ? failure.message : String(failure));
          setSubmitting(false);
        }
      },
    );
  };

  return (
    <section className="player-proposal" aria-label="发起提案">
      <h4>发起提案</h4>
      <p className="company-note">真实 A 股的公司决议由发行人依治理程序作出，投资者不能发起；本入口为游戏化简化。持仓即可、直接生效：需先持有该公司上市证券（任意数量 &gt; 0，名册外持仓也算），受理后走与公司方案完全相同的制度校验，通过即生效。</p>
      <label className="company-picker"><span>行为类别</span>
        <select aria-label="提案行为类别" value={category} onChange={(event) => setCategory(event.currentTarget.value as ProposalCategory)}>
          {CATEGORIES.map((entry) => <option key={entry} value={entry}>{proposalKindLabel(entry)}</option>)}
        </select>
      </label>
      {onCapabilitiesQuery !== undefined && (
        <div className="player-proposal-readiness">
          <h5>当前制度条件（能力面）</h5>
          {capabilitiesLoading && <p className="company-state" role="status">正在查询公司能力面…</p>}
          {capabilitiesError !== null && <p className="company-state is-error" role="alert">公司能力面查询失败：{capabilitiesError}</p>}
          {capabilities !== null && readiness !== undefined && (
            <p className={readiness.ready ? undefined : "company-state"}>
              {readiness.ready ? "前置条件满足（最终以受理校验为准）。" : `当前不满足：${readiness.blockers.join("；")}`}
            </p>
          )}
          {capabilities !== null && "Available" in capabilities.distributable_profit && (
            <p className="company-note">可分配利润 {capabilities.distributable_profit.Available.available_for_distribution_yuan} 元；「满足」不承诺受理，制度校验以提交时点为准。</p>
          )}
          <button type="button" disabled={capabilitiesLoading} onClick={refreshCapabilities}>刷新能力面</button>
        </div>
      )}
      <div className="player-proposal-form">
        {category === "CashDividend" && (
          <label>每股税前红利（元）<input aria-label="每股税前红利（元）" inputMode="decimal" placeholder="如 0.10" value={dividendYuan} onChange={(event) => setDividendYuan(event.currentTarget.value)} /></label>
        )}
        {category === "StockDistribution" && (
          <>
            <label>种类
              <select aria-label="送转种类" value={distributionKind} onChange={(event) => setDistributionKind(event.currentTarget.value as typeof distributionKind)}>
                <option value="BonusShares">送股（股票股利）</option>
                <option value="CapitalReserveConversion">转增（资本公积转增）</option>
              </select>
            </label>
            <label>每股送转比例（百万分之一股/股）<input aria-label="每股送转比例（百万分之一股/股）" inputMode="numeric" placeholder="如 1000000 = 10 送 10" value={distributionRatio} onChange={(event) => setDistributionRatio(event.currentTarget.value)} /></label>
          </>
        )}
        {category === "RightsOffering" && (
          <>
            <label>发行价（元/股）<input aria-label="配股发行价（元）" inputMode="decimal" placeholder="如 5.00" value={rightsPriceYuan} onChange={(event) => setRightsPriceYuan(event.currentTarget.value)} /></label>
            <label>每 1 股配售比例（百万分之一股/股）<input aria-label="配股比例（百万分之一股/股）" inputMode="numeric" placeholder="如 100000 = 10 配 1" value={rightsRatio} onChange={(event) => setRightsRatio(event.currentTarget.value)} /></label>
            <label>缴款期（交易日数）<input aria-label="缴款期交易日数" inputMode="numeric" value={paymentDays} onChange={(event) => setPaymentDays(event.currentTarget.value)} /></label>
          </>
        )}
        {category === "SecondaryOffering" && (
          <>
            <label>发行价（元/股）<input aria-label="增发发行价（元）" inputMode="decimal" placeholder="如 5.00" value={offeringPriceYuan} onChange={(event) => setOfferingPriceYuan(event.currentTarget.value)} /></label>
            <label>定向股数（承购人为本人）<input aria-label="增发定向股数" inputMode="numeric" placeholder="如 100" value={offeringShares} onChange={(event) => setOfferingShares(event.currentTarget.value)} /></label>
          </>
        )}
        {category === "IssuerRepurchase" && (
          <>
            <label>回购价格上限（元/股）<input aria-label="回购价格上限（元）" inputMode="decimal" placeholder="如 9.00" value={repurchasePriceCapYuan} onChange={(event) => setRepurchasePriceCapYuan(event.currentTarget.value)} /></label>
            <label>获批额度（元）<input aria-label="回购获批额度（元）" inputMode="decimal" placeholder="如 900.00" value={repurchaseBudgetYuan} onChange={(event) => setRepurchaseBudgetYuan(event.currentTarget.value)} /></label>
            <label>数量上限（股）<input aria-label="回购数量上限（股）" inputMode="numeric" placeholder="如 100" value={repurchaseMaxShares} onChange={(event) => setRepurchaseMaxShares(event.currentTarget.value)} /></label>
            <label>窗口（交易日数）<input aria-label="回购窗口交易日数" inputMode="numeric" value={repurchaseWindowDays} onChange={(event) => setRepurchaseWindowDays(event.currentTarget.value)} /></label>
            <label>用途
              <select aria-label="回购用途" value={repurchasePurpose} onChange={(event) => setRepurchasePurpose(event.currentTarget.value as typeof repurchasePurpose)}>
                <option value="ValueMaintenance">维护公司价值及股东权益</option>
                <option value="ReduceCapital">减少注册资本（注销）</option>
                <option value="EmployeeIncentive">员工持股计划或股权激励</option>
                <option value="ConvertibleConversion">转换可转债</option>
              </select>
            </label>
          </>
        )}
        {category === "ShareSplit" && (
          <>
            <label>方向
              <select aria-label="拆股／缩股方向" value={splitDirection} onChange={(event) => setSplitDirection(event.currentTarget.value as typeof splitDirection)}>
                <option value="Split">拆股（1 股换 N 股）</option>
                <option value="Consolidate">缩股（N 股换 1 股）</option>
              </select>
            </label>
            <label>整数比例（≥ 2）<input aria-label="拆股／缩股整数比例" inputMode="numeric" placeholder="如 2" value={splitRatio} onChange={(event) => setSplitRatio(event.currentTarget.value)} /></label>
          </>
        )}
        <button type="button" disabled={submitting} onClick={submit}>{submitting ? "提交中…" : "提交提案"}</button>
        {error !== null && <p className="company-state is-error" role="alert">提案未提交：{error}</p>}
        {result !== null && result.outcome === "accepted" && (
          <p className="company-state" role="status">已受理：{proposalKindLabel(result.kind)} 方案 <span className="mono">{result.identity}</span>（批准 {result.approved_on}、公告 {result.announced_on}），按既有状态机推进。</p>
        )}
        {result !== null && result.outcome === "no_holding" && (
          <p className="company-state is-error" role="alert">无持仓拒绝：{result.detail}</p>
        )}
        {result !== null && result.outcome === "institutional_rejection" && (
          <p className="company-state is-error" role="alert">制度拒绝（{proposalClassLabel(result.class)}）：{result.detail}</p>
        )}
      </div>
    </section>
  );
}

interface InGamePreferencesPanelProps {
  readonly companyId: string;
  /** 偏好局内编辑；undefined 表示宿主明确不支持，面板显式提示。 */
  readonly onPreferencesUpdate: ((company: string, preferences: SimpleCompanyPreferences) => Promise<void>) | undefined;
  /** 当前偏好查询（编辑初值）；undefined 表示宿主明确不支持。 */
  readonly onPreferencesQuery: ((company: string) => Promise<SimpleCompanyPreferences>) | undefined;
  /** 刷新键：保存成功后重查。 */
  readonly refreshKey: string;
}

/**
 * 偏好局内编辑（局内版 CompanyPreferencesInput 形态）：任意时刻可改，下一
 * 结算周期末日评估生效；同周期已产生的提案不回滚。非法输入不写入草稿。
 */
export function InGamePreferencesPanel({ companyId, onPreferencesUpdate, onPreferencesQuery, refreshKey }: InGamePreferencesPanelProps) {
  const [preferences, preferencesError, preferencesLoading, refreshPreferences] = useQueried(
    onPreferencesQuery === undefined ? undefined : () => onPreferencesQuery(companyId),
    `${refreshKey}:${companyId}`,
  );
  const [draft, setDraft] = useState<SimpleCompanyPreferences | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [fieldError, setFieldError] = useState<string | null>(null);

  useEffect(() => {
    setDraft(null);
    setSaveError(null);
    setSaved(false);
    setFieldError(null);
  }, [companyId, refreshKey]);

  useEffect(() => {
    if (draft === null && preferences !== null) setDraft(preferences);
  }, [draft, preferences]);

  if (onPreferencesUpdate === undefined || onPreferencesQuery === undefined) {
    return (
      <section className="in-game-preferences" aria-label="公司行为偏好（局内）">
        <h4>公司行为偏好（局内）</h4>
        <p className="company-state">当前宿主不支持偏好局内编辑。</p>
      </section>
    );
  }

  const apply = (next: SimpleCompanyPreferences) => {
    setFieldError(null);
    setSaved(false);
    setDraft(next);
  };

  const save = () => {
    if (draft === null) return;
    setSaving(true);
    setSaveError(null);
    setSaved(false);
    onPreferencesUpdate(companyId, draft).then(
      () => {
        setSaving(false);
        setSaved(true);
        refreshPreferences();
      },
      (failure) => {
        setSaving(false);
        setSaveError(failure instanceof Error ? failure.message : String(failure));
      },
    );
  };

  return (
    <section className="in-game-preferences" aria-label="公司行为偏好（局内）">
      <h4>公司行为偏好（局内）</h4>
      <p className="company-note">修改即时生效保存，下一结算周期末日评估；同周期内已产生的提案不回滚。未配置 = 不自动产生该类方案。</p>
      {preferencesLoading && <p className="company-state" role="status">正在读取当前偏好…</p>}
      {preferencesError !== null && <p className="company-state is-error" role="alert">偏好查询失败：{preferencesError}</p>}
      {draft !== null && (
        <fieldset className="company-preference-group">
          <legend>现金分红</legend>
          <label>
            <input type="checkbox" aria-label="启用现金分红自动提案" checked={draft.cash_dividend !== null}
              onChange={(event) => apply({ ...draft, cash_dividend: event.currentTarget.checked ? { target_payout_bp: 3000, min_distributable_profit: "100000000", cycles_between_proposals: 1 } : null })} />
            启用现金分红自动提案
          </label>
          {draft.cash_dividend !== null && (
            <>
              <label>目标派息比例（基点）<input aria-label="局内目标派息比例（基点）" inputMode="numeric" value={String(draft.cash_dividend.target_payout_bp)}
                onChange={(event) => {
                  const value = Number(event.currentTarget.value);
                  if (Number.isSafeInteger(value) && value >= 1 && value <= 10000) apply({ ...draft, cash_dividend: { ...draft.cash_dividend!, target_payout_bp: value } });
                  else setFieldError("目标派息比例必须在 1..=10000 基点；非法输入不写入草稿。");
                }} /></label>
              <label>最小可分配利润门槛（元）<input aria-label="局内最小可分配利润门槛（元）" inputMode="decimal" value={centsToYuanText(draft.cash_dividend.min_distributable_profit)}
                onChange={(event) => {
                  const text = event.currentTarget.value.trim();
                  if (/^\d+(\.\d{1,2})?$/.test(text)) {
                    const [whole, fraction = ""] = text.split(".");
                    const cents = `${whole}${(fraction + "00").slice(0, 2)}`.replace(/^0+(?=\d)/, "");
                    if (cents !== "0") apply({ ...draft, cash_dividend: { ...draft.cash_dividend!, min_distributable_profit: cents } });
                    else setFieldError("最小可分配利润门槛必须为正数；非法输入不写入草稿。");
                  } else setFieldError("最小可分配利润门槛（元）必须是正数且最多两位小数；非法输入不写入草稿。");
                }} /></label>
              <label>提案最小间隔（结算周期数）<input aria-label="局内现金分红提案最小间隔" inputMode="numeric" value={String(draft.cash_dividend.cycles_between_proposals)}
                onChange={(event) => {
                  const value = Number(event.currentTarget.value);
                  if (Number.isSafeInteger(value) && value >= 1 && value <= 65535) apply({ ...draft, cash_dividend: { ...draft.cash_dividend!, cycles_between_proposals: value } });
                  else setFieldError("提案最小间隔必须是 1..=65535 的整数；非法输入不写入草稿。");
                }} /></label>
            </>
          )}
        </fieldset>
      )}
      {draft !== null && (
        <fieldset className="company-preference-group">
          <legend>送转</legend>
          <label>
            <input type="checkbox" aria-label="启用送转自动提案" checked={draft.stock_distribution !== null}
              onChange={(event) => apply({ ...draft, stock_distribution: event.currentTarget.checked ? { min_distributable_profit: "100000000", shares_per_existing_share_micros: 100000, max_cumulative_expansion_micros: 10000000, cycles_between_proposals: 4 } : null })} />
            启用送转自动提案
          </label>
          {draft.stock_distribution !== null && <small>自动提案固定采用送股，不自动提案转增。</small>}
        </fieldset>
      )}
      {fieldError !== null && <p className="company-state is-error" role="alert">{fieldError}</p>}
      <button type="button" disabled={saving || draft === null} onClick={save}>{saving ? "保存中…" : "保存偏好修改"}</button>
      <button type="button" disabled={preferencesLoading} onClick={refreshPreferences}>重新读取当前偏好</button>
      {saveError !== null && <p className="company-state is-error" role="alert">偏好保存失败：{saveError}</p>}
      {saved && <p className="company-state" role="status">已保存：下一结算周期末日评估生效。</p>}
    </section>
  );
}
