//! 可跨日的个人交易计划状态机（K6）。
//!
//! 纯状态机：转移是 (状态, 事件) 的确定性纯函数，无随机数、无 I/O。
//! 计划不冻结资产，不建立第二套订单/冻结系统；对现有 `ParentOrderPlan`
//! 执行子状态仅持可选引用（真实路由在任务 24/26 接入）。
//! 预算分配与紧迫度报价决策属任务 22/23。

mod allocation;
mod candidates;
pub mod quote_policy;
mod revision;
mod state;
pub mod urgency;
mod validation;

pub use allocation::{
    allocate_soft_budgets, read_allocation_experience, AllocationClass, AllocationConstraint,
    AllocationError, AllocationExperience, AllocationFunds, AllocationGrant, AllocationPolicy,
    AllocationRequest, AllocationResult, ExperienceHolding, ExperienceReadRequest,
};
pub use candidates::{
    blend_candidate, eligible_candidates, experience_cost_signal, fundamental_range_signal,
    fundamental_signal, normalized_score, price_volume_signal, target_position_weight_bp,
    target_share_quantity, technical_signal, trend_signal, CandidateAssessment, CandidateError,
    CandidateSignals, ExcludedSignal, QuantityRounding, SignalComponent, SignalContribution,
    SignalScore, SignalUnavailableReason, TargetShareQuantity,
};
pub use quote_policy::{
    decide_quote, ActiveQuote, BookTop, QuoteAction, QuoteDecision, QuoteDecisionInputs,
    QuoteError, QuoteReason,
};
pub use revision::{PlanRevision, RevisionReason, RevisionRecord};
pub use state::{
    OpinionSource, PauseReason, PlanId, PlanOpen, PlanOpinion, PlanStatus, PlanTarget,
    ResumeReason, ReviewConditions, TerminationReason, TradingPlan, Urgency,
};
pub use urgency::{
    assess_recovery, assess_urgency, PatienceStyle, PauseAssessment, RecoveryAssessment,
    RecoveryInputs, UrgencyAssessment, UrgencyError, UrgencyInputs, UrgencyPolicy, UrgencyReason,
    URGENCY_POLICY_VERSION,
};
pub use validation::{reverse_crosses_threshold, PlanError};

use std::collections::BTreeMap;
use validation::{validate_open, validate_policy};

use crate::account::StockCode;
use crate::orderbook::{AccountId, OrderId};

/// K6：个人期限按风格 5/20/60 交易日；风格→期限映射属任务 17，这里只固定可测试参数。
pub const HORIZON_TRADING_DAYS_SHORT: u32 = 5;
pub const HORIZON_TRADING_DAYS_MEDIUM: u32 = 20;
pub const HORIZON_TRADING_DAYS_LONG: u32 = 60;

/// 计划政策（K5a 固定游戏参数，随 save 固化）。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PlanPolicy {
    /// 反向修订必须越过的另一侧门槛（bp，默认 2000）。
    pub reverse_revision_threshold_bp: i32,
    /// 触发复核的综合判断变化（bp，默认 1000）。
    pub review_signal_delta_bp: i32,
    /// 触发复核的价格相对上次判断变化（bp，默认 200）。
    pub review_price_change_bp: i32,
}

impl Default for PlanPolicy {
    fn default() -> Self {
        Self {
            reverse_revision_threshold_bp: 2000,
            review_signal_delta_bp: 1000,
            review_price_change_bp: 200,
        }
    }
}

/// 计划事件：状态机的唯一输入。接受与成交是不同的变体——委托被接受绝不推进成交。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub enum PlanEvent {
    /// 平静观察：信息/风险/约束无变化。
    ObservedNoChange {
        trading_day: u64,
    },
    /// 子单被权威路由接受（仅建立执行引用）。
    ChildOrderAccepted {
        order_id: OrderId,
        trading_day: u64,
    },
    /// 子单真实成交（唯一推进 filled 的事件）。
    ChildOrderFilled {
        order_id: OrderId,
        qty: u32,
        trading_day: u64,
    },
    /// 超目标真实成交：如实入账并以 FilledBeyondTarget 终止。
    ChildOrderExcessFilled {
        order_id: OrderId,
        qty: u32,
        trading_day: u64,
    },
    Revised {
        revision: PlanRevision,
    },
    Paused {
        reason: PauseReason,
        trading_day: u64,
    },
    Resumed {
        reason: ResumeReason,
        trading_day: u64,
    },
    Terminated {
        reason: TerminationReason,
        trading_day: u64,
    },
    /// 显式到期终止（仅在真正越过有效期后有效）。
    Expired {
        trading_day: u64,
    },
    /// 日终：仅结束子单生命周期。
    TradingDayEnded {
        trading_day: u64,
    },
}

/// 计划集合：按账户+股票至多一个非终止计划；PlanId 单调分配、永不复用。
///
/// 存档只序列化 `policy / next_plan_seq / plans`；(账户,股票) 索引在恢复时
/// 重建并校验一致性，不一致的存档被显式拒绝。
#[derive(Clone, Eq, PartialEq, Debug, ts_rs::TS)]
pub struct PlanBook {
    policy: PlanPolicy,
    next_plan_seq: u64,
    plans: BTreeMap<PlanId, TradingPlan>,
    #[ts(skip)]
    by_account_stock: BTreeMap<(AccountId, StockCode), PlanId>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanBookSave {
    policy: PlanPolicy,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    next_plan_seq: u64,
    plans: BTreeMap<PlanId, TradingPlan>,
}

impl serde::Serialize for PlanBook {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        PlanBookSave {
            policy: self.policy,
            next_plan_seq: self.next_plan_seq,
            plans: self.plans.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for PlanBook {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = PlanBookSave::deserialize(deserializer)?;
        PlanBook::from_parts(raw.policy, raw.next_plan_seq, raw.plans)
            .map_err(serde::de::Error::custom)
    }
}

impl PlanBook {
    /// 以显式政策构造（政策字段立即校验）。
    pub fn new(policy: PlanPolicy) -> Result<Self, PlanError> {
        validate_policy(&policy)?;
        Ok(Self {
            policy,
            next_plan_seq: 0,
            plans: BTreeMap::new(),
            by_account_stock: BTreeMap::new(),
        })
    }

    /// 从存档部件恢复：重建 (账户,股票) 索引并拒绝不一致状态。
    pub fn from_parts(
        policy: PlanPolicy,
        next_plan_seq: u64,
        plans: BTreeMap<PlanId, TradingPlan>,
    ) -> Result<Self, PlanError> {
        validate_policy(&policy)?;
        let mut by_account_stock = BTreeMap::new();
        for (plan_id, plan) in &plans {
            if plan.plan_id != *plan_id {
                return Err(PlanError::SaveInconsistent {
                    detail: format!(
                        "plan entry key {plan_id:?} does not match its id {:?}",
                        plan.plan_id
                    ),
                });
            }
            if plan_id.0 >= next_plan_seq {
                return Err(PlanError::SaveInconsistent {
                    detail: format!(
                        "plan id {plan_id:?} is not below next plan seq {next_plan_seq}"
                    ),
                });
            }
            if let Some(previous) =
                by_account_stock.insert((plan.account, plan.code.clone()), *plan_id)
            {
                return Err(PlanError::SaveInconsistent {
                    detail: format!(
                        "duplicate plan for account {:?} stock {:?}: {previous:?} and {plan_id:?}",
                        plan.account, plan.code
                    ),
                });
            }
        }
        Ok(Self {
            policy,
            next_plan_seq,
            plans,
            by_account_stock,
        })
    }

    pub fn policy(&self) -> &PlanPolicy {
        &self.policy
    }

    /// 开一个新计划：同账户+股票存在非终止计划时拒绝；否则分配新 PlanId。
    pub fn create(&mut self, open: PlanOpen) -> Result<PlanId, PlanError> {
        validate_open(&open)?;
        if let Some(existing_id) = self
            .by_account_stock
            .get(&(open.account, open.code.clone()))
        {
            if !self.plans[existing_id].is_terminal() {
                return Err(PlanError::DuplicateActivePlan {
                    account: open.account,
                    code: open.code,
                });
            }
        }
        let plan_id = PlanId(self.next_plan_seq);
        self.next_plan_seq = self
            .next_plan_seq
            .checked_add(1)
            .ok_or(PlanError::PlanSequenceExhausted)?;
        let plan = TradingPlan::from_open(plan_id, open, &self.policy)?;
        self.by_account_stock
            .insert((plan.account, plan.code.clone()), plan_id);
        self.plans.insert(plan_id, plan);
        Ok(plan_id)
    }

    /// 按 id 查询；未知 id 是类型化错误而非静默空值。
    pub fn plan(&self, plan_id: PlanId) -> Result<&TradingPlan, PlanError> {
        self.plans
            .get(&plan_id)
            .ok_or(PlanError::UnknownPlan { plan_id })
    }

    /// 全部计划 id（PlanId 升序；日终扫描/诊断用——不暴露内部 map）。
    pub fn plan_ids(&self) -> impl Iterator<Item = PlanId> + '_ {
        self.plans.keys().copied()
    }

    /// 账户+股票当前的非终止计划（无则 None）。
    pub fn active_plan(&self, account: AccountId, code: &StockCode) -> Option<&TradingPlan> {
        let plan_id = self.by_account_stock.get(&(account, code.clone()))?;
        let plan = &self.plans[plan_id];
        if plan.is_terminal() {
            None
        } else {
            Some(plan)
        }
    }

    /// 对一个计划应用一个事件：纯 (状态, 事件) 转移，返回事件后的状态。
    pub fn apply(&mut self, plan_id: PlanId, event: PlanEvent) -> Result<PlanStatus, PlanError> {
        let policy = self.policy;
        let plan = self
            .plans
            .get_mut(&plan_id)
            .ok_or(PlanError::UnknownPlan { plan_id })?;
        match event {
            PlanEvent::ObservedNoChange { trading_day } => plan.observe_no_change(trading_day)?,
            PlanEvent::ChildOrderAccepted {
                order_id,
                trading_day,
            } => plan.record_child_order_accepted(order_id, trading_day)?,
            PlanEvent::ChildOrderFilled {
                order_id,
                qty,
                trading_day,
            } => plan.record_real_fill(order_id, qty, trading_day)?,
            PlanEvent::ChildOrderExcessFilled {
                order_id,
                qty,
                trading_day,
            } => plan.record_excess_fill(order_id, qty, trading_day)?,
            PlanEvent::Revised { revision } => revision::apply_revision(plan, &revision, &policy)?,
            PlanEvent::Paused {
                reason,
                trading_day,
            } => revision::pause(plan, reason, trading_day)?,
            PlanEvent::Resumed {
                reason,
                trading_day,
            } => revision::resume(plan, reason, trading_day)?,
            PlanEvent::Terminated {
                reason,
                trading_day,
            } => plan.terminate(reason, trading_day)?,
            PlanEvent::Expired { trading_day } => revision::expire(plan, trading_day)?,
            PlanEvent::TradingDayEnded { trading_day } => {
                revision::end_of_trading_day(plan, trading_day)?
            }
        }
        Ok(plan.status)
    }
}

impl Default for PlanBook {
    fn default() -> Self {
        Self::new(PlanPolicy::default()).expect("default plan policy is valid by construction")
    }
}
