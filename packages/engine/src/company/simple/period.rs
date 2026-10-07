use super::growth::GrowthFactor;
use crate::company::api::PeriodAmounts;
use crate::{
    accounting::AccountingAmount,
    calendar::CivilDate,
    company::{CompanySystemError, rng::OperatingRng},
};

#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub enum SettlementCycle {
    Monthly,
    Quarterly,
    HalfYear,
    Annual,
}

impl SettlementCycle {
    pub fn months(self) -> u8 {
        match self {
            Self::Monthly => 1,
            Self::Quarterly => 3,
            Self::HalfYear => 6,
            Self::Annual => 12,
        }
    }
    pub fn containing(self, date: CivilDate) -> Result<(CivilDate, CivilDate), CompanySystemError> {
        let start_month = ((date.month() - 1) / self.months()) * self.months() + 1;
        let end_month = start_month + self.months() - 1;
        Ok((
            CivilDate::from_ymd(date.year(), start_month, 1)?,
            if end_month == 12 {
                CivilDate::from_ymd(date.year(), 12, 31)?
            } else {
                CivilDate::from_ymd(date.year(), end_month + 1, 1)?.prev()?
            },
        ))
    }
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum AnnualTrendConfig {
    Fixed {
        annual_growth_bp: i32,
    },
    Persistent {
        annual_growth_min_bp: i32,
        annual_growth_max_bp: i32,
        duration_min_months: u16,
        duration_max_months: u16,
    },
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(deny_unknown_fields)]
pub struct PeriodNoiseConfig {
    pub monthly_bp: i32,
    pub quarterly_bp: i32,
    pub half_year_bp: i32,
    pub annual_bp: i32,
}
impl PeriodNoiseConfig {
    pub fn bound(&self, cycle: SettlementCycle) -> i32 {
        match cycle {
            SettlementCycle::Monthly => self.monthly_bp,
            SettlementCycle::Quarterly => self.quarterly_bp,
            SettlementCycle::HalfYear => self.half_year_bp,
            SettlementCycle::Annual => self.annual_bp,
        }
    }
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(tag = "rule", deny_unknown_fields)]
pub enum PeriodVariableExpenseRule {
    RevenueRatio {
        ratio_bp: i32,
        noise: PeriodNoiseConfig,
    },
    Growth {
        #[ts(type = "string")]
        initial_amount: AccountingAmount,
        trend: AnnualTrendConfig,
        noise: PeriodNoiseConfig,
    },
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(deny_unknown_fields)]
pub struct PeriodGenerationParameters {
    #[ts(type = "string")]
    pub initial_revenue: AccountingAmount,
    #[ts(type = "string")]
    pub initial_fixed_expense: AccountingAmount,
    pub revenue_trend: AnnualTrendConfig,
    pub fixed_expense_trend: AnnualTrendConfig,
    pub revenue_noise: PeriodNoiseConfig,
    pub fixed_expense_noise: PeriodNoiseConfig,
    pub demand_sensitivity_bp: i32,
    pub variable_expense: PeriodVariableExpenseRule,
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct AnnualTrendState {
    pub annual_growth_bp: i32,
    pub remaining_months: u16,
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeriodGenerationState {
    pub amounts: PeriodAmounts,
    pub rng: OperatingRng,
    pub revenue_trend: AnnualTrendState,
    pub fixed_expense_trend: AnnualTrendState,
    #[serde(deserialize_with = "crate::company::persistence::required_nullable")]
    pub variable_expense_trend: Option<AnnualTrendState>,
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct TrendSegment {
    pub annual_growth_bp: i32,
    pub months: u8,
}

/// 期间变化解释（F 批共同契约的解释查询 wire 类型；复用既有 history 数据）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PeriodChangeExplanation {
    pub previous: PeriodAmounts,
    pub cycle: SettlementCycle,
    pub environment_change_bp: i32,
    pub demand_contribution_bp: i32,
    pub revenue_segments: Vec<TrendSegment>,
    pub fixed_expense_segments: Vec<TrendSegment>,
    pub variable_expense_segments: Vec<TrendSegment>,
    pub revenue_noise_bp: i32,
    pub fixed_expense_noise_bp: i32,
    pub variable_expense_noise_bp: i32,
    /// 元字符串；无复业事实时为 null（显式区分「无」与缺失）。
    #[serde(deserialize_with = "crate::company::persistence::required_nullable")]
    #[ts(optional = false, type = "string | null")]
    pub restart_revenue: Option<AccountingAmount>,
    #[serde(deserialize_with = "crate::company::persistence::required_nullable")]
    #[ts(optional = false, type = "string | null")]
    pub restart_source: Option<String>,
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub struct PeriodGenerationResult {
    pub state: PeriodGenerationState,
    pub explanation: PeriodChangeExplanation,
}

impl PeriodGenerationParameters {
    pub fn validate(&self) -> Result<(), CompanySystemError> {
        if self.initial_revenue.is_negative() || self.initial_fixed_expense.is_negative() {
            return Err(CompanySystemError::Invalid(
                "期间基准营收及开支不得为负".into(),
            ));
        }
        self.revenue_trend.validate()?;
        self.fixed_expense_trend.validate()?;
        self.revenue_noise.validate()?;
        self.fixed_expense_noise.validate()?;
        match &self.variable_expense {
            PeriodVariableExpenseRule::RevenueRatio { ratio_bp, noise } => {
                noise.validate()?;
                if *ratio_bp < noise.maximum() || ratio_bp.checked_add(noise.maximum()).is_none() {
                    return Err(CompanySystemError::Invalid(
                        "变动开支比例扰动区间非法".into(),
                    ));
                }
            }
            PeriodVariableExpenseRule::Growth {
                initial_amount,
                trend,
                noise,
            } => {
                if initial_amount.is_negative() {
                    return Err(CompanySystemError::Invalid(
                        "变动开支期间基准不得为负".into(),
                    ));
                }
                trend.validate()?;
                noise.validate()?;
            }
        }
        Ok(())
    }
    pub fn initial_state(
        &self,
        mut rng: OperatingRng,
    ) -> Result<PeriodGenerationState, CompanySystemError> {
        self.validate()?;
        let revenue_trend = self.revenue_trend.sample(&mut rng);
        let fixed_expense_trend = self.fixed_expense_trend.sample(&mut rng);
        let (variable_expense, variable_expense_trend) = match &self.variable_expense {
            PeriodVariableExpenseRule::RevenueRatio { ratio_bp, .. } => {
                (self.initial_revenue.apply_basis_points(*ratio_bp)?, None)
            }
            PeriodVariableExpenseRule::Growth {
                initial_amount,
                trend,
                ..
            } => (*initial_amount, Some(trend.sample(&mut rng))),
        };
        Ok(PeriodGenerationState {
            amounts: PeriodAmounts {
                revenue: self.initial_revenue,
                fixed_expense: self.initial_fixed_expense,
                variable_expense,
            },
            rng,
            revenue_trend,
            fixed_expense_trend,
            variable_expense_trend,
        })
    }
}
pub fn generate_period(
    config: &PeriodGenerationParameters,
    previous: &PeriodGenerationState,
    cycle: SettlementCycle,
    environment_change_bp: i32,
    restart: Option<(AccountingAmount, String)>,
) -> Result<PeriodGenerationResult, CompanySystemError> {
    config.validate()?;
    previous.validate(config)?;
    if let Some((revenue, source)) = &restart {
        if !revenue.is_positive() || source.trim().is_empty() || !previous.amounts.revenue.is_zero()
        {
            return Err(CompanySystemError::Invalid(
                "复业必须是零基数的明确正营收基准".into(),
            ));
        }
    }
    let demand = AccountingAmount::from_cents(i128::from(environment_change_bp))
        .apply_basis_points(config.demand_sensitivity_bp)?
        .cents();
    let demand = i32::try_from(demand)
        .map_err(|_| CompanySystemError::Invalid("年化环境贡献超出基点范围".into()))?;
    let mut state = previous.clone();
    let (revenue_factor, revenue_segments) = advance_trend(
        &config.revenue_trend,
        &mut state.revenue_trend,
        &mut state.rng,
        cycle.months(),
        demand,
    )?;
    let (fixed_factor, fixed_segments) = advance_trend(
        &config.fixed_expense_trend,
        &mut state.fixed_expense_trend,
        &mut state.rng,
        cycle.months(),
        0,
    )?;
    let revenue_noise = sample_noise(&mut state.rng, config.revenue_noise.bound(cycle));
    let fixed_noise = sample_noise(&mut state.rng, config.fixed_expense_noise.bound(cycle));
    let mut revenue = revenue_factor
        .with_basis_point_noise(revenue_noise)?
        .apply(previous.amounts.revenue)?;
    if let Some((basis, _)) = &restart {
        revenue = *basis;
    }
    let fixed_expense = fixed_factor
        .with_basis_point_noise(fixed_noise)?
        .apply(previous.amounts.fixed_expense)?;
    let (variable_expense, variable_segments, variable_noise) = match &config.variable_expense {
        PeriodVariableExpenseRule::RevenueRatio { ratio_bp, noise } => {
            let shock = sample_noise(&mut state.rng, noise.bound(cycle));
            let ratio = ratio_bp
                .checked_add(shock)
                .ok_or_else(|| CompanySystemError::Invalid("变动开支比例溢出".into()))?;
            (revenue.apply_basis_points(ratio)?, Vec::new(), shock)
        }
        PeriodVariableExpenseRule::Growth { trend, noise, .. } => {
            let active = state
                .variable_expense_trend
                .as_mut()
                .ok_or_else(|| CompanySystemError::Invalid("缺少变动开支年化趋势".into()))?;
            let (factor, segments) =
                advance_trend(trend, active, &mut state.rng, cycle.months(), 0)?;
            let shock = sample_noise(&mut state.rng, noise.bound(cycle));
            (
                factor
                    .with_basis_point_noise(shock)?
                    .apply(previous.amounts.variable_expense)?,
                segments,
                shock,
            )
        }
    };
    state.amounts = PeriodAmounts {
        revenue,
        fixed_expense,
        variable_expense,
    };
    let explanation = PeriodChangeExplanation {
        previous: previous.amounts.clone(),
        cycle,
        environment_change_bp,
        demand_contribution_bp: demand,
        revenue_segments,
        fixed_expense_segments: fixed_segments,
        variable_expense_segments: variable_segments,
        revenue_noise_bp: revenue_noise,
        fixed_expense_noise_bp: fixed_noise,
        variable_expense_noise_bp: variable_noise,
        restart_revenue: restart.as_ref().map(|(basis, _)| *basis),
        restart_source: restart.map(|(_, source)| source),
    };
    Ok(PeriodGenerationResult { state, explanation })
}

impl AnnualTrendConfig {
    pub(crate) fn validate(&self) -> Result<(), CompanySystemError> {
        match self {
            Self::Fixed { annual_growth_bp } if *annual_growth_bp < -10_000 => {
                Err(CompanySystemError::Invalid("年化增长不得低于 -100%".into()))
            }
            Self::Persistent {
                annual_growth_min_bp,
                annual_growth_max_bp,
                duration_min_months,
                duration_max_months,
            } if *annual_growth_min_bp < -10_000
                || annual_growth_min_bp > annual_growth_max_bp
                || *duration_min_months == 0
                || duration_min_months > duration_max_months =>
            {
                Err(CompanySystemError::Invalid(
                    "持续趋势的年化区间或自然月持续时间非法".into(),
                ))
            }
            _ => Ok(()),
        }
    }
    fn sample(&self, rng: &mut OperatingRng) -> AnnualTrendState {
        match self {
            Self::Fixed { annual_growth_bp } => AnnualTrendState {
                annual_growth_bp: *annual_growth_bp,
                remaining_months: 0,
            },
            Self::Persistent {
                annual_growth_min_bp,
                annual_growth_max_bp,
                duration_min_months,
                duration_max_months,
            } => AnnualTrendState {
                annual_growth_bp: rng.range_i64(
                    i64::from(*annual_growth_min_bp),
                    i64::from(*annual_growth_max_bp),
                ) as i32,
                remaining_months: rng.range_i64(
                    i64::from(*duration_min_months),
                    i64::from(*duration_max_months),
                ) as u16,
            },
        }
    }
    fn contains(&self, rate: i32) -> bool {
        match self {
            Self::Fixed { annual_growth_bp } => rate == *annual_growth_bp,
            Self::Persistent {
                annual_growth_min_bp,
                annual_growth_max_bp,
                ..
            } => (*annual_growth_min_bp..=*annual_growth_max_bp).contains(&rate),
        }
    }
}
impl PeriodNoiseConfig {
    pub(crate) fn validate(&self) -> Result<(), CompanySystemError> {
        if [
            self.monthly_bp,
            self.quarterly_bp,
            self.half_year_bp,
            self.annual_bp,
        ]
        .iter()
        .any(|bound| *bound < 0)
        {
            Err(CompanySystemError::Invalid("期间扰动幅度不得为负".into()))
        } else {
            Ok(())
        }
    }
    fn maximum(&self) -> i32 {
        self.monthly_bp
            .max(self.quarterly_bp)
            .max(self.half_year_bp)
            .max(self.annual_bp)
    }
}
impl PeriodGenerationState {
    pub(crate) fn validate(
        &self,
        config: &PeriodGenerationParameters,
    ) -> Result<(), CompanySystemError> {
        for amount in [
            self.amounts.revenue,
            self.amounts.fixed_expense,
            self.amounts.variable_expense,
        ] {
            if amount.is_negative() {
                return Err(CompanySystemError::Invalid("期间营收及开支必须非负".into()));
            }
        }
        validate_trend(&config.revenue_trend, &self.revenue_trend)?;
        validate_trend(&config.fixed_expense_trend, &self.fixed_expense_trend)?;
        match (&config.variable_expense, &self.variable_expense_trend) {
            (PeriodVariableExpenseRule::RevenueRatio { .. }, None) => Ok(()),
            (PeriodVariableExpenseRule::Growth { trend, .. }, Some(state)) => {
                validate_trend(trend, state)
            }
            _ => Err(CompanySystemError::Invalid(
                "变动开支规则与保存的趋势不匹配".into(),
            )),
        }
    }
}
fn validate_trend(
    config: &AnnualTrendConfig,
    state: &AnnualTrendState,
) -> Result<(), CompanySystemError> {
    let valid = config.contains(state.annual_growth_bp)
        && match config {
            AnnualTrendConfig::Fixed { .. } => state.remaining_months == 0,
            AnnualTrendConfig::Persistent {
                duration_max_months,
                ..
            } => state.remaining_months <= *duration_max_months,
        };
    if valid {
        Ok(())
    } else {
        Err(CompanySystemError::Invalid(
            "保存的年化趋势或自然月进度不合法".into(),
        ))
    }
}
fn sample_noise(rng: &mut OperatingRng, bound: i32) -> i32 {
    rng.range_i64(-i64::from(bound), i64::from(bound)) as i32
}
fn advance_trend(
    config: &AnnualTrendConfig,
    active: &mut AnnualTrendState,
    rng: &mut OperatingRng,
    months: u8,
    annual_shift: i32,
) -> Result<(GrowthFactor, Vec<TrendSegment>), CompanySystemError> {
    let mut remaining = months;
    let mut segments: Vec<TrendSegment> = Vec::new();
    while remaining > 0 {
        if matches!(config, AnnualTrendConfig::Persistent { .. }) && active.remaining_months == 0 {
            *active = config.sample(rng);
        }
        let count = if matches!(config, AnnualTrendConfig::Fixed { .. }) {
            remaining
        } else {
            remaining.min(active.remaining_months.min(12) as u8)
        };
        if let Some(previous) = segments
            .last_mut()
            .filter(|previous| previous.annual_growth_bp == active.annual_growth_bp)
        {
            previous.months = previous
                .months
                .checked_add(count)
                .ok_or_else(|| CompanySystemError::Invalid("自然月趋势跨度溢出".into()))?;
        } else {
            segments.push(TrendSegment {
                annual_growth_bp: active.annual_growth_bp,
                months: count,
            });
        }
        if matches!(config, AnnualTrendConfig::Persistent { .. }) {
            active.remaining_months -= u16::from(count);
        }
        remaining -= count;
    }
    let mut factor = GrowthFactor::from_annual_basis_points(0, 1)?;
    for segment in &segments {
        let annual = segment
            .annual_growth_bp
            .checked_add(annual_shift)
            .ok_or_else(|| CompanySystemError::Invalid("环境与年化趋势合成溢出".into()))?;
        factor = factor.compose(GrowthFactor::from_annual_basis_points(
            annual,
            segment.months,
        )?)?;
    }
    Ok((factor, segments))
}

pub(crate) fn validate_generated_amounts(
    config: &PeriodGenerationParameters,
    amounts: &PeriodAmounts,
    explanation: &PeriodChangeExplanation,
) -> Result<(), CompanySystemError> {
    config.validate()?;
    let demand = AccountingAmount::from_cents(i128::from(explanation.environment_change_bp))
        .apply_basis_points(config.demand_sensitivity_bp)?
        .cents();
    if demand != i128::from(explanation.demand_contribution_bp) {
        return Err(CompanySystemError::Invalid(
            "环境年化贡献与敏感度不一致".into(),
        ));
    }
    for (actual, bound) in [
        (
            explanation.revenue_noise_bp,
            config.revenue_noise.bound(explanation.cycle),
        ),
        (
            explanation.fixed_expense_noise_bp,
            config.fixed_expense_noise.bound(explanation.cycle),
        ),
    ] {
        if i64::from(actual).abs() > i64::from(bound) {
            return Err(CompanySystemError::Invalid(
                "期间随机扰动超出配置跨度".into(),
            ));
        }
    }
    let factor = |trend: &AnnualTrendConfig,
                  segments: &[TrendSegment],
                  shift: i32|
     -> Result<GrowthFactor, CompanySystemError> {
        if segments
            .iter()
            .map(|segment| u32::from(segment.months))
            .sum::<u32>()
            != u32::from(explanation.cycle.months())
        {
            return Err(CompanySystemError::Invalid(
                "自然时间趋势分段没有精确覆盖结算期间".into(),
            ));
        }
        if segments
            .windows(2)
            .any(|pair| pair[0].annual_growth_bp == pair[1].annual_growth_bp)
        {
            return Err(CompanySystemError::Invalid(
                "相邻相同年化趋势必须保存为同一自然时间段".into(),
            ));
        }
        let mut factor = GrowthFactor::from_annual_basis_points(0, 1)?;
        for segment in segments {
            if segment.months == 0 || !trend.contains(segment.annual_growth_bp) {
                return Err(CompanySystemError::Invalid(
                    "趋势分段增长率或时长非法".into(),
                ));
            }
            let rate = segment
                .annual_growth_bp
                .checked_add(shift)
                .ok_or_else(|| CompanySystemError::Invalid("趋势合成年化增长溢出".into()))?;
            factor = factor.compose(GrowthFactor::from_annual_basis_points(
                rate,
                segment.months,
            )?)?;
        }
        Ok(factor)
    };
    let mut revenue = factor(
        &config.revenue_trend,
        &explanation.revenue_segments,
        explanation.demand_contribution_bp,
    )?
    .with_basis_point_noise(explanation.revenue_noise_bp)?
    .apply(explanation.previous.revenue)?;
    match (&explanation.restart_revenue, &explanation.restart_source) {
        (None, None) => {}
        (Some(basis), Some(source))
            if basis.is_positive()
                && explanation.previous.revenue.is_zero()
                && !source.trim().is_empty() =>
        {
            revenue = *basis
        }
        _ => return Err(CompanySystemError::Invalid("期间复业基准与说明非法".into())),
    }
    let fixed = factor(
        &config.fixed_expense_trend,
        &explanation.fixed_expense_segments,
        0,
    )?
    .with_basis_point_noise(explanation.fixed_expense_noise_bp)?
    .apply(explanation.previous.fixed_expense)?;
    let (variable, bound) = match &config.variable_expense {
        PeriodVariableExpenseRule::RevenueRatio { ratio_bp, noise } => {
            if !explanation.variable_expense_segments.is_empty() {
                return Err(CompanySystemError::Invalid(
                    "收入比例费用不得携带独立金额增长轨迹".into(),
                ));
            }
            let ratio = ratio_bp
                .checked_add(explanation.variable_expense_noise_bp)
                .ok_or_else(|| CompanySystemError::Invalid("费用比例溢出".into()))?;
            (
                revenue.apply_basis_points(ratio)?,
                noise.bound(explanation.cycle),
            )
        }
        PeriodVariableExpenseRule::Growth { trend, noise, .. } => (
            factor(trend, &explanation.variable_expense_segments, 0)?
                .with_basis_point_noise(explanation.variable_expense_noise_bp)?
                .apply(explanation.previous.variable_expense)?,
            noise.bound(explanation.cycle),
        ),
    };
    if i64::from(explanation.variable_expense_noise_bp).abs() > i64::from(bound)
        || amounts.revenue != revenue
        || amounts.fixed_expense != fixed
        || amounts.variable_expense != variable
    {
        return Err(CompanySystemError::Invalid(
            "期间账面生成结果与实际解释不一致".into(),
        ));
    }
    for amount in [
        amounts.revenue,
        amounts.fixed_expense,
        amounts.variable_expense,
        explanation.previous.revenue,
        explanation.previous.fixed_expense,
        explanation.previous.variable_expense,
    ] {
        if amount.is_negative() {
            return Err(CompanySystemError::Invalid("期间营收及费用不得为负".into()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn zeros() -> PeriodNoiseConfig {
        PeriodNoiseConfig {
            monthly_bp: 0,
            quarterly_bp: 0,
            half_year_bp: 0,
            annual_bp: 0,
        }
    }
    fn config() -> PeriodGenerationParameters {
        PeriodGenerationParameters {
            initial_revenue: AccountingAmount::from_cents(100_000),
            initial_fixed_expense: AccountingAmount::from_cents(30_000),
            revenue_trend: AnnualTrendConfig::Fixed {
                annual_growth_bp: 1200,
            },
            fixed_expense_trend: AnnualTrendConfig::Fixed {
                annual_growth_bp: 300,
            },
            revenue_noise: zeros(),
            fixed_expense_noise: zeros(),
            demand_sensitivity_bp: 0,
            variable_expense: PeriodVariableExpenseRule::RevenueRatio {
                ratio_bp: 4000,
                noise: zeros(),
            },
        }
    }
    #[test]
    fn each_cycle_applies_annual_growth_to_its_matching_full_period_baseline() {
        for cycle in [
            SettlementCycle::Monthly,
            SettlementCycle::Quarterly,
            SettlementCycle::HalfYear,
            SettlementCycle::Annual,
        ] {
            let config = config();
            let initial = config.initial_state(OperatingRng::from_state(17)).unwrap();
            let result = generate_period(&config, &initial, cycle, 0, None).unwrap();
            assert_eq!(
                result.state.amounts.revenue,
                GrowthFactor::from_annual_basis_points(1200, cycle.months())
                    .unwrap()
                    .apply(config.initial_revenue)
                    .unwrap()
            );
            assert_eq!(
                result.state.amounts.variable_expense,
                result
                    .state
                    .amounts
                    .revenue
                    .apply_basis_points(4000)
                    .unwrap()
            );
        }
    }
    #[test]
    fn persistent_trend_duration_counts_natural_months_not_settlements() {
        let mut config = config();
        config.revenue_trend = AnnualTrendConfig::Persistent {
            annual_growth_min_bp: 1200,
            annual_growth_max_bp: 1200,
            duration_min_months: 2,
            duration_max_months: 2,
        };
        let initial = config.initial_state(OperatingRng::from_state(17)).unwrap();
        let result =
            generate_period(&config, &initial, SettlementCycle::Quarterly, 0, None).unwrap();
        assert_eq!(
            result
                .explanation
                .revenue_segments
                .iter()
                .map(|segment| segment.months)
                .collect::<Vec<_>>(),
            vec![3]
        );
        assert_eq!(result.state.revenue_trend.remaining_months, 1);
    }
    #[test]
    fn period_noise_uses_selected_span_configuration_not_monthly_bound() {
        let mut config = config();
        config.revenue_noise.annual_bp = 10;
        let initial = config.initial_state(OperatingRng::from_state(17)).unwrap();
        let monthly =
            generate_period(&config, &initial, SettlementCycle::Monthly, 0, None).unwrap();
        assert_eq!(monthly.explanation.revenue_noise_bp, 0);
        let annual = generate_period(&config, &initial, SettlementCycle::Annual, 0, None).unwrap();
        assert!((-10..=10).contains(&annual.explanation.revenue_noise_bp));
    }
    #[test]
    fn arbitrary_start_date_belongs_to_complete_natural_cycle_not_partial_fake_report() {
        let date = CivilDate::from_iso("2030-01-07").unwrap();
        let (start, end) = SettlementCycle::Quarterly.containing(date).unwrap();
        assert_eq!(start.to_iso(), "2030-01-01");
        assert_eq!(end.to_iso(), "2030-03-31");
        assert_eq!(
            SettlementCycle::Annual.containing(date).unwrap().1.to_iso(),
            "2030-12-31"
        );
    }
    #[test]
    fn period_trends_and_rng_continue_after_strict_restore() {
        let mut config = config();
        config.revenue_trend = AnnualTrendConfig::Persistent {
            annual_growth_min_bp: -500,
            annual_growth_max_bp: 1200,
            duration_min_months: 2,
            duration_max_months: 5,
        };
        let initial = config.initial_state(OperatingRng::from_state(17)).unwrap();
        let first = generate_period(&config, &initial, SettlementCycle::HalfYear, 0, None).unwrap();
        let restored: PeriodGenerationState =
            serde_json::from_str(&serde_json::to_string(&first.state).unwrap()).unwrap();
        assert_eq!(
            generate_period(&config, &first.state, SettlementCycle::HalfYear, 0, None).unwrap(),
            generate_period(&config, &restored, SettlementCycle::HalfYear, 0, None).unwrap()
        );
    }

    #[test]
    fn revenue_and_expense_growth_produce_profit_inputs_once_and_allow_losses() {
        let mut config = config();
        config.revenue_trend = AnnualTrendConfig::Fixed {
            annual_growth_bp: 600,
        };
        config.fixed_expense_trend = AnnualTrendConfig::Fixed {
            annual_growth_bp: 300,
        };
        let initial = config.initial_state(OperatingRng::from_state(17)).unwrap();
        let result = generate_period(&config, &initial, SettlementCycle::Annual, 0, None).unwrap();
        assert_eq!(
            result.state.amounts,
            PeriodAmounts {
                revenue: AccountingAmount::from_cents(106_000),
                fixed_expense: AccountingAmount::from_cents(30_900),
                variable_expense: AccountingAmount::from_cents(42_400)
            }
        );
        assert_eq!(
            result
                .state
                .amounts
                .revenue
                .sub(result.state.amounts.fixed_expense)
                .unwrap()
                .sub(result.state.amounts.variable_expense)
                .unwrap()
                .cents(),
            32_700
        );
        config.revenue_trend = AnnualTrendConfig::Fixed {
            annual_growth_bp: -5000,
        };
        let initial = config.initial_state(OperatingRng::from_state(17)).unwrap();
        let loss = generate_period(&config, &initial, SettlementCycle::Annual, 0, None).unwrap();
        assert_eq!(
            loss.state
                .amounts
                .revenue
                .sub(loss.state.amounts.fixed_expense)
                .unwrap()
                .sub(loss.state.amounts.variable_expense)
                .unwrap()
                .cents(),
            -900
        );
        config.variable_expense = PeriodVariableExpenseRule::RevenueRatio {
            ratio_bp: 12_000,
            noise: zeros(),
        };
        let initial = config.initial_state(OperatingRng::from_state(17)).unwrap();
        let high_cost =
            generate_period(&config, &initial, SettlementCycle::Annual, 0, None).unwrap();
        assert!(high_cost.state.amounts.variable_expense > high_cost.state.amounts.revenue);
    }

    #[test]
    fn amount_expense_rule_is_exclusive_and_total_amount_uses_half_even() {
        let mut config = config();
        config.variable_expense = PeriodVariableExpenseRule::Growth {
            initial_amount: AccountingAmount::from_cents(20_000),
            trend: AnnualTrendConfig::Fixed {
                annual_growth_bp: 500,
            },
            noise: zeros(),
        };
        let initial = config.initial_state(OperatingRng::from_state(17)).unwrap();
        let first = generate_period(&config, &initial, SettlementCycle::Annual, 0, None).unwrap();
        let next =
            generate_period(&config, &first.state, SettlementCycle::Annual, 0, None).unwrap();
        assert_eq!(first.state.amounts.variable_expense.cents(), 21_000);
        assert_eq!(next.state.amounts.variable_expense.cents(), 22_050);
        config.initial_revenue = AccountingAmount::from_cents(1);
        config.initial_fixed_expense = AccountingAmount::from_cents(3);
        config.revenue_trend = AnnualTrendConfig::Fixed {
            annual_growth_bp: 5000,
        };
        config.fixed_expense_trend = AnnualTrendConfig::Fixed {
            annual_growth_bp: 5000,
        };
        config.variable_expense = PeriodVariableExpenseRule::Growth {
            initial_amount: AccountingAmount::from_cents(1),
            trend: AnnualTrendConfig::Fixed {
                annual_growth_bp: 5000,
            },
            noise: zeros(),
        };
        let initial = config.initial_state(OperatingRng::from_state(17)).unwrap();
        let ties = generate_period(&config, &initial, SettlementCycle::Annual, 0, None).unwrap();
        assert_eq!(
            ties.state.amounts,
            PeriodAmounts {
                revenue: AccountingAmount::from_cents(2),
                fixed_expense: AccountingAmount::from_cents(4),
                variable_expense: AccountingAmount::from_cents(2)
            }
        );
    }

    #[test]
    fn annual_maximum_rate_has_a_wide_executable_factor_not_i32_sum_overflow() {
        let mut config = config();
        config.revenue_trend = AnnualTrendConfig::Fixed {
            annual_growth_bp: i32::MAX,
        };
        config.fixed_expense_trend = AnnualTrendConfig::Fixed {
            annual_growth_bp: i32::MAX,
        };
        config.variable_expense = PeriodVariableExpenseRule::Growth {
            initial_amount: AccountingAmount::from_cents(1),
            trend: AnnualTrendConfig::Fixed {
                annual_growth_bp: i32::MAX,
            },
            noise: zeros(),
        };
        let initial = config.initial_state(OperatingRng::from_state(17)).unwrap();
        let result = generate_period(&config, &initial, SettlementCycle::Annual, 0, None).unwrap();
        assert_eq!(result.state.amounts.variable_expense.cents(), 214_749);
        assert!(result.state.amounts.revenue > initial.amounts.revenue);
    }

    #[test]
    fn shared_environment_changes_revenue_without_directly_overwriting_expense() {
        let mut config = config();
        config.demand_sensitivity_bp = 5000;
        let initial = config.initial_state(OperatingRng::from_state(17)).unwrap();
        let neutral =
            generate_period(&config, &initial, SettlementCycle::Quarterly, 0, None).unwrap();
        let changed =
            generate_period(&config, &initial, SettlementCycle::Quarterly, 200, None).unwrap();
        assert!(changed.state.amounts.revenue > neutral.state.amounts.revenue);
        assert_eq!(
            changed.state.amounts.fixed_expense,
            neutral.state.amounts.fixed_expense
        );
        assert_eq!(changed.explanation.demand_contribution_bp, 100);
        validate_generated_amounts(&config, &changed.state.amounts, &changed.explanation).unwrap();
    }

    #[test]
    fn identical_adjacent_trends_are_one_segment_and_one_root_conversion() {
        let mut config = config();
        config.revenue_trend = AnnualTrendConfig::Persistent {
            annual_growth_min_bp: 1200,
            annual_growth_max_bp: 1200,
            duration_min_months: 1,
            duration_max_months: 1,
        };
        let initial = config.initial_state(OperatingRng::from_state(17)).unwrap();
        let result =
            generate_period(&config, &initial, SettlementCycle::Quarterly, 0, None).unwrap();
        assert_eq!(
            result.explanation.revenue_segments,
            vec![TrendSegment {
                annual_growth_bp: 1200,
                months: 3
            }]
        );
        assert_eq!(
            result.state.amounts.revenue,
            GrowthFactor::from_annual_basis_points(1200, 3)
                .unwrap()
                .apply(config.initial_revenue)
                .unwrap()
        );
    }
}
