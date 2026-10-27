/* oxlint-disable react/only-export-components -- 字段换算/校验 helper 供 SSR 测试与输入组件共用 */
import { useState } from "react";
import type { CompanySystemConfig } from "../../types/generated/CompanySystemConfig.ts";
import type { SimpleCashDividendPreference } from "../../types/generated/SimpleCashDividendPreference.ts";
import type { SimpleCompanyPreferences } from "../../types/generated/SimpleCompanyPreferences.ts";
import type { SimpleStockDistributionPreference } from "../../types/generated/SimpleStockDistributionPreference.ts";

export type CompanyPreferencesCompany = Extract<CompanySystemConfig, { mode: "Simple" }>["config"]["companies"][number];

interface CompanyPreferencesInputProps {
  /** 当前解析成功的公司配置清单；null 表示 JSON 当前无效（显式错误由外层展示）。 */
  readonly companies: readonly CompanyPreferencesCompany[] | null;
  /** 应用某公司的偏好修改（由上层写入配置 JSON 草稿并处理校验错误）。 */
  readonly onApply: (company: number, preferences: SimpleCompanyPreferences) => void;
}

type FieldResult<T> = { readonly ok: true; readonly value: T } | { readonly ok: false; readonly error: string };

/** 分（规范十进制字符串）→ 元文本（两位小数），仅用于展示换算。 */
export function centsToYuanText(cents: string): string {
  const negative = cents.startsWith("-");
  const digits = negative ? cents.slice(1) : cents;
  const padded = digits.padStart(3, "0");
  return `${negative ? "-" : ""}${padded.slice(0, -2)}.${padded.slice(-2)}`;
}

/** 元文本（最多两位小数）→ 规范分字符串（去前导零）；非元格式返回 null。 */
function yuanTextToCents(text: string): string | null {
  const trimmed = text.trim();
  if (!/^\d+(\.\d{1,2})?$/.test(trimmed)) return null;
  const [whole, fraction = ""] = trimmed.split(".");
  return `${whole}${(fraction + "00").slice(0, 2)}`.replace(/^0+(?=\d)/, "");
}

function boundedIntegerField(text: string, minimum: number, maximum: number, label: string): FieldResult<number> {
  const trimmed = text.trim();
  if (!/^\d+$/.test(trimmed)) return { ok: false, error: `${label}必须是整数字符串` };
  const value = Number(trimmed);
  if (!Number.isSafeInteger(value) || value < minimum || value > maximum) return { ok: false, error: `${label}必须在 ${minimum}..=${maximum}` };
  return { ok: true, value };
}

/** 现金分红偏好表单字段 → 偏好值；越域或非数字显式返回错误，不静默钳制。 */
export function cashPreferenceFromFields(payoutBp: string, minYuan: string, cycles: string): FieldResult<SimpleCashDividendPreference> {
  const payout = boundedIntegerField(payoutBp, 1, 10000, "目标派息比例（基点）");
  if (!payout.ok) return payout;
  const cycle = boundedIntegerField(cycles, 1, 65535, "提案最小间隔（结算周期数）");
  if (!cycle.ok) return cycle;
  const cents = yuanTextToCents(minYuan);
  if (cents === null || cents === "0" || BigInt(cents) > 9223372036854775807n) return { ok: false, error: "最小可分配利润门槛（元）必须是正数、最多两位小数且不超过 i64 Money 上限" };
  return { ok: true, value: { target_payout_bp: payout.value, min_distributable_profit: cents, cycles_between_proposals: cycle.value } };
}

/** 送转偏好表单字段 → 偏好值；越域或非数字显式返回错误，不静默钳制。 */
export function stockPreferenceFromFields(minYuan: string, ratioMicros: string, expansionMicros: string, cycles: string): FieldResult<SimpleStockDistributionPreference> {
  const ratio = boundedIntegerField(ratioMicros, 1, 10000000, "每股送转比例（百万分之一股/股）");
  if (!ratio.ok) return ratio;
  const expansion = boundedIntegerField(expansionMicros, 1, 1000000000, "累计股本扩张上限（百万分比）");
  if (!expansion.ok) return expansion;
  const cycle = boundedIntegerField(cycles, 1, 65535, "提案最小间隔（结算周期数）");
  if (!cycle.ok) return cycle;
  const cents = yuanTextToCents(minYuan);
  if (cents === null || cents === "0" || BigInt(cents) > 9223372036854775807n) return { ok: false, error: "触发门槛（元）必须是正数、最多两位小数且不超过 i64 Money 上限" };
  return { ok: true, value: { min_distributable_profit: cents, shares_per_existing_share_micros: ratio.value, max_cumulative_expansion_micros: expansion.value, cycles_between_proposals: cycle.value } };
}

/** 启用时的初始可编辑值（游戏化预设，非行业统计；用户可继续修改）。 */
const DEFAULT_CASH_PREFERENCE: SimpleCashDividendPreference = { target_payout_bp: 3000, min_distributable_profit: "100000000", cycles_between_proposals: 1 };

/**
 * 新局/装配期的公司行为偏好编辑入口（ADR-0037；SimpleCompanyPreferences）：
 * 每公司显式配置现金分红/送转偏好；未勾选=未配置=不自动产生该类方案。
 * 字段非法时显式提示且不写入草稿；偏好随严格存档固化，仅对新游戏生效。
 */
export function CompanyPreferencesInput({ companies, onApply }: CompanyPreferencesInputProps) {
  if (companies === null) {
    return (
      <fieldset className="company-preferences-input" aria-label="公司行为偏好">
        <legend>公司行为偏好（自动提案）</legend>
        <p role="alert">偏好编辑暂不可用：公司配置 JSON 当前无效，请先修正后再编辑偏好。</p>
      </fieldset>
    );
  }
  return (
    <fieldset className="company-preferences-input" aria-label="公司行为偏好">
      <legend>公司行为偏好（自动提案）</legend>
      <p>未配置（不勾选）= 无偏好 = 不自动产生该类方案；偏好只表示配置意向，不承诺尚未交付的日终自动提案。仅对新游戏生效，随存档严格固化。</p>
      {companies.map((entry) => (
        <div key={entry.company} className="company-preferences-company">
          <h5 className="mono">{entry.company}</h5>
          <CashPreferenceControls company={entry.company} preferences={entry.preferences} onApply={onApply} />
          <StockPreferenceControls company={entry.company} preferences={entry.preferences} onApply={onApply} />
        </div>
      ))}
    </fieldset>
  );
}


interface ControlsProps {
  readonly company: number;
  readonly preferences: SimpleCompanyPreferences;
  readonly onApply: (company: number, preferences: SimpleCompanyPreferences) => void;
}

type RawFields = Record<string, string>;

/** 本地保留未完成编辑；仅全部合法后回写，外部有效配置变化才重置字段。 */
function usePreferenceFields(source: unknown, initial: RawFields, validate: (fields: RawFields) => FieldResult<SimpleCompanyPreferences>, props: ControlsProps) {
  const sourceKey = JSON.stringify(source);
  const [state, setState] = useState({ sourceKey, fields: initial, error: null as string | null });
  const current = state.sourceKey === sourceKey ? state : { sourceKey, fields: initial, error: null };
  if (current !== state) setState(current);
  function edit(key: string, raw: string) {
    const fields = { ...current.fields, [key]: raw };
    const result = validate(fields);
    if (!result.ok) { setState({ sourceKey, fields, error: result.error }); return; }
    try {
      props.onApply(props.company, result.value);
      setState({ sourceKey, fields, error: null });
    } catch (failure) {
      setState({ sourceKey, fields, error: failure instanceof Error ? failure.message : String(failure) });
    }
  }
  return { ...current, edit };
}

function CashPreferenceControls(props: ControlsProps) {
  const { company, preferences, onApply } = props;
  const preference = preferences.cash_dividend;
  const controls = usePreferenceFields(preference, {
    payout: preference === null ? "" : String(preference.target_payout_bp),
    minimum: preference === null ? "" : centsToYuanText(preference.min_distributable_profit),
    cycles: preference === null ? "" : String(preference.cycles_between_proposals),
  }, fields => {
    const result = cashPreferenceFromFields(fields.payout!, fields.minimum!, fields.cycles!);
    return result.ok ? { ok: true, value: { ...preferences, cash_dividend: result.value } } : result;
  }, props);
  return <fieldset><legend>现金分红</legend>
    <label><input type="checkbox" name={`cash-preference-${company}`} checked={preference !== null} onChange={event => onApply(company, { ...preferences, cash_dividend: event.currentTarget.checked ? { ...DEFAULT_CASH_PREFERENCE } : null })} />启用现金分红自动提案</label>
    <small>启用预设为虚拟可编辑参数：30%、100 万元、每结算周期；非行业统计。</small>
    {preference !== null && <>
      {[["payout", "目标派息比例（基点）"], ["minimum", "最小可分配利润门槛（元）"], ["cycles", "提案最小间隔（结算周期数）"]].map(([key, label]) => <label key={key}>{label}<input aria-label={label} inputMode={key === "minimum" ? "decimal" : "numeric"} value={controls.fields[key!]} aria-invalid={controls.error !== null} onChange={event => controls.edit(key!, event.currentTarget.value)} /></label>)}
    </>}
    {controls.error !== null && <p role="alert">现金分红偏好无效：{controls.error}；未写入配置，请修正输入。</p>}
  </fieldset>;
}

function StockPreferenceControls(props: ControlsProps) {
  const { company, preferences, onApply } = props;
  const preference = preferences.stock_distribution;
  const sourceKey = JSON.stringify(preference);
  const [editingState, setEditingState] = useState({ sourceKey, editing: preference !== null });
  const editing = editingState.sourceKey === sourceKey ? editingState.editing : preference !== null;
  if (editingState.sourceKey !== sourceKey) setEditingState({ sourceKey, editing });
  const controls = usePreferenceFields(preference, {
    minimum: preference === null ? "" : centsToYuanText(preference.min_distributable_profit),
    ratio: preference === null ? "" : String(preference.shares_per_existing_share_micros),
    expansion: preference === null ? "" : String(preference.max_cumulative_expansion_micros),
    cycles: preference === null ? "" : String(preference.cycles_between_proposals),
  }, fields => {
    const result = stockPreferenceFromFields(fields.minimum!, fields.ratio!, fields.expansion!, fields.cycles!);
    return result.ok ? { ok: true, value: { ...preferences, stock_distribution: result.value } } : result;
  }, props);
  return <fieldset><legend>送转</legend>
    <label><input type="checkbox" name={`stock-preference-${company}`} checked={editing} onChange={event => {
      setEditingState({ sourceKey, editing: event.currentTarget.checked });
      if (!event.currentTarget.checked && preference !== null) onApply(company, { ...preferences, stock_distribution: null });
    }} />启用送转自动提案</label>
    <small>未配置则不自动提案；自动提案仅送股，转增不支持。启用后需明确填写全部字段，未完成前仍为未配置。</small>
    {editing && <>
      {[["minimum", "触发门槛（元）"], ["ratio", "每股送转比例（百万分之一股/股）"], ["expansion", "累计股本扩张上限（百万分比）"], ["cycles", "送转提案最小间隔（结算周期数）"]].map(([key, label]) => <label key={key}>{label}<input aria-label={label} inputMode={key === "minimum" ? "decimal" : "numeric"} value={controls.fields[key!]} aria-invalid={controls.error !== null} onChange={event => controls.edit(key!, event.currentTarget.value)} /></label>)}
    </>}
    {editing && (controls.error !== null || preference === null) && <p role="alert">送转偏好编辑未完成：{controls.error === null ? "请填写全部字段；当前仍为未配置" : controls.error}；未写入配置，请修正输入。</p>}
  </fieldset>;
}
