import { parseCompanySystemConfig } from "../../save/schema/company/system-config.ts";
import { CompanyPreferencesInput, type CompanyPreferencesCompany } from "./CompanyPreferencesInput.tsx";
import type { SimpleCompanyPreferences } from "../../types/generated/SimpleCompanyPreferences.ts";
import type { SettlementCycle } from "../../types/generated/SettlementCycle";

export function CompanySystemInput({ value, onChange, seed, origin, onSeedChange, onRegenerate, onSettlementCycleChange, onPreferencesChange }: {
  readonly value: string;
  readonly onChange: (value: string) => void;
  readonly seed: string;
  readonly origin: "preset" | "custom";
  readonly onSeedChange: (seed: string) => void;
  readonly onRegenerate: () => void;
  readonly onSettlementCycleChange: (cycle: SettlementCycle) => void;
  /** 偏好编辑入口（可选：远程创建等宿主未接线时省略，面板显式说明由 JSON 编辑）。 */
  readonly onPreferencesChange?: (company: string, preferences: SimpleCompanyPreferences) => void;
}) {
  let error: string | null = null;
  let cycle: SettlementCycle | null = null;
  let companies: readonly CompanyPreferencesCompany[] | null = null;
  try {
    const config = parseCompanySystemConfig(JSON.parse(value));
    cycle = config.config.settlement_cycle;
    companies = config.config.companies;
  }
  catch (failure) { error = failure instanceof Error ? failure.message : String(failure); }
  const seedValid = /^(0|[1-9]\d*)$/.test(seed) && BigInt(seed) <= (1n << 64n) - 1n;
  return <fieldset aria-label="公司基本面系统设置">
    <legend>公司基本面系统：Simple</legend>
    <p>虚拟简化模型，不使用真实市场统计。整局选定后不能切换；Simulation 尚未实现，不能创建该模式。</p>
    <p>可选择月、季度、半年或年度结算，使用年化持续趋势与独立期间扰动生成营收和开支，再由一致财务状态计算利润。财务查询、披露和股本行为使用共同规则；Simple 不需要具体客户、工厂或研发项目。</p>
    <p>Simple 只生成账面汇总财务，不建立真实公司资金或 SyntheticFunding；股本行为遵守共同适用规则，不能因账面现金不足而把公司功能整体禁用。</p>
    <label>基本面结算周期<select aria-label="基本面结算周期" value={cycle === null ? "" : cycle} disabled={origin === "custom" || cycle === null} onChange={(event) => {
      const selected = event.currentTarget.value;
      if (!["Monthly", "Quarterly", "HalfYear", "Annual"].includes(selected)) throw new Error(`无效结算周期 ${selected}`);
      onSettlementCycleChange(selected as SettlementCycle);
    }}><option value="" disabled>请先完善配置</option><option value="Monthly">自然月</option><option value="Quarterly">自然季度</option><option value="HalfYear">自然半年</option><option value="Annual">自然年</option></select></label>
    <p>预设切换周期保留同一 seed 并重新计算相同跨度的初始营收和开支，不改变公开排期。本人编辑的配置请在 JSON 中同时设置周期及对应金额，或明确重新生成预设后再切换，不静默覆盖编辑内容。</p>
    <p>初始虚拟预设以开局价格和总股本形成规模基准：虚拟 PE 在10–30之间，PB在1–3之间；使用20%税前盈余率、25%所得税参数倒出相应期间营收，固定开支取营收20%、变动开支60%。这些不是真实市场统计，也不是承诺的未来利润。期初权益以账面应收对应资本，不创建真实公司现金。后续只按公司配置演变，不再反推成交价格，不保证开盘不跳变。</p>
    <label>本局 seed<input aria-label="公司配置与运行统一 seed" inputMode="numeric" value={seed} onChange={(event) => onSeedChange(event.currentTarget.value)} aria-invalid={!seedValid} /></label>
    <p>当前配置来源：{origin === "preset" ? "虚拟预设" : "本人编辑"}。创建使用已展示的 seed，不重新抽取；本人编辑的 JSON 不会因 seed 改动被自动覆盖。</p>
    <button type="button" onClick={onRegenerate}>保留合法参数，重新生成 seed 与初值</button>
    <p>JSON 尚未完成时请先修正后再生成；不会自动丢弃本人编辑或用默认值掩盖无效配置。</p>
    {onPreferencesChange !== undefined
      ? <CompanyPreferencesInput companies={companies} onApply={onPreferencesChange} />
      : <fieldset className="company-preferences-input" aria-label="公司行为偏好"><legend>公司行为偏好（自动提案）</legend><p className="company-state">当前创建入口未接入偏好编辑；偏好仍可在下方 JSON 的 companies[].preferences 中显式配置（未配置=不自动产生方案）。</p></fieldset>}
    {!seedValid && <p role="alert">seed 必须为 0 至 u64::MAX 的规范十进制整数字符串。</p>}
    <details>
      <summary>编辑公司基本面参数</summary>
      <label>Simple 配置（完整 JSON）<textarea aria-label="Simple 公司配置 JSON" rows={16} value={value} onChange={(event) => onChange(event.currentTarget.value)} spellCheck={false} /></label>
      <dl>
        <div><dt>金额</dt><dd>会计金额使用两位小数元字符串，交易与投资者账户 Money 使用十进制分字符串；不混用单位。</dd></div>
        <div><dt>变化率</dt><dd>所有 *_bp 字段使用整数基点，100 bp = 1%；收入与开支分别变化，利润不得独立抽取或覆盖。</dd></div>
        <div><dt>环境</dt><dd>initial_change_bp 为初始年化需求变化；persistence_bp 为上期变化保留比例；noise 按月、季度、半年、年分别配置有界扰动。环境只进入营收趋势，不直接设置利润或成交价格。</dd></div>
        <div><dt>每家公司</dt><dd>company 需与当前发行人身份逐一匹配；营收、固定开支和变动开支参数均须明确配置。参数与财务期初不一致时拒绝，不自动补平。</dd></div>
        <div><dt>偏好</dt><dd>companies[].preferences 为公司行为偏好（ADR-0037）：cash_dividend／stock_distribution 未配置（null）即不自动产生该类方案；数值域由严格 parser 校验，非法值显式拒绝。</dd></div>
        <div><dt>日期</dt><dd>settlement_cycle 选择基本面结算周期；prehistory_periods 表示同长度虚拟前史期间数量。初始营收和开支属于完整所选期间，不能把月金额当年度金额。报告公开日期独立使用财报公开频率设置；较长结算下未形成的更短报告窗口明确不可用，不平均摊造。虚拟前史不产生证券成交。</dd></div>
        <div><dt>趋势与扰动</dt><dd>revenue_trend／fixed_expense_trend 是年化基点，Persistent 使用自然月持续时间；期间内趋势变化按实际持续时间复合。各 noise 的 monthly_bp／quarterly_bp／half_year_bp／annual_bp 独立，不声称真实市场波动率换算。</dd></div>
      </dl>
    </details>
    {error !== null && <p role="alert">公司基本面设置无效：{error}；请修正后再创建新游戏。</p>}
  </fieldset>;
}
