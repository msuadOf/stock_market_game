//! 信念簿（K5 行 130–133，任务 18）：逐 (NPC, 股票) 的个人基本面预期状态。
//!
//! 估值纯属个人：本模块不存在任何设定/调整市场价格的路径（类型面上不引用
//! market/session——测试以行情快照 spy 锁定）；跨 NPC 也不共享估值。
//! 输入只来自本人已获知的公开报告（任务 16 `NpcObservationContext` 引用
//! 面），个人假设在构造时一次性抽定（每 profile 生命周期恰 6 次 f64，
//! canonical 序见 `draw_personal_assumptions` 文档）。信息驱动的更新语义
//! （λ 修订/直接重估/到期/经历）在 `fundamental/update.rs`。

use std::collections::{BTreeMap, BTreeSet};

use crate::account::StockCode;
use crate::accounting::reports::ReportKind;
use crate::company::{CompanyId, CompanyKind};
use crate::experience::RetailExperienceState;
use crate::information::{AcquisitionError, NpcObservationContext, PublicationId};
use crate::orderbook::AccountId;

use super::analysis_profile::{AnalysisProfile, FundamentalMethod};
use super::fundamental::{
    draw_personal_assumptions, BeliefCause, CauseRecord, ForecastState, PersonalAssumptions,
    ValuationOutcome, ValuationUnavailable, FAILURE_CONFIDENCE_DELTA_BP,
    PROFITABLE_EXIT_CONFIDENCE_DELTA_BP,
};
use super::profile::StrategyProfile;
use super::InstitutionExperiencePolicy;
use super::Rng;

/// 信念输入（调用方装配；`ctx` = 本人已知公开信息 + 可见行情引用面——
/// 行情不进入估值，仅供上下文携带）。
pub struct BeliefInputs<'a, Market> {
    pub ctx: &'a NpcObservationContext<'a, Market>,
    pub company: CompanyId,
    pub kind: CompanyKind,
    /// 发行人固定的已发行普通股总股数（比较报价前验证正分母；绝不使用
    /// 流通股数）。
    pub total_issued_shares: u64,
    pub as_of_trading_day: u64,
}

/// 信念编排错误（瞬态操作错误；**不可用状态存进条目**而非报错——NPC 保持
/// 运转，其他信号路径照常）。
#[derive(Debug, thiserror::Error)]
pub enum BeliefError {
    #[error("acquisition guard rejected the material: {0}")]
    Acquisition(#[from] AcquisitionError),
    #[error("valuation facts unavailable: {0}")]
    FactsUnavailable(#[from] ValuationUnavailable),
    #[error("material {report:?} does not belong to {company:?}")]
    MaterialNotForCompany {
        report: PublicationId,
        company: CompanyId,
    },
    #[error("material {report:?} is not an annual report (kind {kind:?})")]
    MaterialNotAnnual {
        report: PublicationId,
        kind: ReportKind,
    },
    #[error("horizon has {remaining_trading_days} trading day(s) remaining")]
    HorizonNotElapsed { remaining_trading_days: u64 },
    #[error("no belief entry for the stock yet")]
    NoBeliefEntry,
    #[error("no own-known annual material to re-estimate from")]
    NoOwnAnnualMaterial,
}

/// 逐股票信念条目（K5 行 130）。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BeliefEntry {
    pub company: CompanyId,
    /// 本次估值路径（`None` = 零基本权重——方法禁用，估值 Unavailable）。
    pub method: Option<FundamentalMethod>,
    pub forecast: ForecastState,
    /// 信心 0..=10000bp（行动权重，不是统计置信区间）。
    pub confidence_bp: u16,
    pub valuation: ValuationOutcome,
    /// 当前估值所用的本人已获知报告 id。
    pub used_report_ids: Vec<PublicationId>,
    pub anchor_trading_day: u64,
    pub horizon_trading_days: u16,
    pub last_cause: Option<CauseRecord>,
    /// 已消费的一次性经历订单 id（同一订单/事件只更新一次）。
    pub applied_experience_orders: BTreeSet<u64>,
}

/// 信念簿：每 NPC 一本（条目按股票键控）。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BeliefBook {
    pub(super) npc: AccountId,
    pub(super) profile: StrategyProfile,
    pub(super) analysis: AnalysisProfile,
    pub(super) assumptions: PersonalAssumptions,
    pub(super) entries: BTreeMap<StockCode, BeliefEntry>,
    /// 机构本人真实成交与本人观察的事实容器；复用散户的通用事实结构，不赋予机构散户买卖规则。
    pub(super) experience: RetailExperienceState,
    #[serde(deserialize_with = "deserialize_institution_policy")]
    pub(super) institution_policy: Option<InstitutionExperiencePolicy>,
    pub(super) institution_account_risk_paused: bool,
}

fn deserialize_institution_policy<'de, Decoder: serde::Deserializer<'de>>(
    decoder: Decoder,
) -> Result<Option<InstitutionExperiencePolicy>, Decoder::Error> {
    serde::Deserialize::deserialize(decoder)
}

impl BeliefBook {
    /// 构造并在此时一次性抽定个人假设（此后绝不再抽样——`apply_cause` 全
    /// 系列不接收 RNG）。
    pub fn new(
        npc: AccountId,
        profile: StrategyProfile,
        analysis: AnalysisProfile,
        rng: &mut dyn Rng,
    ) -> Self {
        let assumptions = draw_personal_assumptions(&profile, rng);
        let institution_policy = match &profile {
            StrategyProfile::Institution(style) => {
                Some(InstitutionExperiencePolicy::default_for_style(*style))
            }
            _ => None,
        };
        Self {
            npc,
            profile,
            analysis,
            assumptions,
            entries: BTreeMap::new(),
            experience: RetailExperienceState::without_equity_reference(),
            institution_policy,
            institution_account_risk_paused: false,
        }
    }

    pub fn npc(&self) -> AccountId {
        self.npc
    }

    pub fn profile(&self) -> &StrategyProfile {
        &self.profile
    }

    pub fn assumptions(&self) -> &PersonalAssumptions {
        &self.assumptions
    }

    pub fn experience(&self) -> &RetailExperienceState {
        &self.experience
    }

    pub fn experience_mut(&mut self) -> &mut RetailExperienceState {
        &mut self.experience
    }

    pub fn institution_policy(&self) -> Option<&InstitutionExperiencePolicy> {
        self.institution_policy.as_ref()
    }

    pub(crate) fn institution_account_risk_paused(&self) -> bool {
        self.institution_account_risk_paused
    }

    /// 在本人观察时按 frozen policy 更新账户风险记忆，无 peak 时保持既有 latch。
    pub(crate) fn observe_institution_account_risk(
        &mut self,
        equity: crate::Money,
        moment: crate::experience::ExperienceMoment,
    ) -> Result<(), crate::experience::ExperienceError> {
        if let Some(paused) = self.assess_institution_account_risk(equity, moment)? {
            self.institution_account_risk_paused = paused;
        }
        Ok(())
    }

    fn assess_institution_account_risk(
        &self,
        equity: crate::Money,
        moment: crate::experience::ExperienceMoment,
    ) -> Result<Option<bool>, crate::experience::ExperienceError> {
        let policy = self
            .institution_policy
            .as_ref()
            .expect("institution observation has a frozen policy");
        let experience = &self.experience;
        let paused = self.institution_account_risk_paused;
        if equity.cents() < 0 {
            return Err(crate::experience::ExperienceError::NonPositiveMoney {
                field: "institution equity",
                cents: equity.cents(),
            });
        }
        if let Some(peak) = experience.peak_equity.filter(|peak| peak.cents() <= 0) {
            return Err(crate::experience::ExperienceError::NonPositiveMoney {
                field: "institution equity peak",
                cents: peak.cents(),
            });
        }
        experience.feedback.ensure_as_of_reached(&moment)?;
        let failure_decay = experience
            .feedback
            .failure_events
            .last()
            .map_or(0, |event| {
                (moment.trading_day - event.moment.trading_day)
                    / crate::experience::FAILURE_DECAY_TRADING_DAYS
            });
        let failure_influence =
            (experience.feedback.failure_events.len() as u64).saturating_sub(failure_decay);
        if failure_influence >= u64::from(policy.risk_pause_failed_buys()) {
            return Ok(Some(true));
        }
        let threshold = if paused {
            policy.risk_resume_drawdown_bp()
        } else {
            policy.risk_pause_drawdown_bp()
        };
        Ok(experience.peak_equity.map(|peak| {
            let decline = i128::from(peak.cents()) - i128::from(equity.cents());
            let boundary = i128::from(peak.cents()) * i128::from(threshold);
            if paused {
                decline * 10_000 > boundary
            } else {
                decline * 10_000 >= boundary
            }
        }))
    }

    pub(crate) fn set_institution_policy(&mut self, policy: InstitutionExperiencePolicy) {
        assert!(
            matches!(self.profile, StrategyProfile::Institution(_)),
            "only institution books can hold an institution policy"
        );
        self.institution_policy = Some(policy);
    }

    /// 个体分析档案（K5a 混合权重面；任务 26 会话决策链接线读取）。
    pub fn analysis(&self) -> &AnalysisProfile {
        &self.analysis
    }

    pub fn entry(&self, stock: &StockCode) -> Option<&BeliefEntry> {
        self.entries.get(stock)
    }

    /// 已有信念条目的股票键（StockCode 序；候选集组装用）。
    pub fn entry_stocks(&self) -> impl Iterator<Item = &StockCode> {
        self.entries.keys()
    }

    /// 唯一变更入口：显式 cause（新材料/更正/违约/到期/经历）。无触发 ⇒
    /// 调用方根本不该调（模块不存在无 cause 的变更路径——no-trigger-stable
    /// 由测试以字节对比锁定）。
    pub fn apply_cause<M>(
        &mut self,
        stock: &StockCode,
        cause: BeliefCause,
        inputs: &BeliefInputs<'_, M>,
    ) -> Result<&BeliefEntry, BeliefError> {
        match cause {
            BeliefCause::ExperienceFailure { order } => {
                self.apply_experience(
                    stock,
                    cause,
                    order,
                    FAILURE_CONFIDENCE_DELTA_BP,
                    inputs.as_of_trading_day,
                )?;
            }
            BeliefCause::ProfitableExit { order } => {
                self.apply_experience(
                    stock,
                    cause,
                    order,
                    PROFITABLE_EXIT_CONFIDENCE_DELTA_BP,
                    inputs.as_of_trading_day,
                )?;
            }
            BeliefCause::NewMaterial { report } => {
                self.apply_material(stock, cause, report, inputs, false)?;
            }
            BeliefCause::Correction { report } => {
                self.apply_material(stock, cause, report, inputs, true)?;
            }
            BeliefCause::CreditDefault { announcement } => {
                self.apply_credit_default(stock, cause, announcement, inputs)?;
            }
            BeliefCause::HorizonExpired => self.apply_horizon_expiry(stock, cause, inputs)?,
        }
        Ok(self
            .entries
            .get(stock)
            .expect("every cause path leaves an entry (formation or existing)"))
    }
}

#[cfg(test)]
mod risk_owner_tests {
    use super::*;
    use crate::experience::{ExperienceError, ExperienceMoment, FailureEventRecord};
    use crate::strategy::{derive_analysis_profile, InstitutionStyle};
    use crate::{CivilDate, Money};

    fn moment(minute: u64, day: u64) -> ExperienceMoment {
        ExperienceMoment {
            civil_date: CivilDate::from_iso("2030-01-07").unwrap(),
            market_minute: minute,
            trading_day: day,
        }
    }

    fn book(paused: bool, peak: Option<i64>) -> BeliefBook {
        let profile = StrategyProfile::Institution(InstitutionStyle::DeepValue);
        let mut rng = crate::session::SplitMix64::new(42);
        let analysis = derive_analysis_profile(&profile, AccountId(1), &mut rng).unwrap();
        let mut book = BeliefBook::new(AccountId(1), profile, analysis, &mut rng);
        book.institution_policy = Some(
            InstitutionExperiencePolicy::new(
                1,
                crate::strategy::InstitutionLossResponse::HoldOrAdd,
                800,
                1200,
                3000,
                1500,
                3,
                500,
            )
            .unwrap(),
        );
        book.institution_account_risk_paused = paused;
        book.experience.peak_equity = peak.map(Money::from_cents);
        book
    }

    #[test]
    fn risk_latch_without_peak_preserves_memory_but_real_failures_can_pause() {
        for paused in [false, true] {
            let mut book = book(paused, None);
            book.observe_institution_account_risk(Money::ZERO, moment(4, 0))
                .unwrap();
            assert_eq!(book.institution_account_risk_paused(), paused);
            for order in 1..=3 {
                book.experience
                    .feedback
                    .failure_events
                    .push(FailureEventRecord {
                        code: StockCode("600101".to_owned()),
                        order_id: Some(order),
                        moment: moment(order, 0),
                    });
            }
            book.observe_institution_account_risk(Money::ZERO, moment(4, 0))
                .unwrap();
            assert!(book.institution_account_risk_paused());
            book.observe_institution_account_risk(Money::ZERO, moment(5, 20))
                .unwrap();
            assert!(book.institution_account_risk_paused());
            assert_eq!(book.experience.feedback.failure_events.len(), 3);
        }
    }

    #[test]
    fn risk_latch_trigger_and_recovery_keep_distinct_equality_boundaries() {
        for (paused, equity, expected) in [
            (false, 7001, false),
            (false, 7000, true),
            (false, 6999, true),
            (true, 8499, true),
            (true, 8500, false),
            (true, 8501, false),
        ] {
            let mut book = book(paused, Some(10_000));
            book.observe_institution_account_risk(Money::from_cents(equity), moment(1, 0))
                .unwrap();
            assert_eq!(book.institution_account_risk_paused(), expected);
            let encoded = serde_json::to_value(&book).unwrap();
            assert_eq!(
                encoded["institution_account_risk_paused"],
                serde_json::json!(expected)
            );
            let restored: BeliefBook = serde_json::from_value(encoded).unwrap();
            assert_eq!(restored, book);
        }
    }

    #[test]
    fn risk_latch_invalid_facts_preserve_memory_and_error_order() {
        let mut negative = book(true, Some(0));
        assert!(matches!(
            negative.observe_institution_account_risk(Money::from_cents(-1), moment(1, 0)),
            Err(ExperienceError::NonPositiveMoney {
                field: "institution equity",
                ..
            })
        ));
        assert!(negative.institution_account_risk_paused());
        for peak in [0, -1] {
            let mut invalid = book(false, Some(peak));
            assert!(matches!(
                invalid.observe_institution_account_risk(Money::ZERO, moment(1, 0)),
                Err(ExperienceError::NonPositiveMoney {
                    field: "institution equity peak",
                    ..
                })
            ));
            assert!(!invalid.institution_account_risk_paused());
        }
        let mut future = book(false, Some(10_000));
        future.experience.feedback.latest_moment = Some(moment(2, 0));
        assert!(matches!(
            future.observe_institution_account_risk(Money::ZERO, moment(1, 0)),
            Err(ExperienceError::AsOfBeforeLatestEvent { .. })
        ));
        assert!(!future.institution_account_risk_paused());
    }
}
