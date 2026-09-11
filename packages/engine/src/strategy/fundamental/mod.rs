//! 个人基本面假设与共享类型（K5 行 130–133 / K5a 行 147–154，任务 18）。
//!
//! 本目录是策略域的纯分析内核：只消费本人已获知的公开报告事实（任务 16
//! `NpcObservationContext` 引用面 → [`super::super::beliefs`] 编排 →
//! [`facts::extract_annual_facts`]），不读 session/行情/总账。估值纯属个人
//! ——不存在任何设定或调整市场价格的路径（测试以行情快照 spy 锁定）。
//!
//! 数值纪律：全程整数（分 + 基点），除法一律 [`div_round_half_even`]
//! （半偶舍入），乘加全部 checked（溢出 ⇒ 类型化不可用，绝不截断）。

mod facts;
mod forecast;
mod update;
mod valuation;

pub use facts::{AnnualFacts, PriorRevenue, extract_annual_facts};
pub use forecast::{
    ForecastBasis, ForecastState, GROWTH_PRIOR_CLAMP_BP, GrowthObservation, initial_forecast,
    observe_growth, revise_forecast,
};
pub use update::{BeliefCause, CauseRecord};
pub(crate) use update::{FAILURE_CONFIDENCE_DELTA_BP, PROFITABLE_EXIT_CONFIDENCE_DELTA_BP};
pub use valuation::{ScenarioEstimates, cash_flow, earnings_multiple, equity_roe};

use crate::accounting::AccountingAmount;
use crate::accounting::reports::ReportKind;
use crate::company::CompanyId;
use crate::information::PublicationId;
use crate::money::Money;

use super::Rng;
use super::analysis_profile::FundamentalMethod;
use super::profile::{InstitutionStyle, RetailStyle, StrategyProfile};

/// 个人一次性假设（K5a 行 149–151 的区间）。游戏默认经
/// [`draw_personal_assumptions`] 抽样落在计划区间内；类型允许任意值——退化
/// 假设（如 PE ≤ 0、终值增长 ≥ 资本成本）在方法层得到类型化不可用，绝不
/// 被静默 clamp 或代换。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PersonalAssumptions {
    /// 个人增长偏差 [-1000,1000]bp（一次性）。
    pub growth_deviation_bp: i32,
    /// 盈利质量系数 [8000,12000]bp（一次性）。
    pub quality_coefficient_bp: i32,
    /// 个人 PE 倍数（整数）：DeepValue 8..=16 / Defensive 6..=12 / 其他 10..=24。
    pub pe_multiple: i32,
    /// 权益资本成本 [800,1600]bp（一次性）。
    pub equity_cost_bp: i32,
    /// 终值增长 [0,300]bp（必须严格小于权益资本成本，否则现金流法不可用）。
    pub terminal_growth_bp: i32,
    /// 预期 ROE 个人偏差 [-300,300]bp（一次性）。
    pub roe_deviation_bp: i32,
}

/// 一次性抽样：恰 6 次 `next_f64`，canonical 序 = 字段声明序（增长偏差 →
/// 质量系数 → PE → 权益成本 → 终值增长 → ROE 偏差）。
///
/// RNG 流由调用方注入：必须是**专用确定性 profile 流**（任务 26 接线时用
/// 独立种子派生，绝不与 session 主 RNG 混流——extraction_replay 字节锚点
/// 会红，任务 17 已登记该教训）。整数区间采样沿用 sampling.rs 的
/// `lo + (f × width).min(width - 1)` 含端点约定。
pub fn draw_personal_assumptions(
    profile: &StrategyProfile,
    rng: &mut dyn Rng,
) -> PersonalAssumptions {
    let (pe_lo, pe_hi) = pe_range_for(profile);
    PersonalAssumptions {
        growth_deviation_bp: sample_i32_inclusive(rng, -1_000, 1_000),
        quality_coefficient_bp: sample_i32_inclusive(rng, 8_000, 12_000),
        pe_multiple: sample_i32_inclusive(rng, pe_lo, pe_hi),
        equity_cost_bp: sample_i32_inclusive(rng, 800, 1_600),
        terminal_growth_bp: sample_i32_inclusive(rng, 0, 300),
        roe_deviation_bp: sample_i32_inclusive(rng, -300, 300),
    }
}

/// PE 档位（K5a 行 149）：按主导风格的能力聚类。
fn pe_range_for(profile: &StrategyProfile) -> (i32, i32) {
    match profile {
        StrategyProfile::Institution(InstitutionStyle::DeepValue) => (8, 16),
        StrategyProfile::Institution(InstitutionStyle::Defensive) => (6, 12),
        _ => (10, 24),
    }
}

fn sample_i32_inclusive(rng: &mut dyn Rng, lo: i32, hi: i32) -> i32 {
    let width = u64::try_from(hi - lo + 1).expect("sampling width is positive by construction");
    let offset = ((rng.next_f64() * width as f64) as u64).min(width - 1);
    lo + i32::try_from(offset).expect("draw offset is bounded by the sampling width")
}

/// 估值不可用的类型化理由（存进信念条目、可序列化；绝不静默 fallback，
/// 也绝不抛弃整个 NPC——其他信号路径照常运转）。
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, serde::Serialize, serde::Deserialize)]
pub enum ValuationUnavailable {
    #[error("fundamental method disabled: zero fundamental weight in the analysis profile")]
    MethodDisabled,
    #[error("report company {report:?} does not match the expected issuer {expected:?}")]
    CompanyMismatch {
        expected: CompanyId,
        report: CompanyId,
    },
    #[error("unsupported report kind for valuation facts: {kind:?}")]
    UnsupportedReportKind { kind: ReportKind },
    #[error("future-dated material {report:?}: published after the evaluation instant")]
    FutureDatedMaterial { report: PublicationId },
    #[error(
        "net income to parent is not positive; the earnings multiple never takes an absolute value"
    )]
    NonPositiveNetIncome,
    #[error("personal PE multiple must be positive, got {pe}")]
    NonPositivePe { pe: i32 },
    #[error("equity cost must be a positive denominator, got {cost_bp} bp")]
    NonPositiveCost { cost_bp: i32 },
    #[error(
        "terminal growth {terminal_bp} bp must stay strictly below the equity cost {cost_bp} bp"
    )]
    TerminalGrowthNotBelowCost { terminal_bp: i32, cost_bp: i32 },
    #[error("book equity to parent is not positive")]
    NonPositiveBookEquity,
    #[error("average equity to parent is not positive; observed ROE is undefined")]
    NonPositiveAverageEquity,
    #[error("expected ROE {expected_bp} bp is not positive (high-risk; no positive fallback)")]
    NonPositiveExpectedRoe { expected_bp: i32 },
    #[error("equity valuation estimate is not positive; no positive value is fabricated")]
    NonPositiveValuationEstimate,
    #[error("growth prior unavailable: degenerate revenue base in own-known reports")]
    GrowthPriorUnavailable,
    #[error(
        "financing cash flow cannot be split into interest vs principal from own-known statements"
    )]
    FinancingSplitUndeterminable,
    #[error("total issued shares must be a positive denominator")]
    ZeroIssuedShares,
    #[error("checked arithmetic overflow at step: {step}")]
    Overflow {
        step: std::borrow::Cow<'static, str>,
    },
    #[error("per-share value exceeds the money range")]
    PerShareOutOfRange,
}

/// 每股价格区间（悲观/乐观情景；K5a 行 141/153——个人区间存每股价格，
/// 不把公司总价值直接与股价比较）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PerShareRange {
    pub pessimistic: Money,
    pub optimistic: Money,
}

/// 估值结果：归母整体权益估计（分）+ 每股区间，或类型化不可用。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ValuationOutcome {
    Available {
        total_equity_estimate: AccountingAmount,
        per_share: PerShareRange,
    },
    Unavailable {
        reason: ValuationUnavailable,
    },
}

/// 能力中心（λ 修订档位的风格聚类；K5a 行 154）。
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CapabilityCenter {
    /// 散户长期。
    LongTerm,
    /// 机构深度价值/防御。
    Value,
    /// 机构成长。
    Growth,
    /// 其余一切（含均衡——λ 归"其他"，期限归 20 日档）。
    Other,
}

/// 主导风格 → 能力中心。
pub fn capability_center(profile: &StrategyProfile) -> CapabilityCenter {
    match profile {
        StrategyProfile::Retail(RetailStyle::LongTerm) => CapabilityCenter::LongTerm,
        StrategyProfile::Institution(InstitutionStyle::DeepValue | InstitutionStyle::Defensive) => {
            CapabilityCenter::Value
        }
        StrategyProfile::Institution(InstitutionStyle::Growth) => CapabilityCenter::Growth,
        _ => CapabilityCenter::Other,
    }
}

/// λ 修订权重（K5a 行 154）：LongTerm/Value 4000bp、Growth 6000bp、其他 2500bp。
pub fn revision_lambda_bp(center: CapabilityCenter) -> i32 {
    match center {
        CapabilityCenter::LongTerm | CapabilityCenter::Value => 4_000,
        CapabilityCenter::Growth => 6_000,
        CapabilityCenter::Other => 2_500,
    }
}

/// 估值期限（K5 行 133）：长期/价值 60、均衡/成长 20、其他 5 交易日。
pub fn belief_horizon_days(profile: &StrategyProfile) -> u16 {
    match profile {
        StrategyProfile::Retail(RetailStyle::LongTerm)
        | StrategyProfile::Institution(InstitutionStyle::DeepValue | InstitutionStyle::Defensive) => {
            60
        }
        StrategyProfile::Institution(InstitutionStyle::Balanced | InstitutionStyle::Growth) => 20,
        _ => 5,
    }
}

/// 每股换算：归母整体权益估计（分）÷ 已发行普通股总股数（半偶舍入），
/// 越出 Money 值域 ⇒ 类型化。
pub fn per_share_price(
    total_cents: i128,
    total_issued_shares: u64,
) -> Result<Money, ValuationUnavailable> {
    if total_issued_shares == 0 {
        return Err(ValuationUnavailable::ZeroIssuedShares);
    }
    let cents = i64::try_from(div_round_half_even(
        total_cents,
        i128::from(total_issued_shares),
    ))
    .map_err(|_| ValuationUnavailable::PerShareOutOfRange)?;
    Ok(Money::from_cents(cents))
}

/// 情景三元组 → 每股区间（每端点独立验证）。
pub fn to_per_share_range(
    estimates: ScenarioEstimates,
    total_issued_shares: u64,
) -> Result<PerShareRange, ValuationUnavailable> {
    Ok(PerShareRange {
        pessimistic: per_share_price(estimates.pessimistic, total_issued_shares)?,
        optimistic: per_share_price(estimates.optimistic, total_issued_shares)?,
    })
}

/// 按方法分发（K5a 行 147）：方法不适用 ⇒ 类型化 Unavailable，绝不无声换
/// 另一模型；增长仅现金流法消费（先验退化 ⇒ GrowthPriorUnavailable）。
pub fn estimate_by_method(
    method: FundamentalMethod,
    facts: &AnnualFacts,
    assumptions: &PersonalAssumptions,
    growth_bp: Option<i32>,
    total_issued_shares: u64,
) -> ValuationOutcome {
    let estimates = match method {
        FundamentalMethod::EarningsMultiple => earnings_multiple(facts, assumptions),
        FundamentalMethod::EquityRoe => equity_roe(facts, assumptions),
        FundamentalMethod::CashFlow => match growth_bp {
            Some(growth_bp) => cash_flow(facts, assumptions, growth_bp),
            None => Err(ValuationUnavailable::GrowthPriorUnavailable),
        },
    };
    match estimates {
        Ok(estimates) => match to_per_share_range(estimates, total_issued_shares) {
            Ok(per_share) => ValuationOutcome::Available {
                total_equity_estimate: AccountingAmount::from_cents(estimates.central),
                per_share,
            },
            Err(reason) => ValuationOutcome::Unavailable { reason },
        },
        Err(reason) => ValuationOutcome::Unavailable { reason },
    }
}

/// 整数半偶舍入除法（denominator > 0；负分子按符号对称）。
///
/// 与 `accounting::amount::div_round_half_even` / `strategy::technical` 同
/// 算法的受控本地副本（该函数在 accounting 是私有；local-copy + 交叉引用
/// 是本仓库既定先例，任务 13 统一入口时再议）。
pub(crate) fn div_round_half_even(numerator: i128, denominator: i128) -> i128 {
    debug_assert!(
        denominator > 0,
        "all fundamental denominators are validated positive"
    );
    let sign = if numerator < 0 { -1_i128 } else { 1_i128 };
    let numerator_abs = numerator.unsigned_abs();
    let denominator_abs = denominator.unsigned_abs();
    let quotient = numerator_abs / denominator_abs;
    let remainder = numerator_abs % denominator_abs;
    let twice = remainder * 2;
    let rounded = if twice > denominator_abs || (twice == denominator_abs && quotient % 2 == 1) {
        quotient + 1
    } else {
        quotient
    };
    sign * i128::try_from(rounded).expect("u128 quotient of i128 inputs fits i128")
}
