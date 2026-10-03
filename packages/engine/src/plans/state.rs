//! 个人交易计划的权威状态类型：账户+股票唯一、可跨日、只以真实成交计进度。
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
/// 股数目标才有股数级完成/超额语义；比例目标由预算换算层转成股数。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub enum PlanTarget {
    /// 目标股数（正数）。
    ShareCount(u32),
    /// 目标仓位，0..=10000 基点。
    PositionFractionBp(u32),
}

/// 执行紧迫度：独立于估值的执行状态字段，决策逻辑在 urgency 中实现。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub enum Urgency {
    Patient,
    Normal,
    Urgent,
}

/// 暂停原因：跌势加速 / 风险压力 / 被动不利选择。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub enum PauseReason {
    IntradayDropAcceleration,
    RiskPressure,
    AdverseSelection,
}

/// 恢复原因：恢复须下一本人观察。
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

/// 个人观点来源（分析族）。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub enum OpinionSource {
    Fundamental,
    Trend,
    PriceVolume,
    Technical,
    Experience,
    Blended,
}

/// 观点快照：综合判断分数 S（由上游计算）与来源。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct PlanOpinion {
    pub signal_score_bp: i32,
    pub source: OpinionSource,
}

/// 复核条件：触发下次复核的阈值与上次复核基线；由会话决策链基于这些条件决定是否复核。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct ReviewConditions {
    pub min_signal_delta_bp: i32,
    pub min_price_change_bp: i32,
    pub last_review_signal_score_bp: i32,
    pub last_review_trading_day: u64,
    /// 上次本人实际复核时该发行人的价格与个人信息获取次数。
    pub last_review_price: Option<crate::Money>,
    pub last_review_acquired_count: u32,
    #[serde(deserialize_with = "deserialize_review_resources")]
    pub last_review_resources: Option<ReviewResources>,
}

/// 上次本人复核时的资源事实，而非当前账户的另一份预留账本。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
pub struct ReviewResources {
    pub cash: crate::Money,
    pub frozen_cash: crate::Money,
    pub held_qty: u32,
    pub t1_locked: u32,
}

impl ReviewResources {
    pub(crate) fn is_valid(self) -> bool {
        self.cash >= crate::Money::ZERO
            && self.frozen_cash >= crate::Money::ZERO
            && self.frozen_cash <= self.cash
            && self.t1_locked <= self.held_qty
    }
}

fn deserialize_review_resources<'de, Decoder: serde::Deserializer<'de>>(
    decoder: Decoder,
) -> Result<Option<ReviewResources>, Decoder::Error> {
    serde::Deserialize::deserialize(decoder)
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
    /// 信心，0..=10000 bp。
    pub confidence_bp: u32,
    pub urgency: Urgency,
    /// 有效期（交易日数，日内策略为 1，其他风格可跨日）。
    pub horizon_trading_days: u32,
    pub created_trading_day: u64,
}

/// 可跨日的个人交易计划：每账户+股票唯一、版本化修订、真实成交计进度。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct TradingPlan {
    pub(in crate::plans) plan_id: PlanId,
    pub(in crate::plans) account: AccountId,
    pub(in crate::plans) code: StockCode,
    pub(in crate::plans) direction: Side,
    pub(in crate::plans) target: PlanTarget,
    /// 累计真实成交（按当前方向计；反向修订开新进度，旧腿真实成交留在账户里）。
    pub(in crate::plans) filled_qty: u32,
    pub(in crate::plans) opinion: PlanOpinion,
    pub(in crate::plans) confidence_bp: u32,
    pub(in crate::plans) urgency: Urgency,
    pub(in crate::plans) status: PlanStatus,
    /// 修订版本号，开户为 1，每次修订 +1。
    pub(in crate::plans) version: u32,
    pub(in crate::plans) last_revision: Option<super::revision::RevisionRecord>,
    /// 最近一次恢复携带的显式原因（恢复交易日见 last_event_trading_day）。
    pub(in crate::plans) last_resume: Option<ResumeReason>,
    pub(in crate::plans) created_trading_day: u64,
    pub(in crate::plans) horizon_trading_days: u32,
    pub(in crate::plans) review: ReviewConditions,
    /// 对现有母单子单的可选引用；日终清除（仅结束子单生命周期，不结束计划）。
    pub(in crate::plans) active_child_order_id: Option<OrderId>,
    /// 最近一次已接受事件的交易日，事件时间必须单调不减。
    pub(in crate::plans) last_event_trading_day: u64,
}

impl TradingPlan {
    pub fn code(&self) -> &StockCode {
        &self.code
    }

    pub fn plan_id(&self) -> PlanId {
        self.plan_id
    }

    pub fn account(&self) -> AccountId {
        self.account
    }

    pub fn direction(&self) -> Side {
        self.direction
    }

    pub fn target(&self) -> PlanTarget {
        self.target
    }

    pub fn filled_qty(&self) -> u32 {
        self.filled_qty
    }

    pub fn opinion(&self) -> PlanOpinion {
        self.opinion
    }

    pub fn confidence_bp(&self) -> u32 {
        self.confidence_bp
    }

    pub fn urgency(&self) -> Urgency {
        self.urgency
    }

    pub fn status(&self) -> PlanStatus {
        self.status
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn last_revision(&self) -> Option<super::revision::RevisionRecord> {
        self.last_revision
    }

    pub fn last_resume(&self) -> Option<ResumeReason> {
        self.last_resume
    }

    pub fn created_trading_day(&self) -> u64 {
        self.created_trading_day
    }

    pub fn horizon_trading_days(&self) -> u32 {
        self.horizon_trading_days
    }

    pub fn review(&self) -> ReviewConditions {
        self.review
    }

    pub fn active_child_order_id(&self) -> Option<OrderId> {
        self.active_child_order_id
    }

    pub fn last_event_trading_day(&self) -> u64 {
        self.last_event_trading_day
    }

    /// 本人复核的临时紧迫度输入；只改变副本，不生成修订事件或修改权威计划。
    pub(crate) fn review_preview(&self, direction: Side, confidence_bp: u32) -> Self {
        let mut preview = self.clone();
        preview.direction = direction;
        preview.confidence_bp = confidence_bp;
        preview
    }

    /// 资源事实已由 PlanBook 在按 id 查询前校验；此处只记录该计划的复核基线。
    pub(in crate::plans) fn record_resource_review(&mut self, resources: ReviewResources) {
        self.review.last_review_resources = Some(resources);
    }

    /// 记录本计划实际观察到的事实，不预留账户资产。
    pub(in crate::plans) fn record_review(
        &mut self,
        trading_day: u64,
        price: crate::Money,
        acquired_count: u32,
    ) -> Result<(), PlanError> {
        self.ensure_event_allowed("review", trading_day, false)?;
        if price.cents() <= 0 {
            return Err(PlanError::SaveInconsistent {
                detail: format!("plan {:?} reviewed a nonpositive price", self.plan_id),
            });
        }
        self.review.last_review_trading_day = trading_day;
        self.review.last_review_price = Some(price);
        self.review.last_review_acquired_count = acquired_count;
        self.last_event_trading_day = trading_day;
        Ok(())
    }

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
                last_review_price: None,
                last_review_acquired_count: 0,
                last_review_resources: None,
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

#[cfg(test)]
mod owner_tests {
    use super::*;

    #[test]
    fn review_preview_preserves_authoritative_plan_and_wire_facts() {
        let plan = TradingPlan::from_open(
            PlanId(7),
            PlanOpen {
                account: AccountId(1),
                code: StockCode("600101".to_owned()),
                direction: Side::Buy,
                target: PlanTarget::ShareCount(100),
                opinion: PlanOpinion {
                    signal_score_bp: 3000,
                    source: OpinionSource::Blended,
                },
                confidence_bp: 8000,
                urgency: Urgency::Normal,
                horizon_trading_days: 5,
                created_trading_day: 0,
            },
            &PlanPolicy::default(),
        )
        .unwrap();
        let original = serde_json::to_value(&plan).unwrap();
        let preview = plan.review_preview(Side::Sell, 5000);
        let mut expected = original.clone();
        expected["direction"] = serde_json::json!(Side::Sell);
        expected["confidence_bp"] = serde_json::json!(5000);
        assert_eq!(serde_json::to_value(&preview).unwrap(), expected);
        assert_eq!(serde_json::to_value(&plan).unwrap(), original);
        let restored: TradingPlan = serde_json::from_value(expected).unwrap();
        assert_eq!(restored, preview);
    }
}
