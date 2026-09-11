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
use crate::information::{AcquisitionError, NpcObservationContext, PublicationId};
use crate::orderbook::AccountId;

use super::Rng;
use super::analysis_profile::{AnalysisProfile, FundamentalMethod};
use super::fundamental::{
    BeliefCause, CauseRecord, FAILURE_CONFIDENCE_DELTA_BP, ForecastState,
    PROFITABLE_EXIT_CONFIDENCE_DELTA_BP, PersonalAssumptions, ValuationOutcome,
    ValuationUnavailable, draw_personal_assumptions,
};
use super::profile::StrategyProfile;

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
pub struct BeliefBook {
    pub(super) npc: AccountId,
    pub(super) profile: StrategyProfile,
    pub(super) analysis: AnalysisProfile,
    pub(super) assumptions: PersonalAssumptions,
    pub(super) entries: BTreeMap<StockCode, BeliefEntry>,
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
        Self {
            npc,
            profile,
            analysis,
            assumptions,
            entries: BTreeMap::new(),
        }
    }

    pub fn npc(&self) -> AccountId {
        self.npc
    }

    pub fn assumptions(&self) -> &PersonalAssumptions {
        &self.assumptions
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
