use super::{
    finance::SimpleFinanceState,
    period::{generate_period, PeriodGenerationState},
    SimpleConfig,
};
use crate::{
    accounting::AccountingAmount,
    calendar::CivilDate,
    company::{
        api::*,
        identity::IssuerRegistry,
        rng::{OperatingRng, RngStream},
        CompanyId, CompanySystemError,
    },
};
use std::collections::BTreeMap;

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SimpleCompanyState {
    pub generation: PeriodGenerationState,
    pub finance: SimpleFinanceState,
    #[serde(deserialize_with = "crate::company::persistence::required_nullable")]
    pub pending_restart: Option<(AccountingAmount, String)>,
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SimpleFundamentals {
    pub config: SimpleConfig,
    pub history_start: CivilDate,
    pub advanced_through: CivilDate,
    pub environment_change_bp: i32,
    pub environment_rng: OperatingRng,
    #[serde(deserialize_with = "crate::company::persistence::unique_map")]
    pub companies: BTreeMap<CompanyId, SimpleCompanyState>,
    pub history: Vec<SimpleDisclosureCandidate>,
}

impl SimpleFundamentals {
    pub fn create(
        issuers: &IssuerRegistry,
        config: SimpleConfig,
        start: CivilDate,
        seed: u64,
    ) -> Result<Self, CompanySystemError> {
        config.validate_for_issuers(issuers)?;
        let mut history_start = config.settlement_cycle.containing(start)?.0;
        for _ in 0..config.prehistory_periods {
            let previous = history_start.prev()?;
            history_start = config.settlement_cycle.containing(previous)?.0;
        }
        let baseline_end = history_start.prev()?;
        let mut companies = BTreeMap::new();
        for item in &config.companies {
            let generation = item.generation.initial_state(OperatingRng::derive(
                seed,
                RngStream::CompanyOperating,
                &format!("simple:{}", item.company.0),
            ))?;
            let kind = issuers
                .get(&item.company)
                .ok_or_else(|| CompanySystemError::Invalid("Simple 财务开账缺少发行人".into()))?
                .kind;
            let finance = SimpleFinanceState::create(
                item.company.clone(),
                kind,
                &item.finance,
                baseline_end,
            )?;
            companies.insert(
                item.company.clone(),
                SimpleCompanyState {
                    generation,
                    finance,
                    pending_restart: None,
                },
            );
        }
        let mut state = Self {
            environment_change_bp: config.environment.initial_change_bp,
            environment_rng: OperatingRng::derive(
                seed,
                RngStream::MarketShock,
                "simple-environment",
            ),
            config,
            history_start,
            advanced_through: baseline_end,
            companies,
            history: Vec::new(),
        };
        let last = start.prev()?;
        let mut cursor = history_start;
        while cursor <= last {
            state.advance_day(cursor)?;
            if cursor == last {
                break;
            }
            cursor = cursor.next()?;
        }
        state.validate(issuers)?;
        Ok(state)
    }

    pub fn advance_day(
        &mut self,
        date: CivilDate,
    ) -> Result<Vec<SimpleDisclosureCandidate>, CompanySystemError> {
        if self.advanced_through.next()? != date {
            return Err(CompanySystemError::Invalid(format!(
                "日期必须连续推进：上次 {}，本次 {date}",
                self.advanced_through
            )));
        }
        let (period_start, period_end) = self.config.settlement_cycle.containing(date)?;
        if date != period_end {
            self.advanced_through = date;
            return Ok(Vec::new());
        }
        let mut environment_rng = self.environment_rng.clone();
        let environment = super::environment::advance_environment(
            self.environment_change_bp,
            &self.config.environment,
            &mut environment_rng,
            self.config.settlement_cycle,
        )?;
        use rayon::prelude::*;
        let candidates: Result<Vec<_>, CompanySystemError> = self
            .config
            .companies
            .par_iter()
            .map(|config| {
                let previous = self
                    .companies
                    .get(&config.company)
                    .ok_or_else(|| CompanySystemError::Invalid("公司状态缺失".into()))?;
                let generated = generate_period(
                    &config.generation,
                    &previous.generation,
                    self.config.settlement_cycle,
                    environment,
                    previous.pending_restart.clone(),
                )?;
                let mut state = previous.clone();
                state
                    .finance
                    .apply_period(period_start, date, &generated.state.amounts)?;
                state.generation = generated.state;
                state.pending_restart = None;
                let candidate = SimpleDisclosureCandidate {
                    company: config.company.clone(),
                    period_start,
                    period_end: date,
                    amounts: state.generation.amounts.clone(),
                    explanation: generated.explanation,
                };
                Ok((state, candidate))
            })
            .collect();
        let mut generated = Vec::new();
        for (state, candidate) in candidates? {
            self.companies.insert(candidate.company.clone(), state);
            generated.push(candidate);
        }
        generated.sort_by(|left, right| left.company.cmp(&right.company));
        self.history.extend(generated.iter().cloned());
        self.environment_change_bp = environment;
        self.environment_rng = environment_rng;
        self.advanced_through = date;
        Ok(generated)
    }

    pub fn submit_command(&mut self, command: CompanyCommand) -> Result<(), CompanySystemError> {
        match command {
            CompanyCommand::RestartRevenue {
                company,
                revenue,
                source,
            } => {
                if !revenue.is_positive() || source.trim().is_empty() {
                    return Err(CompanySystemError::Invalid(
                        "复业需要正收入与明确场景说明".into(),
                    ));
                }
                let state = self.companies.get_mut(&company).ok_or_else(|| {
                    CompanySystemError::Invalid(format!("未知公司 {}", company.0))
                })?;
                if !state.generation.amounts.revenue.is_zero() || state.pending_restart.is_some() {
                    return Err(CompanySystemError::Invalid(
                        "仅零收入且没有待执行复业的公司可以复业".into(),
                    ));
                }
                state.pending_restart = Some((revenue, source));
                Ok(())
            }
        }
    }

    pub fn validate(&self, issuers: &IssuerRegistry) -> Result<(), CompanySystemError> {
        self.config.validate_for_issuers(issuers)?;
        if self.history_start.day() != 1
            || self.history_start > self.advanced_through.next()?
            || self.companies.len() != self.config.companies.len()
        {
            return Err(CompanySystemError::Invalid(
                "Simple 日期或公司状态集合不一致".into(),
            ));
        }
        let (current_start, current_end) = self
            .config
            .settlement_cycle
            .containing(self.advanced_through)?;
        let latest_end = if self.advanced_through == current_end {
            current_end
        } else {
            current_start.prev()?
        };
        for config in &self.config.companies {
            let state = self
                .companies
                .get(&config.company)
                .ok_or_else(|| CompanySystemError::Invalid("Simple 缺少公司状态".into()))?;
            state.finance.validate()?;
            state.generation.validate(&config.generation)?;
            if state.finance.company_id() != &config.company
                || state.finance.kind() != config.kind
                || state.finance.config() != &config.finance
                || state.finance.opening_date() != self.history_start.prev()?
                || state.finance.as_of() != latest_end
            {
                return Err(CompanySystemError::Invalid(
                    "Simple 财务身份、配置或期间不一致".into(),
                ));
            }
            for amount in [
                state.generation.amounts.revenue,
                state.generation.amounts.fixed_expense,
                state.generation.amounts.variable_expense,
            ] {
                if amount.is_negative() {
                    return Err(CompanySystemError::Invalid(
                        "Simple 月度营收和费用必须非负".into(),
                    ));
                }
            }
            if let Some((revenue, source)) = &state.pending_restart {
                if !revenue.is_positive()
                    || source.trim().is_empty()
                    || !state.generation.amounts.revenue.is_zero()
                {
                    return Err(CompanySystemError::Invalid("待执行复业不合法".into()));
                }
            }
            if self.history.is_empty()
                && state.generation.amounts
                    != config
                        .generation
                        .initial_state(OperatingRng::from_state(0))?
                        .amounts
            {
                return Err(CompanySystemError::Invalid(
                    "Simple 初始期间基准与明确配置不一致".into(),
                ));
            }
        }
        let sorted: BTreeMap<_, _> = self
            .config
            .companies
            .iter()
            .map(|item| (&item.company, item))
            .collect();
        let mut previous: BTreeMap<_, _> = sorted
            .iter()
            .map(|(id, config)| {
                Ok((
                    *id,
                    config
                        .generation
                        .initial_state(OperatingRng::from_state(0))?
                        .amounts,
                ))
            })
            .collect::<Result<_, CompanySystemError>>()?;
        let mut cursor = self.history_start;
        let mut offset = 0;
        while cursor <= latest_end {
            let end = self.config.settlement_cycle.containing(cursor)?.1;
            if end > latest_end {
                break;
            }
            for (company, config) in &sorted {
                let candidate = self
                    .history
                    .get(offset)
                    .ok_or_else(|| CompanySystemError::Invalid("Simple 月度历史缺失".into()))?;
                if &candidate.company != *company
                    || candidate.explanation.cycle != self.config.settlement_cycle
                    || candidate.period_start != cursor
                    || candidate.period_end != end
                    || candidate.explanation.previous != previous[company]
                {
                    return Err(CompanySystemError::Invalid(
                        "Simple 月度历史身份、期间或连续性非法".into(),
                    ));
                }
                super::period::validate_generated_amounts(
                    &config.generation,
                    &candidate.amounts,
                    &candidate.explanation,
                )?;
                previous.insert(company, candidate.amounts.clone());
                if end == latest_end
                    && self
                        .companies
                        .get(*company)
                        .map(|state| &state.generation.amounts)
                        != Some(&candidate.amounts)
                {
                    return Err(CompanySystemError::Invalid(
                        "Simple 最新月生成额与当前状态不一致".into(),
                    ));
                }
                offset += 1;
            }
            if end == latest_end {
                break;
            }
            cursor = end.next()?;
        }
        if offset != self.history.len() {
            return Err(CompanySystemError::Invalid(
                "Simple 历史包含多余或未来材料".into(),
            ));
        }
        Ok(())
    }
}
