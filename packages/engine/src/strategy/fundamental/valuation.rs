//! 三条独立估值路径（K5a 行 149–153）+ 每股换算。绝不共享一个"fair value"：
//! 每法先算**归母整体权益估计**（分），再除以发行人固定的已发行普通股
//! 总股数（绝不除以流通股数），个人区间存每股价格（悲观/乐观两情景，
//! 每情景独立验证——任一退化 ⇒ 整法类型化不可用，不补正值）。
//!
//! 游戏假设（登记 issues.md）：本游戏报表不产出"非经常项目"行，盈利倍数法
//! 的可持续盈利调整恒为 0；报表提供该行时在此扩展。

use super::div_round_half_even;
use super::facts::AnnualFacts;
use super::{PersonalAssumptions, ValuationUnavailable as Unavailable};

/// 情景估计（分）：中央 + 悲观/乐观。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScenarioEstimates {
    pub central: i128,
    pub pessimistic: i128,
    pub optimistic: i128,
}

/// 区间扰动（K5a 行 153）：增长 ±300bp、质量系数 ±1000bp、ROE ±200bp、
/// 资本成本 ±200bp。
const GROWTH_SCENARIO_BP: i32 = 300;
const QUALITY_SCENARIO_BP: i32 = 1_000;
const ROE_SCENARIO_BP: i32 = 200;
const COST_SCENARIO_BP: i32 = 200;

fn overflow(step: &'static str) -> Unavailable {
    Unavailable::Overflow { step: step.into() }
}

fn scale_round(value: i128, numerator: i128, denominator: i128) -> Result<i128, Unavailable> {
    let scaled = value
        .checked_mul(numerator)
        .ok_or_else(|| overflow("checked ratio scaling"))?;
    Ok(div_round_half_even(scaled, denominator))
}

/// 盈利倍数法（K5a 行 149）：估计 = 归母净利 × 质量系数 × PE。
/// 净利 ≤ 0 ⇒ 不可用（绝不取绝对值）；区间 = 质量系数 ±1000bp。
pub fn earnings_multiple(
    facts: &AnnualFacts,
    assumptions: &PersonalAssumptions,
) -> Result<ScenarioEstimates, Unavailable> {
    let ni = facts.net_income_to_parent.cents();
    if ni <= 0 {
        return Err(Unavailable::NonPositiveNetIncome);
    }
    if assumptions.pe_multiple <= 0 {
        return Err(Unavailable::NonPositivePe {
            pe: assumptions.pe_multiple,
        });
    }
    let estimate = |quality_bp: i32| -> Result<i128, Unavailable> {
        let scaled = ni
            .checked_mul(i128::from(quality_bp))
            .and_then(|value| value.checked_mul(i128::from(assumptions.pe_multiple)))
            .ok_or_else(|| overflow("sustainable earnings x quality coefficient"))?;
        let value = div_round_half_even(scaled, 10_000);
        if value <= 0 {
            return Err(Unavailable::NonPositiveValuationEstimate);
        }
        Ok(value)
    };
    Ok(ScenarioEstimates {
        central: estimate(assumptions.quality_coefficient_bp)?,
        pessimistic: estimate(assumptions.quality_coefficient_bp - QUALITY_SCENARIO_BP)?,
        optimistic: estimate(assumptions.quality_coefficient_bp + QUALITY_SCENARIO_BP)?,
    })
}

/// FCFE 起点（口径推导，模块文档钉死）：
///
/// - capex = −投资活动 CF：本游戏投资类现金流出唯一构成是资本开支；
///   地产购地/开发支出按其列报选择记经营——已含在经营 CF 内，口径统一；
/// - 新借 − 还本 = 借款行附注运动（利息不经借款科目）；
/// - 现金利息支付 = (新借 − 还本) − 筹资 CF（筹资 = 新借 − 利息 − 还本；
///   本游戏经营 CF 未扣利息支付 ⇒ 需再扣利息统一到股东口径）；
/// - 代数恒等：FCFE = 经营CF − capex + (新借−还本) − 利息 ≡ 年度净现金
///   变动（游戏无分红/回购——K3 红线）。筹资拆出负利息 ⇒ 窗口含借款行
///   运动无法解释的筹资流入（如开局凭证）⇒ 类型化 Undeterminable。
fn fcfe_starting_point(facts: &AnnualFacts) -> Result<i128, Unavailable> {
    let capex = facts
        .investing_cf
        .cents()
        .checked_neg()
        .ok_or_else(|| overflow("capex negation"))?;
    let interest = facts
        .net_new_borrowings
        .cents()
        .checked_sub(facts.financing_cf.cents())
        .ok_or_else(|| overflow("financing split"))?;
    if interest < 0 {
        return Err(Unavailable::FinancingSplitUndeterminable);
    }
    facts
        .operating_cf
        .cents()
        .checked_sub(capex)
        .and_then(|value| value.checked_add(facts.net_new_borrowings.cents()))
        .and_then(|value| value.checked_sub(interest))
        .ok_or_else(|| overflow("fcfe assembly"))
}

/// 现金流法（K5a 行 150）：5 年个人增长预测 + 有条件终值；资本成本必须
/// 严格大于终值增长；区间 = 增长 ∓300bp × 资本成本 ±200bp。
pub fn cash_flow(
    facts: &AnnualFacts,
    assumptions: &PersonalAssumptions,
    growth_bp: i32,
) -> Result<ScenarioEstimates, Unavailable> {
    let fcfe = fcfe_starting_point(facts)?;
    let run = |growth: i32, cost: i32| {
        dcf_equity_total(fcfe, growth, cost, assumptions.terminal_growth_bp)
    };
    Ok(ScenarioEstimates {
        central: run(growth_bp, assumptions.equity_cost_bp)?,
        pessimistic: run(
            growth_bp - GROWTH_SCENARIO_BP,
            assumptions.equity_cost_bp + COST_SCENARIO_BP,
        )?,
        optimistic: run(
            growth_bp + GROWTH_SCENARIO_BP,
            assumptions.equity_cost_bp - COST_SCENARIO_BP,
        )?,
    })
}

/// 5 年 FCFE 折现 + 终值（全整数、逐步半偶舍入；checked 溢出 ⇒ Overflow）：
/// `FCFE_t = rhe(FCFE_{t−1} × (10000+g), 10000)`；
/// `PV_t = rhe(FCFE_t × 10000, 10000+r)`；
/// `FCFE_6 = rhe(FCFE_5 × (10000+gt), 10000)`，`TV = rhe(FCFE_6 × 10000, r−gt)`
/// （r ≤ gt ⇒ 类型化不可用——无限估值），PV(TV) 按五年折现链。
fn dcf_equity_total(
    fcfe: i128,
    growth_bp: i32,
    cost_bp: i32,
    terminal_bp: i32,
) -> Result<i128, Unavailable> {
    if cost_bp <= 0 {
        return Err(Unavailable::NonPositiveCost { cost_bp });
    }
    let spread = cost_bp - terminal_bp;
    if spread <= 0 {
        return Err(Unavailable::TerminalGrowthNotBelowCost {
            terminal_bp,
            cost_bp,
        });
    }
    let growth_factor = i128::from(10_000 + growth_bp);
    let terminal_factor = i128::from(10_000 + terminal_bp);
    if growth_factor <= 0 || terminal_factor <= 0 {
        // 退化增长（负因子）令折现链无定义——类型化，不产出无意义估值。
        return Err(Unavailable::NonPositiveValuationEstimate);
    }
    let discount = i128::from(10_000 + cost_bp);
    let mut f = fcfe;
    let mut total: i128 = 0;
    for _ in 0..5 {
        f = scale_round(f, growth_factor, 10_000)?;
        let present = scale_round(f, 10_000, discount)?;
        total = total
            .checked_add(present)
            .ok_or_else(|| overflow("dcf year sum"))?;
    }
    let fcfe_terminal = scale_round(f, terminal_factor, 10_000)?;
    let mut terminal_value = scale_round(fcfe_terminal, 10_000, i128::from(spread))?;
    for _ in 0..5 {
        terminal_value = scale_round(terminal_value, 10_000, discount)?;
    }
    let value = total
        .checked_add(terminal_value)
        .ok_or_else(|| overflow("dcf total"))?;
    if value <= 0 {
        return Err(Unavailable::NonPositiveValuationEstimate);
    }
    Ok(value)
}

/// 权益 ROE 法（K5a 行 151，银行/保险按规则固定）：
/// 观察 ROE = rhe(归母净利 × 10000, 平均归母权益)（显式平均权益口径）；
/// 预期 ROE = 观察 ROE + 个人偏差；估计 = 归母权益 × 预期ROE / 资本成本。
/// 权益/平均权益/预期 ROE 非正 ⇒ 不可用/高风险；区间 = ROE ∓200bp ×
/// 资本成本 ±200bp。
pub fn equity_roe(
    facts: &AnnualFacts,
    assumptions: &PersonalAssumptions,
) -> Result<ScenarioEstimates, Unavailable> {
    let equity = facts.equity_to_parent.cents();
    if equity <= 0 {
        return Err(Unavailable::NonPositiveBookEquity);
    }
    if assumptions.equity_cost_bp <= 0 {
        return Err(Unavailable::NonPositiveCost {
            cost_bp: assumptions.equity_cost_bp,
        });
    }
    let sum = facts
        .opening_equity_to_parent
        .cents()
        .checked_add(equity)
        .ok_or_else(|| overflow("average equity"))?;
    let average = div_round_half_even(sum, 2);
    if average <= 0 {
        return Err(Unavailable::NonPositiveAverageEquity);
    }
    let scaled_ni = facts
        .net_income_to_parent
        .cents()
        .checked_mul(10_000)
        .ok_or_else(|| overflow("observed ROE"))?;
    let observed_bp = i32::try_from(div_round_half_even(scaled_ni, average))
        .map_err(|_| overflow("observed ROE"))?;
    let expected_bp = observed_bp
        .checked_add(assumptions.roe_deviation_bp)
        .ok_or_else(|| overflow("expected ROE"))?;
    if expected_bp <= 0 {
        return Err(Unavailable::NonPositiveExpectedRoe { expected_bp });
    }
    let estimate = |roe_bp: i32, cost_bp: i32| -> Result<i128, Unavailable> {
        if cost_bp <= 0 {
            return Err(Unavailable::NonPositiveCost { cost_bp });
        }
        if roe_bp <= 0 {
            return Err(Unavailable::NonPositiveExpectedRoe {
                expected_bp: roe_bp,
            });
        }
        let scaled = equity
            .checked_mul(i128::from(roe_bp))
            .ok_or_else(|| overflow("equity x expected ROE"))?;
        let value = div_round_half_even(scaled, i128::from(cost_bp));
        if value <= 0 {
            return Err(Unavailable::NonPositiveValuationEstimate);
        }
        Ok(value)
    };
    Ok(ScenarioEstimates {
        central: estimate(expected_bp, assumptions.equity_cost_bp)?,
        pessimistic: estimate(
            expected_bp - ROE_SCENARIO_BP,
            assumptions.equity_cost_bp + COST_SCENARIO_BP,
        )?,
        optimistic: estimate(
            expected_bp + ROE_SCENARIO_BP,
            assumptions.equity_cost_bp - COST_SCENARIO_BP,
        )?,
    })
}
