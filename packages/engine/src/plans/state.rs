//! 个人交易计划的权威状态类型（K6）：账户+股票唯一、可跨日、只以真实成交计进度。
//!
//! 复用既有身份类型（AccountId/StockCode/Side/OrderId），不建立平行体系；
//! 对母单执行子状态仅持可选订单 id 引用，不复制订单/冻结逻辑。
//! 事件转移（观察/接受/成交/修订/暂停/恢复/到期/日终）见 [`super::revision`]。

use super::validation::{validate_open, PlanError};
use super::PlanPolicy;
use crate::account::StockCode;
use crate::orderbook::{AccountId, OrderId, Side};

/// 计划稳定 id：由 [`super::PlanBook`] 单调分配，永不复用。
#[derive(
    Copy,
    Clone,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    ts_rs::TS,
)]
#[ts(type = "number")]
#[serde(transparent)]
pub struct PlanId(#[serde(with = "crate::orderbook::js_safe_u64")] pub u64);

/// 计划目标：目标仓位（权益占比 bp）或目标股数，两者都可表达。
/// 份额目标才有份额级完成/超额语义；比例目标由预算换算层（任务 22）转成股数。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub enum PlanTarget {
    /// 目标股数（正数）。
    ShareCount(u32),
    /// 目标仓位，0..=10000 基点。
    PositionFractionBp(u32),
}

/// 执行紧迫度（K6）：独立于估值的执行状态字段，决策逻辑属任务 23。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub enum Urgency {
    Patient,
    Normal,
    Urgent,
}

/// 暂停原因（K6：跌势加速 / 风险压力 / 被动不利选择）。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub enum PauseReason {
    IntradayDropAcceleration,
    RiskPressure,
    AdverseSelection,
}

/// 恢复原因（K5a：恢复须下一本人观察）。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub enum ResumeReason {
    /// 暂停触发条件解除。
    TriggerCleared,
    /// 本人观察后重新确认观点。
    OpinionReaffirmed,
}

/// 终止原因：到期 / 无资金 / 撤销 / 超目标成交。绝不用于冒充真实完成。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub enum TerminationReason {
    /// 有效期届满。
    HorizonExpired,
    /// 可用资金不足以继续执行（数据化原因，不调用引擎）。
    FundsUnavailable,
    /// 主动撤销。
    Cancelled,
    /// 真实成交超过目标（零股等边缘），如实入账后终止。
    FilledBeyondTarget,
}

/// 计划状态：Active / Paused / Completed / Terminated。
/// `Completed` 仅用于真实完成（累计真实成交到达份额目标）。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub enum PlanStatus {
    Active,
    Paused { reason: PauseReason },
    Completed,
    Terminated { reason: TerminationReason },
}

/// 个人观点来源（K5 分析族）。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub enum OpinionSource {
    Fundamental,
    Trend,
    PriceVolume,
    Technical,
    Experience,
    Blended,
}

/// 观点快照：综合判断分数（K5a 的 S，由上游计算）与来源。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct PlanOpinion {
    pub signal_score_bp: i32,
    pub source: OpinionSource,
}

/// 复核条件（K5a）：触发下次复核的阈值与上次复核基线；是否复核由任务 22 决定。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct ReviewConditions {
    pub min_signal_delta_bp: i32,
    pub min_price_change_bp: i32,
    pub last_review_signal_score_bp: i32,
    pub last_review_trading_day: u64,
}

/// 开户请求：一个账户对一只股票开一个方向的计划（类型化参数组）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PlanOpen {
    pub account: AccountId,
    pub code: StockCode,
    pub direction: Side,
    pub target: PlanTarget,
    pub opinion: PlanOpinion,
    /// 信心，0..=10000 bp（K5）。
    pub confidence_bp: u32,
    pub urgency: Urgency,
    /// 有效期（交易日数，按风格 5/20/60；参数化，不硬编码单一风格）。
    pub horizon_trading_days: u32,
    pub created_trading_day: u64,
}

/// 可跨日的个人交易计划（K6）：唯一 per 账户+股票，版本化修订，真实成交计进度。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct TradingPlan {
    pub plan_id: PlanId,
    pub account: AccountId,
    pub code: StockCode,
    pub direction: Side,
    pub target: PlanTarget,
    /// 累计真实成交（按当前方向计；反向修订开新进度，旧腿真实成交留在账户里）。
    pub filled_qty: u32,
    pub opinion: PlanOpinion,
    pub confidence_bp: u32,
    pub urgency: Urgency,
    pub status: PlanStatus,
    /// 修订版本号，开户为 1，每次修订 +1。
    pub version: u32,
    pub last_revision: Option<super::revision::RevisionRecord>,
    /// 最近一次恢复携带的显式原因（恢复交易日见 last_event_trading_day）。
    pub last_resume: Option<ResumeReason>,
    pub created_trading_day: u64,
    pub horizon_trading_days: u32,
    pub review: ReviewConditions,
    /// 对现有母单子单的可选引用；日终清除（仅结束子单生命周期，不结束计划）。
    pub active_child_order_id: Option<OrderId>,
    /// 最近一次已接受事件的交易日，事件时间必须单调不减。
    pub last_event_trading_day: u64,
}

impl TradingPlan {
    /// 由开户请求构造（先验证字段）。
    pub fn from_open(
        plan_id: PlanId,
        open: PlanOpen,
        policy: &PlanPolicy,
    ) -> Result<Self, PlanError> {
        validate_open(&open)?;
        Ok(Self {
            plan_id,
            account: open.account,
            code: open.code,
            direction: open.direction,
            target: open.target,
            filled_qty: 0,
            opinion: open.opinion,
            confidence_bp: open.confidence_bp,
            urgency: open.urgency,
            status: PlanStatus::Active,
            version: 1,
            last_revision: None,
            last_resume: None,
            created_trading_day: open.created_trading_day,
            horizon_trading_days: open.horizon_trading_days,
            review: ReviewConditions {
                min_signal_delta_bp: policy.review_signal_delta_bp,
                min_price_change_bp: policy.review_price_change_bp,
                last_review_signal_score_bp: open.opinion.signal_score_bp,
                last_review_trading_day: open.created_trading_day,
            },
            active_child_order_id: None,
            last_event_trading_day: open.created_trading_day,
        })
    }

    /// 有效期覆盖的最后一个交易日（含开户日共 `horizon_trading_days` 天）。
    pub fn last_valid_trading_day(&self) -> u64 {
        self.created_trading_day + u64::from(self.horizon_trading_days) - 1
    }

    /// 份额目标下的剩余数量；比例目标或剩余已非正时为 `None`。
    pub fn remaining_share_qty(&self) -> Option<u32> {
        match self.target {
            PlanTarget::ShareCount(target_qty) => target_qty.checked_sub(self.filled_qty),
            PlanTarget::PositionFractionBp(_) => None,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            PlanStatus::Completed | PlanStatus::Terminated { .. }
        )
    }

    /// 显式终止（撤销 / 无资金等）：Active 与 Paused 均可终止。
    pub(super) fn terminate(
        &mut self,
        reason: TerminationReason,
        trading_day: u64,
    ) -> Result<(), PlanError> {
        self.ensure_event_allowed("terminate", trading_day, false)?;
        self.status = PlanStatus::Terminated { reason };
        self.last_event_trading_day = trading_day;
        Ok(())
    }
}
