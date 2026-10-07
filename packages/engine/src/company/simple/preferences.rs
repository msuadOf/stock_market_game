//! Simple 模型内的公司行为偏好（ADR-0037：偏好自动方案 + 显式 API 并存）。
//!
//! 偏好属于模型内部：只在此文件做「提案评估」——按整数/定点比较构造**显式方案**，
//! 由 Session 侧在与显式 API 完全相同的 `approve_cash_dividend` /
//! `approve_stock_distribution` 入口提交，进入同一 announce/登记/入账状态机与
//! 同一制度校验（可分配利润、法定公积金、注册资本法定事实、真实名册）。
//! 偏好不绕过任何校验；提案被制度拒绝时由 [`SimplePreferenceLedger`] 如实记录，
//! 同一结算周期不重试。未配置偏好（两项皆 `None`）= 不自动产生任何方案，
//! 这是安全默认，不是隐含的「默认经营风格」。
//!
//! 评估只在结算周期末日结算完成后进行（Session 日结候选事务内），每周期每公司
//! 每类别至多评估一次，全部为 O(偏好项) 的整数比较与少量既有事实扫描；
//! 不引入每 tick 的堆分配或全量克隆。

use crate::accounting::AccountingAmount;
use crate::calendar::{CalendarExchange, CivilDate, TradingCalendar};
use crate::company::dividend::DistributableProfit;
use crate::company::{CompanyId, DividendDeclaration};
use crate::money::Money;
use crate::{account::StockCode, company::cash_dividend::CashDividendPlan};

/// 送转比例的分母：比例字段以「百万分之一股/股」表达（与
/// `StockDistributionEventPlan::shares_per_existing_share_micros` 同单位）。
pub(crate) const SHARE_RATIO_DENOMINATOR: u64 = 1_000_000;

/// 基点分母（1bp = 万分之一 = 0.01%），与既有 `persistence_bp` 等字段同约定。
pub(crate) const BASIS_POINT_DENOMINATOR: i128 = 10_000;

/// 每公司行为偏好配置。严格持久化字段：无 serde 默认，旧档缺失显式拒绝。
///
/// 类型扩展位：配股（rights）、增发（offering）、回购（buyback）的偏好项由
/// 对应机制批次落地时在此结构追加字段（如 `pub rights: Option<...>`）；本批
/// 显式不实现其机制，也不预留无语义的占位类型，避免伪造未实现机制的配置面。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct SimpleCompanyPreferences {
    /// 现金分红偏好项；`None` = 不启用（不自动产生现金分红方案）。
    pub cash_dividend: Option<SimpleCashDividendPreference>,
    /// 送转偏好项；`None` = 不启用（不自动产生送转方案）。
    pub stock_distribution: Option<SimpleStockDistributionPreference>,
}

impl SimpleCompanyPreferences {
    /// 未配置任何偏好的空配置：不自动产生任何方案。
    pub fn none() -> Self {
        Self {
            cash_dividend: None,
            stock_distribution: None,
        }
    }

    /// 配置域校验（进入 `SimpleConfig::validate_for_issuers`）。
    pub fn validate(&self) -> Result<(), crate::company::CompanySystemError> {
        use crate::company::CompanySystemError;
        if let Some(cash) = &self.cash_dividend {
            if !(1..=10_000).contains(&cash.target_payout_bp) {
                return Err(CompanySystemError::Invalid(
                    "现金分红偏好目标派息比例必须为 1..=10000 bp".into(),
                ));
            }
            if cash.min_distributable_profit.cents() < 1 {
                return Err(CompanySystemError::Invalid(
                    "现金分红偏好最小可分配利润门槛必须为正数（分）".into(),
                ));
            }
            if cash.cycles_between_proposals == 0 {
                return Err(CompanySystemError::Invalid(
                    "现金分红偏好提案间隔周期数必须 >= 1".into(),
                ));
            }
        }
        if let Some(stock) = &self.stock_distribution {
            if stock.min_distributable_profit.cents() < 1 {
                return Err(CompanySystemError::Invalid(
                    "送转偏好最小可分配利润门槛必须为正数（分）".into(),
                ));
            }
            if !(1..=10_000_000).contains(&stock.shares_per_existing_share_micros) {
                return Err(CompanySystemError::Invalid(
                    "送转偏好每股送转比例必须为 1..=10000000 百万分比（不超过 10 送 100）".into(),
                ));
            }
            if !(1..=1_000_000_000).contains(&stock.max_cumulative_expansion_micros) {
                return Err(CompanySystemError::Invalid(
                    "送转偏好累计股本扩张上限必须为 1..=1000000000 百万分比".into(),
                ));
            }
            if stock.cycles_between_proposals == 0 {
                return Err(CompanySystemError::Invalid(
                    "送转偏好提案间隔周期数必须 >= 1".into(),
                ));
            }
        }
        Ok(())
    }
}

/// 现金分红偏好（游戏化设计，非官方规则；登记于 `docs/trading-rules.md`）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct SimpleCashDividendPreference {
    /// 目标派息比例，基点（万分数，1bp=0.01%）：提案税前总额 =
    /// 可分配利润 × 该比例（整数分向下取整，再摊到每股整数分后反乘股数）。
    /// 1..=10000。
    pub target_payout_bp: u16,
    /// 最小可分配利润门槛（`Money` 分）：可分配利润低于该值不提案
    /// （宁可不分红也不制造接近零的分红）。必须为正数。
    pub min_distributable_profit: Money,
    /// 相邻两次提案（含被制度拒绝的提案）之间至少间隔的完整结算周期数；
    /// 1 = 每个结算周期期末都评估提案，2 = 隔一个周期，以此类推。
    pub cycles_between_proposals: u16,
}

/// 送转偏好（游戏化设计，非官方规则）。
///
/// 自动提案固定采用送股（`BonusShares`）：转增（`CapitalReserveConversion`）
/// 依赖 Simple 账面尚未建模的资本公积科目，且库藏股存在时强制要求回购专户
/// 登记事实（未配置会使 R 日分配致命失败），本批显式不自动提案转增。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct SimpleStockDistributionPreference {
    /// 触发门槛：可分配利润（`Money` 分）≥ 该值才评估提案送股。
    /// 必须为正数。
    pub min_distributable_profit: Money,
    /// 每股送转比例（百万分之一股/股），如 100_000 = 10 送 1。
    /// 1..=10_000_000。
    pub shares_per_existing_share_micros: u64,
    /// 累计股本扩张上限（百万分比）：自开局初始发行股数起，因送转已批准的
    /// 累计新增股数占初始发行股数的比例不得超过该上限，超过后不再提案。
    /// 1..=1_000_000_000。
    pub max_cumulative_expansion_micros: u64,
    /// 相邻两次提案（含被制度拒绝的提案）之间至少间隔的完整结算周期数。
    pub cycles_between_proposals: u16,
}

/// 偏好提案类别。扩展位：配股／增发／回购类别由对应机制批次加入。
#[derive(
    Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, serde::Serialize, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
pub enum SimplePreferenceProposalKind {
    CashDividend,
    StockDistribution,
}

impl SimplePreferenceProposalKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CashDividend => "cash-dividend",
            Self::StockDistribution => "stock-distribution",
        }
    }
}

/// 一笔被制度拒绝（或构造失败）的偏好提案记录：如实留痕，不静默、不重试。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimplePreferenceRejection {
    pub company: CompanyId,
    /// 评估发生的结算周期末日。
    pub evaluated_on: CivilDate,
    pub kind: SimplePreferenceProposalKind,
    /// 拒绝原因原文（`approve_*` 错误信息或构造期无法成案的原因）。
    pub detail: String,
}

/// 偏好提案的运行时台账。接受的提案由 `SimpleFinanceState` 的分红/送转事实与
/// Session 账簿记录，不在此重复；这里只登记拒绝，用于同周期幂等（不重试风暴）
/// 与跨恢复的频率判定。
#[derive(Clone, Eq, PartialEq, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimplePreferenceLedger {
    pub rejections: Vec<SimplePreferenceRejection>,
}

impl SimplePreferenceLedger {
    /// 记录一笔拒绝。同一 (company, kind, evaluated_on) 已存在且原因一致时幂等
    /// 返回 Ok；同键不同原因视为状态漂移，显式报错（铁律 2：不静默吞错）。
    pub fn record_rejection(
        &mut self,
        company: &CompanyId,
        kind: SimplePreferenceProposalKind,
        evaluated_on: CivilDate,
        detail: String,
    ) -> Result<(), crate::company::CompanySystemError> {
        if let Some(existing) = self.rejections.iter().find(|row| {
            &row.company == company && row.kind == kind && row.evaluated_on == evaluated_on
        }) {
            if existing.detail == detail {
                return Ok(());
            }
            return Err(crate::company::CompanySystemError::Invalid(format!(
                "公司 {} 在 {} 的 {:?} 提案拒绝记录与既有台账不一致",
                company.0, evaluated_on, kind
            )));
        }
        self.rejections.push(SimplePreferenceRejection {
            company: company.clone(),
            evaluated_on,
            kind,
            detail,
        });
        self.rejections.sort_by(|left, right| {
            (&left.company, left.evaluated_on, left.kind.as_str()).cmp(&(
                &right.company,
                right.evaluated_on,
                right.kind.as_str(),
            ))
        });
        Ok(())
    }

    /// 该公司该类别最近一次被拒提案的评估日；无记录返回 `None`。
    pub fn last_rejection_on(
        &self,
        company: &CompanyId,
        kind: SimplePreferenceProposalKind,
    ) -> Option<CivilDate> {
        self.rejections
            .iter()
            .filter(|row| &row.company == company && row.kind == kind)
            .map(|row| row.evaluated_on)
            .max()
    }

    /// 台账校验：公司身份已知、日期不晚于已推进日、键唯一。
    pub(crate) fn validate(
        &self,
        known: &std::collections::BTreeSet<CompanyId>,
        advanced_through: CivilDate,
    ) -> Result<(), crate::company::CompanySystemError> {
        use crate::company::CompanySystemError;
        let mut seen = std::collections::BTreeSet::new();
        for row in &self.rejections {
            if !known.contains(&row.company)
                || row.evaluated_on > advanced_through
                || row.detail.trim().is_empty()
                || !seen.insert((row.company.clone(), row.kind, row.evaluated_on))
            {
                return Err(CompanySystemError::Invalid(format!(
                    "偏好提案拒绝台账含未知公司、未来日期或重复键：{}",
                    row.company.0
                )));
            }
        }
        Ok(())
    }
}

/// 相邻自然月的月差：`(later - earlier)` 的整月数（要求 earlier <= later）。
fn months_between(earlier: CivilDate, later: CivilDate) -> i32 {
    i32::from(later.year()) * 12 + i32::from(later.month())
        - (i32::from(earlier.year()) * 12 + i32::from(earlier.month()))
}

/// 频率判定：自最近一次提案（接受或被拒）所属周期末日起，是否已满足
/// `cycles_between_proposals` 个完整结算周期的间隔。
pub(crate) fn proposal_due(
    last_proposal_period_end: Option<CivilDate>,
    evaluated_on: CivilDate,
    cycles_between_proposals: u16,
    cycle_months: u8,
) -> bool {
    let Some(last) = last_proposal_period_end else {
        return true;
    };
    let required = i32::from(cycles_between_proposals) * i32::from(cycle_months);
    months_between(last, evaluated_on) >= required
}

/// 偏好自动提案的确定性现金分红计划 id（同公司同周期唯一，幂等键）。
pub(crate) fn cash_dividend_plan_id(company: &CompanyId, period_end: CivilDate) -> String {
    format!("simple-preference:{}:dividend:{}", company.0, period_end)
}

/// 偏好自动提案的确定性送转事件 id（同公司同周期唯一，幂等键）。
pub(crate) fn stock_distribution_event_id(company: &CompanyId, period_end: CivilDate) -> String {
    format!(
        "simple-preference:{}:stock-distribution:{}",
        company.0, period_end
    )
}

/// 现金分红提案封顶的行情敞口上下文（游戏化保护，非官方规则；口径登记于
/// `docs/trading-rules.md`「公司行为偏好自动提案」）。
pub(crate) struct SimpleCashDividendExposure {
    /// 提案时点最新收盘价 = 周期末最后一个交易日的收盘（跌停余量推导的锚）。
    pub last_close: Money,
    /// 该股适用涨跌幅限制（整数基点，如 10% = 1000）。
    pub limit_bps: u32,
    /// 申报价格最小变动单位：跌停链迭代与引擎 `price_bound` 同一取整口径。
    pub tick: Money,
    /// 同除息日既有现金分红方案（显式 + 自动）合计税前每股红利：同除息日的
    /// gross 在除息日按组合并计算参考价，必须一并计入封顶。
    pub same_ex_date_gross_per_share: Money,
    /// 同除息日既有送转事件的股份变动比例（合并除权组对分子有更严的下界）。
    pub same_ex_date_ratio_micros: Option<u64>,
}

/// 现金分红偏好评估输入：全部为既有事实的只读快照，评估本身不产生副作用。
pub(crate) struct SimpleCashDividendEvaluation<'a> {
    pub preference: &'a SimpleCashDividendPreference,
    pub company: &'a CompanyId,
    pub stock: &'a StockCode,
    pub exchange: CalendarExchange,
    /// 结算周期末日（评估触发日）。
    pub period_end: CivilDate,
    /// 提案批准/公告日：`period_end` 的次一自然日（= 评估时点的会话自然日；
    /// 分红决议批准日必须晚于最后结算日，见 `declare_dividend`）。
    pub approve_on: CivilDate,
    pub calendar: &'a TradingCalendar,
    pub distributable: DistributableProfit,
    /// 名册非库藏股股数（与 `approve_cash_dividend` 同口径）。
    pub eligible_shares: u64,
    /// 行情敞口上下文。游戏化保护（非官方规则，登记于 `docs/trading-rules.md`）：
    /// 每股派息按「实际日程推导的跌停敞口余量」封顶——从锚收盘迭代引擎真实
    /// 跌停链 n 步（n 为公告→登记间敞露交易日数）得登记日最小可能收盘，再减
    /// 必留正参考价分子，防止偏好自动方案的除息参考价非正（参考价非正会使
    /// 除息日日结致命失败，自动提案不得把局推向不可结算状态）。缺行情锚时
    /// 无法约束上限，如实拒绝。
    pub exposure: Option<SimpleCashDividendExposure>,
    /// 已显式绑定的注册资本法定事实；缺省时无法构造决议。
    pub registered_capital: Option<AccountingAmount>,
    /// 该公司现金分红最近一次提案（接受或被拒）所属结算周期末日。
    pub last_proposal_period_end: Option<CivilDate>,
    pub cycle_months: u8,
    /// 本周期已由偏好产生过现金分红提案（确定性 plan_id 已存在于既有事实）。
    pub suppress_duplicate: bool,
}

/// 现金分红偏好评估结论。
#[derive(Debug)]
pub(crate) enum SimpleCashDividendOutcome {
    /// 触发条件不满足或本周期已处理：不提案（不是拒绝，无需记录）。
    Skip,
    /// 构造出显式方案，交给与显式 API 相同的 `approve_cash_dividend` 入口。
    Proposal {
        declaration: DividendDeclaration,
        plan: CashDividendPlan,
    },
    /// 公司想提案但无法成案（缺法定事实/金额摊薄为零/溢出）：如实记录拒绝。
    Rejected { detail: String },
}

/// 现金分红偏好评估：纯整数/定点计算，O(1)。
pub(crate) fn evaluate_cash_dividend_preference(
    input: &SimpleCashDividendEvaluation<'_>,
) -> SimpleCashDividendOutcome {
    if input.suppress_duplicate
        || !proposal_due(
            input.last_proposal_period_end,
            input.period_end,
            input.preference.cycles_between_proposals,
            input.cycle_months,
        )
    {
        return SimpleCashDividendOutcome::Skip;
    }
    let available = input.distributable.available_for_distribution;
    if available < AccountingAmount::from_money(input.preference.min_distributable_profit) {
        return SimpleCashDividendOutcome::Skip;
    }
    let Some(registered_capital) = input.registered_capital else {
        return SimpleCashDividendOutcome::Rejected {
            detail: "现金分红提案需要先显式绑定公司注册资本法定事实".into(),
        };
    };
    let available_cents = available.cents();
    let target_cents =
        available_cents * i128::from(input.preference.target_payout_bp) / BASIS_POINT_DENOMINATOR;
    let per_share = if input.eligible_shares == 0 {
        0
    } else {
        i64::try_from(target_cents / i128::from(input.eligible_shares)).unwrap_or(0)
    };
    // 游戏化保护：先按方案日程推导（登记日, 除息日），再按敞露交易日数与
    // 涨跌幅限制迭代「公告→登记间连续跌停」的真实链得到登记日收盘的最小
    // 可能值，每股派息（连同同除息日既有方案的 gross 合并）封顶在该值以内，
    // 保证除息日参考价在最坏行情下仍严格为正。
    let announcement_on = input.approve_on;
    let registered_on =
        match preference_registered_on(input.calendar, input.exchange, announcement_on) {
            Ok(date) => date,
            Err(error) => {
                return SimpleCashDividendOutcome::Rejected {
                    detail: format!("现金分红提案登记日推导失败：{error}"),
                };
            }
        };
    let ex_dividend_on = match input
        .calendar
        .next_trading_day(input.exchange, registered_on)
    {
        Ok(date) => date,
        Err(error) => {
            return SimpleCashDividendOutcome::Rejected {
                detail: format!("现金分红提案除息日推导失败：{error}"),
            };
        }
    };
    let Some(exposure) = input.exposure.as_ref() else {
        return SimpleCashDividendOutcome::Rejected {
            detail: "现金分红提案缺少行情前收锚，无法约束每股派息上限".into(),
        };
    };
    let exposure_days = match exposure_trading_days(
        input.calendar,
        input.exchange,
        input.period_end,
        registered_on,
    ) {
        Ok(days) => days,
        Err(error) => {
            return SimpleCashDividendOutcome::Rejected {
                detail: format!("现金分红提案跌停敞口交易日推导失败：{error}"),
            };
        }
    };
    let Some(margin_floor_cents) = limit_down_margin_floor_cents(
        exposure.last_close.cents(),
        exposure.limit_bps,
        exposure_days,
        exposure.tick.cents(),
    ) else {
        return SimpleCashDividendOutcome::Rejected {
            detail: format!(
                "现金分红提案跌停链推导失败：收盘 {} 分（价位 {} 分、涨跌幅限制\
                 {}bp、敞露 {} 个交易日）无法迭代出正的最小收盘",
                exposure.last_close.cents(),
                exposure.tick.cents(),
                exposure.limit_bps,
                exposure_days
            ),
        };
    };
    let combined_ratio_micros = exposure.same_ex_date_ratio_micros.unwrap_or(0);
    let required_positive_cents = combined_group_required_positive_cents(combined_ratio_micros);
    // 同除息日合计税前每股红利（既有显式/自动方案 + 本提案）的封顶。
    let gross_cap_cents = margin_floor_cents - required_positive_cents;
    if gross_cap_cents <= 0 {
        return SimpleCashDividendOutcome::Rejected {
            detail: format!(
                "现金分红提案跌停余量封顶非正：最坏登记日收盘下界 {} 分不足以\
                 保留 {} 分正参考价分子",
                margin_floor_cents, required_positive_cents
            ),
        };
    }
    let existing_gross_cents = i128::from(exposure.same_ex_date_gross_per_share.cents());
    let per_share_cap_cents = gross_cap_cents - existing_gross_cents;
    if per_share_cap_cents <= 0 {
        return SimpleCashDividendOutcome::Rejected {
            detail: format!(
                "同除息日既有方案税前每股 {} 分已占满/超出跌停余量封顶 {} 分，\
                 本周期提案每股不足一分，无法构造整数分方案",
                existing_gross_cents, gross_cap_cents
            ),
        };
    }
    let per_share = per_share.min(i64::try_from(per_share_cap_cents).unwrap_or(i64::MAX));
    if per_share <= 0 {
        return SimpleCashDividendOutcome::Rejected {
            detail: format!(
                "目标派息额 {} 分摊到 {} 股（或受跌停余量封顶）后每股不足一分，\
                 无法构造整数分方案",
                target_cents, input.eligible_shares
            ),
        };
    }
    let gross_cents = i128::from(per_share)
        .checked_mul(i128::from(input.eligible_shares))
        .unwrap_or(i128::MAX);
    let authorized_money = match (i64::try_from(gross_cents), i64::try_from(available_cents)) {
        (Ok(_gross), Ok(authorized)) => authorized,
        _ => {
            return SimpleCashDividendOutcome::Rejected {
                detail: "现金分红提案金额超出 Money 分值域".into(),
            };
        }
    };
    let per_share_money = Money::from_cents(per_share);
    let payable_on = ex_dividend_on;
    let plan_id = cash_dividend_plan_id(input.company, input.period_end);
    let plan = match CashDividendPlan::new(
        plan_id.clone(),
        input.company.clone(),
        input.stock.clone(),
        input.exchange,
        crate::company::ex_reference_price::CashDividendFormula::StandardCashOnly,
        announcement_on,
        announcement_on,
        registered_on,
        ex_dividend_on,
        payable_on,
        per_share_money,
        Money::from_cents(authorized_money),
        input.calendar,
    ) {
        Ok(plan) => plan,
        Err(error) => {
            return SimpleCashDividendOutcome::Rejected {
                detail: format!("现金分红偏好方案自检失败：{error}"),
            };
        }
    };
    SimpleCashDividendOutcome::Proposal {
        declaration: DividendDeclaration {
            plan_id,
            approved_on: announcement_on,
            total_gross: AccountingAmount::from_cents(gross_cents),
            registered_capital,
        },
        plan,
    }
}

/// 送转提案的行情敞口上下文（游戏化保护；与现金分红封顶同口径推导，登记于
/// `docs/trading-rules.md`「公司行为偏好自动提案」）。
pub(crate) struct SimpleStockDistributionExposure {
    /// 提案时点最新收盘价 = 周期末最后一个交易日的收盘（跌停余量推导的锚）。
    pub last_close: Money,
    /// 该股适用涨跌幅限制（整数基点）。
    pub limit_bps: u32,
    /// 申报价格最小变动单位：跌停链迭代与引擎 `price_bound` 同一取整口径。
    pub tick: Money,
    /// 同除权日既有现金分红方案（显式 + 自动，含本周期刚批准的自动现金提案）
    /// 合计税前每股红利：合并除权组的参考价 = (前收 − 该红利)/(1 + 变动比例)。
    pub same_ex_date_gross_per_share: Money,
    /// 同除权日已存在送转事件：两起送转事件的合并除权口径未核实，除权日日结
    /// 会显式失败（`prepare_ex_references_for_current_date`），自动提案不得叠加。
    pub same_ex_date_stock_event: bool,
    /// 同除权日已存在配股事件：配股×送转同日的合并除权口径未核实，除权日
    /// 日结会显式失败（`prepare_ex_references_for_current_date`），自动提案
    /// 不得叠加（受理时配股实际认购比例未定，按事件存在性保守守卫）。
    pub same_ex_date_rights_event: bool,
}

/// 送转偏好评估输入。
pub(crate) struct SimpleStockDistributionEvaluation<'a> {
    pub preference: &'a SimpleStockDistributionPreference,
    pub company: &'a CompanyId,
    pub stock: &'a StockCode,
    pub exchange: CalendarExchange,
    pub period_end: CivilDate,
    pub approve_on: CivilDate,
    pub calendar: &'a TradingCalendar,
    pub distributable: DistributableProfit,
    /// 名册非库藏股股数（送转分配的参与基数，与分配算法同口径）。
    pub eligible_shares: u64,
    /// 开局初始发行股数（累计扩张上限的基数）。
    pub initial_issued_shares: u64,
    /// 既有送转事实（含已批准未入账）累计新增股数。
    pub cumulative_distributed_shares: u64,
    /// 行情敞口上下文；缺行情锚时无法推导除权参考价安全余量，如实拒绝。
    pub exposure: Option<SimpleStockDistributionExposure>,
    pub last_proposal_period_end: Option<CivilDate>,
    pub cycle_months: u8,
    /// 本周期已由偏好产生过送转提案（确定性 event_id 已存在）。
    pub suppress_duplicate: bool,
}

/// 送转偏好评估结论。
#[derive(Debug)]
pub(crate) enum SimpleStockDistributionOutcome {
    Skip,
    Proposal {
        plan: crate::company::stock_distribution::StockDistributionEventPlan,
    },
    Rejected {
        detail: String,
    },
}

/// 送转偏好评估：纯整数/定点计算，O(1)。
pub(crate) fn evaluate_stock_distribution_preference(
    input: &SimpleStockDistributionEvaluation<'_>,
) -> SimpleStockDistributionOutcome {
    if input.suppress_duplicate
        || !proposal_due(
            input.last_proposal_period_end,
            input.period_end,
            input.preference.cycles_between_proposals,
            input.cycle_months,
        )
    {
        return SimpleStockDistributionOutcome::Skip;
    }
    let available = input.distributable.available_for_distribution;
    if available < AccountingAmount::from_money(input.preference.min_distributable_profit) {
        return SimpleStockDistributionOutcome::Skip;
    }
    // 累计扩张上限：已批准送转累计新增 + 本提案新增 ≤ 上限 × 初始发行。
    let proposed_total = u128::from(input.eligible_shares)
        .checked_mul(u128::from(
            input.preference.shares_per_existing_share_micros,
        ))
        .map(|product| product / u128::from(SHARE_RATIO_DENOMINATOR))
        .unwrap_or(0);
    let cumulative_after = u128::from(input.cumulative_distributed_shares) + proposed_total;
    let expansion_budget = u128::from(input.initial_issued_shares)
        .checked_mul(u128::from(input.preference.max_cumulative_expansion_micros))
        .unwrap_or(u128::MAX)
        / u128::from(SHARE_RATIO_DENOMINATOR);
    if cumulative_after > expansion_budget {
        return SimpleStockDistributionOutcome::Skip;
    }
    let new_shares = match u64::try_from(proposed_total) {
        Ok(shares) if shares > 0 => shares,
        _ => {
            return SimpleStockDistributionOutcome::Rejected {
                detail: format!(
                    "送转比例 {} 百万分比摊到 {} 股不足一股，无法构造方案",
                    input.preference.shares_per_existing_share_micros, input.eligible_shares
                ),
            };
        }
    };
    // 游戏化保护：自动送转不得把局推向除权日不可结算状态。同除权日既有送转
    // 或配股事件的合并除权口径未核实（叠加会在除权日显式失败）；与同除权日现金红利
    // 合并成组时，最坏行情（公告→登记连续跌停）下的合并参考价必须仍 ≥1 分。
    let announcement_on = input.approve_on;
    let registered_on =
        match preference_registered_on(input.calendar, input.exchange, announcement_on) {
            Ok(date) => date,
            Err(error) => {
                return SimpleStockDistributionOutcome::Rejected {
                    detail: format!("送转提案登记日推导失败：{error}"),
                };
            }
        };
    let ex_rights_on = match input
        .calendar
        .next_trading_day(input.exchange, registered_on)
    {
        Ok(date) => date,
        Err(error) => {
            return SimpleStockDistributionOutcome::Rejected {
                detail: format!("送转提案除权日推导失败：{error}"),
            };
        }
    };
    let Some(exposure) = input.exposure.as_ref() else {
        return SimpleStockDistributionOutcome::Rejected {
            detail: "送转提案缺少行情前收锚，无法推导除权参考价安全余量".into(),
        };
    };
    if exposure.same_ex_date_stock_event {
        return SimpleStockDistributionOutcome::Rejected {
            detail: format!(
                "同除权日 {} 已存在送转事件，两起送转的合并除权口径未核实，\
                 自动提案不叠加",
                ex_rights_on
            ),
        };
    }
    if exposure.same_ex_date_rights_event {
        return SimpleStockDistributionOutcome::Rejected {
            detail: format!(
                "同除权日 {} 已存在配股事件，配股×送转同日的合并除权口径未核实，\
                 自动提案不叠加",
                ex_rights_on
            ),
        };
    }
    let exposure_days = match exposure_trading_days(
        input.calendar,
        input.exchange,
        input.period_end,
        registered_on,
    ) {
        Ok(days) => days,
        Err(error) => {
            return SimpleStockDistributionOutcome::Rejected {
                detail: format!("送转提案跌停敞口交易日推导失败：{error}"),
            };
        }
    };
    let Some(margin_floor_cents) = limit_down_margin_floor_cents(
        exposure.last_close.cents(),
        exposure.limit_bps,
        exposure_days,
        exposure.tick.cents(),
    ) else {
        return SimpleStockDistributionOutcome::Rejected {
            detail: format!(
                "送转提案跌停链推导失败：收盘 {} 分（价位 {} 分、涨跌幅限制 {}bp、\
                 敞露 {} 个交易日）无法迭代出正的最小收盘",
                exposure.last_close.cents(),
                exposure.tick.cents(),
                exposure.limit_bps,
                exposure_days
            ),
        };
    };
    let required_positive_cents =
        combined_group_required_positive_cents(input.preference.shares_per_existing_share_micros);
    let combined_numerator_floor =
        margin_floor_cents - i128::from(exposure.same_ex_date_gross_per_share.cents());
    if combined_numerator_floor < required_positive_cents {
        return SimpleStockDistributionOutcome::Rejected {
            detail: format!(
                "同除权日合并除权参考价安全余量不足：最坏登记日收盘下界 {} 分 − \
                 同日现金红利 {} 分 < 保留 {} 分正参考价分子所需",
                margin_floor_cents,
                exposure.same_ex_date_gross_per_share.cents(),
                required_positive_cents
            ),
        };
    }
    SimpleStockDistributionOutcome::Proposal {
        plan: crate::company::stock_distribution::StockDistributionEventPlan {
            event_id: stock_distribution_event_id(input.company, input.period_end),
            approval_reference: format!(
                "simple-preference:{}:{}",
                input.company.0, input.period_end
            ),
            issuer: input.company.clone(),
            stock: input.stock.clone(),
            exchange: input.exchange,
            kind: crate::company::stock_distribution::StockDistributionKind::BonusShares,
            approved_on: announcement_on,
            announced_on: announcement_on,
            registered_on,
            ex_rights_on,
            shares_per_existing_share_micros: input.preference.shares_per_existing_share_micros,
            approved_total_new_shares: new_shares,
        },
    }
}

/// `date` 起（含）的第一个交易所交易日。
fn first_trading_day_on_or_after(
    calendar: &TradingCalendar,
    exchange: CalendarExchange,
    date: CivilDate,
) -> Result<CivilDate, crate::calendar::CalendarError> {
    if calendar.is_trading_day(exchange, date)? {
        return Ok(date);
    }
    calendar.next_trading_day(exchange, date)
}

/// 自动提案的权益登记日：公告日起的首个交易日；若公告日本身就是交易日，
/// 登记日再顺延到下一交易日（公告日 ≠ 登记日）。既有公开 typed 分红公告
/// 只在「日结时状态恰为 Announced 且当日为公告日」时发布，公告与登记同日
/// 会让披露时状态已推进为 Registered 而永远错过公告通道——自动提案必须
/// 与显式方案一样经由该通道公开（ADR-0037：不新增旁路）。
fn preference_registered_on(
    calendar: &TradingCalendar,
    exchange: CalendarExchange,
    announced_on: CivilDate,
) -> Result<CivilDate, crate::calendar::CalendarError> {
    let first = first_trading_day_on_or_after(calendar, exchange, announced_on)?;
    if first == announced_on {
        return calendar.next_trading_day(exchange, first);
    }
    Ok(first)
}

/// 自动提案的（登记日, 除息/除权日）：Session 侧扫描同除息日既有方案与
/// 评估函数内部构造方案共用同一推导，避免两侧日程口径漂移。
pub(crate) fn preference_ex_dates(
    calendar: &TradingCalendar,
    exchange: CalendarExchange,
    announced_on: CivilDate,
) -> Result<(CivilDate, CivilDate), crate::calendar::CalendarError> {
    let registered_on = preference_registered_on(calendar, exchange, announced_on)?;
    let ex_on = calendar.next_trading_day(exchange, registered_on)?;
    Ok((registered_on, ex_on))
}

/// 提案锚收盘日（周期末日）到登记日之间的跌停敞露交易日数：区间
/// `(period_end, registered_on]` 内的交易所交易日。评估发生在周期末日结后、
/// 批准日开市前，锚即该区间前最后一个交易日的收盘价；区间内每个交易日的
/// 收盘相对前一收盘最多再跌一个跌停幅度。按日程逐日推导，不拍脑袋固定
/// 天数（公告日为交易日时 n=2：公告日与登记日；公告日休市时 n=1）。
pub(crate) fn exposure_trading_days(
    calendar: &TradingCalendar,
    exchange: CalendarExchange,
    period_end: CivilDate,
    registered_on: CivilDate,
) -> Result<u32, crate::calendar::CalendarError> {
    let mut count = 0_u32;
    let mut cursor = period_end;
    while cursor < registered_on {
        cursor = calendar.next_trading_day(exchange, cursor)?;
        count += 1;
    }
    Ok(count)
}

/// 连续 `n` 个交易日一字跌停后登记日收盘价的最小可能值（整数分）。
///
/// **权威推导 = 从锚收盘出发迭代引擎真实跌停链 `n` 步**：每步复用
/// `market::price_bound_cents` 的整数核心（昨收 × (1−p) 正数四舍五入到
/// 最小价位；**跌停幅度不足一个价位时强制至少下移一个价位**；最低不低于
/// 一个价位），与 [`crate::market::Market`] 的 `down_stop` 完全同源，不存在
/// 另行推导的闭式近似。
///
/// 历史教训（2026-10-07 修复轮 2，已修复）：旧实现的闭式线性下界
/// floor(锚 × (1 − n·p)) 依托「四舍五入偏上 ⇒ 收盘 ≥ 锚×(1−p)^n ≥ 锚×(1−np)」
/// 的推断，但该推断漏掉了低价域守卫——锚 × limit_bps ≤ 5000（跌停幅度不足
/// 半个价位）时舍入结果不离开昨收，守卫强制整价位下移，单日实际跌幅超过 p
/// （如 4 分锚 @10% 板两日实际 4→3→2，闭式却给 floor(3.2)=3），按闭式封顶
/// 会使最坏除息参考价非正（反例：10% 板锚 3–5 分、5% 板锚 3–10 分 × n=2）。
/// 复核以独立复刻 `price_bound` 整数逻辑的暴力验证证伪后改为本迭代实现，
/// 不保留闭式快速路径；测试侧另有独立复刻实现做全域交叉验证。
///
/// 链每步恒 ≥ 一个价位，故锚为正时返回值恒 ≥ tick，不存在「余量耗尽」；
/// `None` 仅表示输入非法（锚/价位非正、limit_bps > 10000）或推导溢出，
/// 由调用方如实拒绝（铁律二）。
pub(crate) fn limit_down_margin_floor_cents(
    last_close_cents: i64,
    limit_bps: u32,
    exposure_days: u32,
    tick_cents: i64,
) -> Option<i128> {
    if last_close_cents <= 0 {
        return None;
    }
    let down_ratio_bps = 10_000_u32.checked_sub(limit_bps)?;
    let mut current_cents = last_close_cents;
    for _ in 0..exposure_days {
        current_cents =
            crate::market::price_bound_cents(current_cents, down_ratio_bps, tick_cents, false)?;
    }
    Some(i128::from(current_cents))
}

/// 合并除权除息组要得到 ≥1 分的除权参考价，「前收盘 − 同日合计税前每股
/// 现金红利」的分子必须 ≥ ceil((10^6 + ratio_micros)/10^6) 分：参考价 =
/// 分子 × 10^6 / (10^6 + ratio) 银行家舍入到分，商为 0 时舍入无法保证 ≥1。
/// 纯现金组（ratio = 0）即熟悉的「至少留 1 分」。
pub(crate) fn combined_group_required_positive_cents(ratio_micros: u64) -> i128 {
    1 + i128::from((ratio_micros + SHARE_RATIO_DENOMINATOR - 1) / SHARE_RATIO_DENOMINATOR)
}
