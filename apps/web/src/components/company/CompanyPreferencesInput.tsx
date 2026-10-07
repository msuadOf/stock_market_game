/* oxlint-disable react/only-export-components -- 字段换算/校验 helper 供 SSR 测试与输入组件共用 */
import type { CompanySystemConfig } from "../../types/generated/CompanySystemConfig.ts";
import type { SimpleCashDividendPreference } from "../../types/generated/SimpleCashDividendPreference.ts";
import type { SimpleCompanyPreferences } from "../../types/generated/SimpleCompanyPreferences.ts";
import type { SimpleStockDistributionPreference } from "../../types/generated/SimpleStockDistributionPreference.ts";

export type CompanyPreferencesCompany = Extract<CompanySystemConfig, { mode: "Simple" }>["config"]["companies"][number];

interface CompanyPreferencesInputProps {
  /** 当前解析成功的公司配置清单；null 表示 JSON 当前无效（显式错误由外层展示）。 */
  readonly companies: readonly CompanyPreferencesCompany[] | null;
  /** 应用某公司的偏好修改（由上层写入配置 JSON 草稿并处理校验错误）。 */
  readonly onApply: (company: string, preferences: SimpleCompanyPreferences) => void;
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
  if (cents === null || cents === "0") return { ok: false, error: "最小可分配利润门槛（元）必须是正数且最多两位小数" };
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
  if (cents === null || cents === "0") return { ok: false, error: "触发门槛（元）必须是正数且最多两位小数" };
  return { ok: true, value: { min_distributable_profit: cents, shares_per_existing_share_micros: ratio.value, max_cumulative_expansion_micros: expansion.value, cycles_between_proposals: cycle.value } };
}

/** 启用时的初始可编辑值（游戏化预设，非行业统计；用户可继续修改）。 */
const DEFAULT_CASH_PREFERENCE: SimpleCashDividendPreference = { target_payout_bp: 3000, min_distributable_profit: "100000000", cycles_between_proposals: 1 };
const DEFAULT_STOCK_PREFERENCE: SimpleStockDistributionPreference = { min_distributable_profit: "100000000", shares_per_existing_share_micros: 100000, max_cumulative_expansion_micros: 10000000, cycles_between_proposals: 4 };

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
      <p>未配置（不勾选）= 无偏好 = 不自动产生该类方案；结算周期末日日结按配置评估，被制度拒绝的提案如实记入台账。仅对新游戏生效，随存档严格固化。</p>
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

function CashPreferenceControls({ company, preferences, onApply }: {
  readonly company: string;
  readonly preferences: SimpleCompanyPreferences;
  readonly onApply: (company: string, preferences: SimpleCompanyPreferences) => void;
}) {
  const preference = preferences.cash_dividend;
  const enabled = preference !== null;
  return (
    <fieldset className="company-preference-group">
      <legend>现金分红</legend>
      <label>
        <input
          type="checkbox"
          name={`cash-preference-${company}`}
          checked={enabled}
          onChange={(event) => onApply(company, { ...preferences, cash_dividend: event.currentTarget.checked ? DEFAULT_CASH_PREFERENCE : null })}
        />
        启用现金分红自动提案
      </label>
      {!enabled && <small>未配置：不自动产生现金分红方案。</small>}
      {enabled && preference !== null && (
        <>
          <label>目标派息比例（基点）<input aria-label="目标派息比例（基点）" inputMode="numeric" value={String(preference.target_payout_bp)} onChange={(event) => {
            const result = cashPreferenceFromFields(event.currentTarget.value, centsToYuanText(preference.min_distributable_profit), String(preference.cycles_between_proposals));
            if (result.ok) onApply(company, { ...preferences, cash_dividend: { ...preference, target_payout_bp: result.value.target_payout_bp } });
          }} /></label>
          <label>最小可分配利润门槛（元）<input aria-label="最小可分配利润门槛（元）" inputMode="decimal" value={centsToYuanText(preference.min_distributable_profit)} onChange={(event) => {
            const result = cashPreferenceFromFields(String(preference.target_payout_bp), event.currentTarget.value, String(preference.cycles_between_proposals));
            if (result.ok) onApply(company, { ...preferences, cash_dividend: { ...preference, min_distributable_profit: result.value.min_distributable_profit } });
          }} /></label>
          <label>提案最小间隔（结算周期数）<input aria-label="提案最小间隔（结算周期数）" inputMode="numeric" value={String(preference.cycles_between_proposals)} onChange={(event) => {
            const result = cashPreferenceFromFields(String(preference.target_payout_bp), centsToYuanText(preference.min_distributable_profit), event.currentTarget.value);
            if (result.ok) onApply(company, { ...preferences, cash_dividend: { ...preference, cycles_between_proposals: result.value.cycles_between_proposals } });
          }} /></label>
          <small>非法输入不会写入草稿；请修正后再继续。</small>
        </>
      )}
    </fieldset>
  );
}

function StockPreferenceControls({ company, preferences, onApply }: {
  readonly company: string;
  readonly preferences: SimpleCompanyPreferences;
  readonly onApply: (company: string, preferences: SimpleCompanyPreferences) => void;
}) {
  const preference = preferences.stock_distribution;
  const enabled = preference !== null;
  return (
    <fieldset className="company-preference-group">
      <legend>送转</legend>
      <label>
        <input
          type="checkbox"
          name={`stock-preference-${company}`}
          checked={enabled}
          onChange={(event) => onApply(company, { ...preferences, stock_distribution: event.currentTarget.checked ? DEFAULT_STOCK_PREFERENCE : null })}
        />
        启用送转自动提案
      </label>
      {!enabled && <small>未配置：不自动产生送转方案（自动提案固定采用送股，不自动提案转增）。</small>}
      {enabled && preference !== null && (
        <>
          <label>触发门槛（元）<input aria-label="触发门槛（元）" inputMode="decimal" value={centsToYuanText(preference.min_distributable_profit)} onChange={(event) => {
            const result = stockPreferenceFromFields(event.currentTarget.value, String(preference.shares_per_existing_share_micros), String(preference.max_cumulative_expansion_micros), String(preference.cycles_between_proposals));
            if (result.ok) onApply(company, { ...preferences, stock_distribution: { ...preference, min_distributable_profit: result.value.min_distributable_profit } });
          }} /></label>
          <label>每股送转比例（百万分之一股/股）<input aria-label="每股送转比例（百万分之一股/股）" inputMode="numeric" value={String(preference.shares_per_existing_share_micros)} onChange={(event) => {
            const result = stockPreferenceFromFields(centsToYuanText(preference.min_distributable_profit), event.currentTarget.value, String(preference.max_cumulative_expansion_micros), String(preference.cycles_between_proposals));
            if (result.ok) onApply(company, { ...preferences, stock_distribution: { ...preference, shares_per_existing_share_micros: result.value.shares_per_existing_share_micros } });
          }} /></label>
          <label>累计股本扩张上限（百万分比）<input aria-label="累计股本扩张上限（百万分比）" inputMode="numeric" value={String(preference.max_cumulative_expansion_micros)} onChange={(event) => {
            const result = stockPreferenceFromFields(centsToYuanText(preference.min_distributable_profit), String(preference.shares_per_existing_share_micros), event.currentTarget.value, String(preference.cycles_between_proposals));
            if (result.ok) onApply(company, { ...preferences, stock_distribution: { ...preference, max_cumulative_expansion_micros: result.value.max_cumulative_expansion_micros } });
          }} /></label>
          <label>提案最小间隔（结算周期数）<input aria-label="送转提案最小间隔（结算周期数）" inputMode="numeric" value={String(preference.cycles_between_proposals)} onChange={(event) => {
            const result = stockPreferenceFromFields(centsToYuanText(preference.min_distributable_profit), String(preference.shares_per_existing_share_micros), String(preference.max_cumulative_expansion_micros), event.currentTarget.value);
            if (result.ok) onApply(company, { ...preferences, stock_distribution: { ...preference, cycles_between_proposals: result.value.cycles_between_proposals } });
          }} /></label>
          <small>非法输入不会写入草稿；请修正后再继续。</small>
        </>
      )}
    </fieldset>
  );
}
