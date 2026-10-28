//! 同一身份簿内的个人交易计划与发行人回购授权。
//!
//! 纯状态机：转移是 (状态, 事件) 的确定性纯函数，无随机数、无 I/O。
//! 计划不冻结资产，不建立第二套订单/冻结系统；母单执行子状态
//! （`PlanExecutionLeg`）唯一保存在 `PlanEntry` 对应来源，个人执行由会话计划路由接线。
//! 预算分配与紧迫度报价决策分别在 allocation 与 urgency 中实现。

mod allocation;
mod candidates;
mod entry;
pub mod quote_policy;
mod records;
mod repurchase_origin;
mod revision;
mod state;
pub mod urgency;
mod validation;

pub(crate) use allocation::allocate_child_quote_budgets;
pub use allocation::{
    allocate_soft_budgets, read_allocation_experience, AllocationClass, AllocationConstraint,
    AllocationError, AllocationExperience, AllocationFunds, AllocationGrant, AllocationRequest,
    AllocationResult, ExperienceHolding, ExperienceReadRequest,
};
pub use candidates::{
    blend_candidate, eligible_candidates, experience_cost_signal, fundamental_range_signal,
    fundamental_signal, normalized_score, price_volume_signal, target_position_weight_bp,
    target_share_quantity, technical_signal, trend_signal, CandidateAssessment, CandidateError,
    CandidateSignals, CandidateTargetProposal, ExcludedSignal, QuantityRounding, SignalComponent,
    SignalContribution, SignalScore, SignalUnavailableReason, TargetShareQuantity,
};
pub use quote_policy::{
    decide_quote, ActiveQuote, BookTop, QuoteAction, QuoteDecision, QuoteDecisionInputs,
    QuoteError, QuoteReason,
};
pub use entry::{PlanEntry, PlanOrigin};
pub use revision::{PlanRevision, RevisionReason, RevisionRecord};
pub use repurchase_origin::{RepurchaseExecutionPlan, RepurchaseExecutionStatus};
pub use state::{
    OpinionSource, PauseReason, PlanExecutionLeg, PlanId, PlanOpen, PlanOpinion, PlanStatus,
    PlanTarget, ResumeReason, ReviewConditions, ReviewResources, TerminationReason, TradingPlan,
    Urgency,
};
pub use urgency::{
    assess_recovery, assess_urgency, PatienceStyle, PauseAssessment, RecoveryAssessment,
    RecoveryInputs, UrgencyAssessment, UrgencyError, UrgencyInputs, UrgencyPolicy, UrgencyReason,
    URGENCY_POLICY_VERSION,
};
pub use validation::{reverse_crosses_threshold, PlanError};

use std::collections::BTreeMap;
use validation::{validate_open, validate_policy};

use crate::account::StockId;
use crate::orderbook::{AccountId, OrderId, Side};

/// 行政候选中的公司计划开启事实；身份仍由唯一 PlanBook 分配。
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct RepurchaseExecutionOpen {
    pub owner: AccountId,
    pub issuer: crate::company::CompanyId,
    pub stock: StockId,
}

/// 个人期限按风格为 5/20/60 交易日；风格→期限映射由分析档案派生，这里固定参数。
pub const HORIZON_TRADING_DAYS_SHORT: u32 = 5;
pub const HORIZON_TRADING_DAYS_MEDIUM: u32 = 20;
pub const HORIZON_TRADING_DAYS_LONG: u32 = 60;

/// 计划政策（固定游戏参数，随 save 固化）。
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
    /// The order book canceled the unfilled remainder of this child.
    ChildOrderCanceled {
        order_id: OrderId,
        trading_day: u64,
    },
    /// 子单真实成交（唯一推进 filled 的事件）。
    ChildOrderFilled {
        order_id: OrderId,
        qty: u32,
        child_complete: bool,
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

/// 每账户的下一分配值；None 显式表示完整 u64 序列已经耗尽。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct OwnerSequence {
    pub owner: AccountId,
    #[serde(with = "owner_sequence_wire")]
    #[ts(type = "string | null")]
    pub next_sequence: Option<u64>,
}

mod owner_sequence_wire {
    use serde::{Deserialize, Deserializer, Serializer};
    #[derive(Deserialize)]
    struct Sequence(#[serde(with = "crate::orderbook::canonical_u64_decimal")] u64);
    pub fn serialize<S: Serializer>(value: &Option<u64>, serializer: S) -> Result<S::Ok, S::Error> {
        match value {
            Some(value) => serializer.serialize_some(&value.to_string()),
            None => serializer.serialize_none(),
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(decoder: D) -> Result<Option<u64>, D::Error> {
        Ok(Option::<Sequence>::deserialize(decoder)?.map(|value| value.0))
    }
}

/// 计划集合：每账户+股票至多一个非终止计划，账户内身份永不复用。
#[derive(Clone, Eq, PartialEq, Debug, ts_rs::TS)]
pub struct PlanBook {
    policy: PlanPolicy,
    #[ts(as = "Vec<OwnerSequence>")]
    owner_sequences: BTreeMap<AccountId, Option<u64>>,
    #[ts(as = "Vec<PlanEntry>")]
    plans: records::PlanRecords,
    #[ts(skip)]
    by_account_stock: BTreeMap<AccountId, BTreeMap<StockId, PlanId>>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanBookSave {
    policy: PlanPolicy,
    owner_sequences: Vec<OwnerSequence>,
    plans: Vec<PlanEntry>,
}

impl serde::Serialize for PlanBook {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut save = serializer.serialize_struct("PlanBook", 3)?;
        save.serialize_field("policy", &self.policy)?;
        save.serialize_field(
            "owner_sequences",
            &self.owner_sequences().collect::<Vec<_>>(),
        )?;
        save.serialize_field("plans", &self.plans)?;
        save.end()
    }
}

impl<'de> serde::Deserialize<'de> for PlanBook {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        let raw = PlanBookSave::deserialize(decoder)?;
        Self::from_parts(raw.policy, raw.owner_sequences, raw.plans)
            .map_err(serde::de::Error::custom)
    }
}

impl PlanBook {
    /// 准备公司授权条目；只有与资金共同提交的行政候选才能发布 Funded。
    pub(crate) fn create_issuer_repurchase(
        &mut self,
        open: RepurchaseExecutionOpen,
    ) -> Result<PlanId, PlanError> {
        let id = self.prepare_identity(open.owner, open.stock)?;
        let plan = RepurchaseExecutionPlan::for_approval_candidate(id, open.issuer, open.stock);
        self.install_new_entry(PlanEntry::IssuerRepurchase(plan));
        Ok(id)
    }

    fn prepare_identity(&self, owner: AccountId, code: StockId) -> Result<PlanId, PlanError> {
        if self
            .by_account_stock
            .get(&owner)
            .is_some_and(|stocks| stocks.contains_key(&code))
        {
            return Err(PlanError::DuplicateActivePlan {
                account: owner,
                code,
            });
        }
        let sequence = match self.owner_sequences.get(&owner) {
            Some(Some(next)) => *next,
            Some(None) => return Err(PlanError::PlanSequenceExhausted),
            None => 0,
        };
        Ok(PlanId { owner, sequence })
    }

    fn install_new_entry(&mut self, entry: PlanEntry) {
        let id = entry.plan_id();
        self.owner_sequences
            .insert(id.owner, id.sequence.checked_add(1));
        self.by_account_stock
            .entry(id.owner)
            .or_default()
            .insert(*entry.code(), id);
        self.plans.insert(id, entry);
    }

    fn investor_mut(&mut self, id: PlanId) -> Result<&mut TradingPlan, PlanError> {
        let entry = self
            .plans
            .get_mut(&id)
            .ok_or(PlanError::UnknownPlan { plan_id: id })?;
        match entry {
            PlanEntry::Investor(plan) => Ok(plan),
            PlanEntry::IssuerRepurchase(_) => Err(PlanError::WrongOrigin {
                plan_id: id,
                expected: PlanOrigin::Investor,
                actual: PlanOrigin::IssuerRepurchase,
            }),
        }
    }

    fn require_connected_child_origin(&self, id: PlanId) -> Result<(), PlanError> {
        match self.entry(id)? {
            PlanEntry::Investor(_) => Ok(()),
            PlanEntry::IssuerRepurchase(_) => {
                Err(PlanError::IssuerChildNotConnected { plan_id: id })
            }
        }
    }

    /// 全来源读取；个人入口不能借 Unknown 掩盖公司条目。
    pub fn entry(&self, id: PlanId) -> Result<&PlanEntry, PlanError> {
        self.plans
            .get(&id)
            .ok_or(PlanError::UnknownPlan { plan_id: id })
    }

    pub fn active_entry(&self, account: AccountId, code: &StockId) -> Option<&PlanEntry> {
        let id = self.by_account_stock.get(&account)?.get(code)?;
        Some(self.entry(*id).expect("活动索引必须对应真实条目"))
    }

    /// 只用于个人判断的读取面，不将公司授权当个人观点。
    pub fn investor_plans(&self) -> impl Iterator<Item = &TradingPlan> {
        self.plans.iter().filter_map(|(_, entry)| match entry {
            PlanEntry::Investor(plan) => Some(plan),
            PlanEntry::IssuerRepurchase(_) => None,
        })
    }

    pub(crate) fn active_investor_plan_ids_for_account(&self, owner: AccountId) -> Vec<PlanId> {
        self.active_plan_ids_for_account(owner)
            .into_iter()
            .filter(|id| {
                matches!(
                    self.entry(*id).expect("活动身份必须存在"),
                    PlanEntry::Investor(_)
                )
            })
            .collect()
    }

    /// 两类日终共用同一簿；未接真实来源的公司执行腿不能静默跨日或被清空。
    pub(crate) fn end_trading_day(&mut self, day: u64) -> Result<(), PlanError> {
        self.end_trading_day_for_stocks(|_| Ok(Some(day)))
    }

    /// 前置只读门共用真实事件守卫；公司执行腿先于任何休市 mask 被拒绝。
    pub(crate) fn validate_end_trading_day_for_stocks(
        &self,
        mut stock_day: impl FnMut(crate::StockId) -> Result<Option<u64>, PlanError>,
    ) -> Result<(), PlanError> {
        for id in self.active_plan_ids() {
            if let PlanEntry::IssuerRepurchase(plan) = self.entry(id)? {
                if plan.execution().is_some() {
                    return Err(PlanError::IssuerChildNotConnected { plan_id: id });
                }
                stock_day(*plan.stock_ref())?;
            }
        }
        for id in self.active_plan_ids() {
            if let PlanEntry::Investor(plan) = self.entry(id)? {
                if !plan.is_terminal() {
                    if let Some(day) = stock_day(*plan.code())? {
                        plan.ensure_event_allowed("day-end", day, true)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// 同一原子迁移算法；None 表示本股休市，缺失 row 必须由调用方显式报错。
    pub(crate) fn end_trading_day_for_stocks(
        &mut self,
        mut stock_day: impl FnMut(crate::StockId) -> Result<Option<u64>, PlanError>,
    ) -> Result<(), PlanError> {
        self.validate_end_trading_day_for_stocks(&mut stock_day)?;
        let mut working = self.clone();
        for id in self.active_plan_ids() {
            match self.entry(id)? {
                PlanEntry::Investor(plan) if !plan.is_terminal() => {
                    if let Some(trading_day) = stock_day(*plan.code())? {
                        working.apply(id, PlanEvent::TradingDayEnded { trading_day })?;
                    }
                }
                PlanEntry::Investor(_) => {}
                PlanEntry::IssuerRepurchase(plan) => {
                    if plan.execution().is_some() {
                        return Err(PlanError::IssuerChildNotConnected { plan_id: id });
                    }
                }
            }
        }
        *self = working;
        Ok(())
    }

    pub(crate) fn record_resource_review(
        &mut self,
        plan_id: PlanId,
        resources: ReviewResources,
    ) -> Result<(), PlanError> {
        if !resources.is_valid() {
            return Err(PlanError::SaveInconsistent {
                detail: "invalid personal review resource facts".to_owned(),
            });
        }
        self.investor_mut(plan_id)?
            .record_resource_review(resources);
        Ok(())
    }

    /// Records the facts actually observed by one plan. This never reserves assets.
    pub(crate) fn record_review(
        &mut self,
        plan_id: PlanId,
        trading_day: u64,
        price: crate::Money,
        acquired_count: u32,
    ) -> Result<(), PlanError> {
        let plan = self.investor_mut(plan_id)?;
        plan.record_review(trading_day, price, acquired_count)
    }

    /// Only live plans participate in decision roots. Historical plan records stay
    /// queryable in `plans` without making every observation scan them.
    pub(crate) fn active_investor_codes(&self, account: AccountId) -> impl Iterator<Item = &StockId> {
        self.by_account_stock
            .get(&account)
            .into_iter()
            .flat_map(|stocks| stocks.iter())
            .filter_map(move |(code, id)| match self.entry(*id).expect("活动索引必须对应真实条目") {
                PlanEntry::Investor(_) => Some(code),
                PlanEntry::IssuerRepurchase(_) => None,
            })
    }

    pub(crate) fn active_investor_accounts(&self) -> impl Iterator<Item = AccountId> + '_ {
        self.by_account_stock.keys().copied().filter(move |owner| self.active_investor_codes(*owner).next().is_some())
    }

    /// 以显式政策构造（政策字段立即校验）。
    pub fn new(policy: PlanPolicy) -> Result<Self, PlanError> {
        validate_policy(&policy)?;
        Ok(Self {
            policy,
            owner_sequences: BTreeMap::new(),
            plans: records::PlanRecords::default(),
            by_account_stock: BTreeMap::new(),
        })
    }

    /// 从存档部件恢复：重建 (账户,股票) 索引并拒绝不一致状态。
    ///
    /// 同一 (账户,股票) 允许存在多条计划（`create` 在旧计划终止后分配新
    /// PlanId；终止计划保留在簿内），但**至多一条非终止**；热索引只存
    /// 非终止计划，旧历史不参与每次日终遍历。
    pub fn from_parts(
        policy: PlanPolicy,
        owner_sequences: Vec<OwnerSequence>,
        plans: Vec<PlanEntry>,
    ) -> Result<Self, PlanError> {
        validate_policy(&policy)?;
        let mut owners = std::collections::BTreeSet::new();
        for entry in &owner_sequences {
            if !owners.insert(entry.owner) {
                return Err(PlanError::PlanWriterOwnershipViolation { owner: entry.owner });
            }
        }
        let sequences: BTreeMap<_, _> = owner_sequences
            .into_iter()
            .map(|entry| (entry.owner, entry.next_sequence))
            .collect();
        let mut identities = std::collections::BTreeSet::new();
        for plan in &plans {
            if !identities.insert(plan.plan_id()) {
                return Err(PlanError::DuplicatePlanIdentity {
                    plan_id: plan.plan_id(),
                });
            }
        }
        let mut by_account_stock = BTreeMap::new();
        for plan in &plans {
            let plan_id = plan.plan_id();
            match plan {
                PlanEntry::Investor(plan) => {
                    plan.validate_horizon()?;
                    if plan_id.owner != plan.account() {
                        return Err(PlanError::PlanOwnerMismatch {
                            plan_id,
                            account: plan.account(),
                        });
                    }
                    if plan
                        .review()
                        .last_review_price
                        .is_some_and(|price| price.cents() <= 0)
                        || plan
                            .review()
                            .last_review_resources
                            .is_some_and(|resources| !resources.is_valid())
                        || plan.review().last_review_trading_day < plan.created_trading_day()
                        || plan.review().last_review_trading_day > plan.last_event_trading_day()
                    {
                        return Err(PlanError::SaveInconsistent {
                            detail: format!("plan {plan_id:?} has an invalid review baseline"),
                        });
                    }
                }
                PlanEntry::IssuerRepurchase(plan) => {
                    plan.validate()
                        .map_err(|error| PlanError::SaveInconsistent {
                            detail: error.to_string(),
                        })?
                }
            }
            match sequences.get(&plan_id.owner) {
                Some(None) => {}
                Some(Some(next)) if *next > plan_id.sequence => {}
                next => {
                    return Err(PlanError::InvalidPlanSequence {
                        owner: plan_id.owner,
                        next_sequence: next.copied().flatten(),
                    })
                }
            }
            if !plan.is_terminal()
                && by_account_stock
                    .entry(plan.account())
                    .or_insert_with(BTreeMap::new)
                    .insert(*plan.code(), plan_id)
                    .is_some()
            {
                return Err(PlanError::SaveInconsistent {
                    detail: format!(
                        "two non-terminal plans for account {:?} stock {:?}",
                        plan.account(),
                        plan.code()
                    ),
                });
            }
        }
        Ok(Self {
            policy,
            owner_sequences: sequences,
            plans: plans
                .into_iter()
                .map(|plan| (plan.plan_id(), plan))
                .collect::<BTreeMap<_, _>>()
                .into(),
            by_account_stock,
        })
    }

    /// 按数值账户顺序返回全部游标，包含没有当前计划的历史游标。
    pub fn owner_sequences(&self) -> impl Iterator<Item = OwnerSequence> + '_ {
        self.owner_sequences
            .iter()
            .map(|(&owner, &next_sequence)| OwnerSequence {
                owner,
                next_sequence,
            })
    }

    /// 权威移交的完整部件；不重算游标或替换 policy。
    pub fn into_parts(self) -> (PlanPolicy, Vec<OwnerSequence>, Vec<PlanEntry>) {
        let sequences = self.owner_sequences().collect();
        let plans = self.plans.iter().map(|(_, plan)| plan.clone()).collect();
        (self.policy, sequences, plans)
    }

    pub fn policy(&self) -> &PlanPolicy {
        &self.policy
    }

    /// 开一个新计划：同账户+股票存在非终止计划时拒绝；否则分配新 PlanId。
    pub fn create(&mut self, open: PlanOpen) -> Result<PlanId, PlanError> {
        validate_open(&open)?;
        let id = self.prepare_identity(open.account, open.code)?;
        let plan = TradingPlan::from_open(id, open, &self.policy)?;
        self.install_new_entry(PlanEntry::Investor(plan));
        Ok(id)
    }

    /// 按 id 查询；未知 id 是类型化错误而非静默空值。
    pub fn plan(&self, plan_id: PlanId) -> Result<&TradingPlan, PlanError> {
        match self.entry(plan_id)? {
            PlanEntry::Investor(plan) => Ok(plan),
            PlanEntry::IssuerRepurchase(_) => Err(PlanError::WrongOrigin {
                plan_id,
                expected: PlanOrigin::Investor,
                actual: PlanOrigin::IssuerRepurchase,
            }),
        }
    }

    /// 全部历史计划 id（PlanId 升序；存档/诊断用——不暴露内部 map）。
    pub fn plan_ids(&self) -> impl Iterator<Item = PlanId> + '_ {
        self.plans.keys().copied()
    }

    /// 非终止计划 id，按 PlanId 升序保持既有计划遍历顺序。
    /// 它不表示正式委托的受理先后。
    pub(crate) fn active_plan_ids(&self) -> Vec<PlanId> {
        let mut ids = self
            .by_account_stock
            .values()
            .flat_map(|by_stock| by_stock.values().copied())
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids
    }

    /// 仅取该账户的非终止计划，保留原来的 PlanId 遍历顺序。
    pub(crate) fn active_plan_ids_for_account(&self, account: AccountId) -> Vec<PlanId> {
        let mut ids = self
            .by_account_stock
            .get(&account)
            .into_iter()
            .flat_map(|by_stock| by_stock.values().copied())
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids
    }

    /// 账户+股票当前的非终止计划（无则 None）。
    pub fn active_investor_plan(&self, account: AccountId, code: &StockId) -> Option<&TradingPlan> {
        let plan_id = self.by_account_stock.get(&account)?.get(code)?;
        match self.entry(*plan_id).expect("活动索引必须对应真实条目") {
            PlanEntry::Investor(plan) => Some(plan),
            PlanEntry::IssuerRepurchase(_) => None,
        }
    }

    // ── 母单执行子状态的唯一 owner 入口（ADR-0015 2026-09-10 修订）──
    // 创建/推进/撤销/成交/限价修订都从这里更新 `TradingPlan` 的执行腿；
    // 会话侧不再持有任何第二份可变计划状态。

    /// 安装/替换一个执行腿的调度参数（新子单报价前调用）。仅 `Active` 计划可安装。
    pub fn install_execution(
        &mut self,
        plan_id: PlanId,
        child_qty: u32,
        limit_price: crate::Money,
        expires_market_minute: u64,
    ) -> Result<(), PlanError> {
        self.require_connected_child_origin(plan_id)?;
        let plan = self.investor_mut(plan_id)?;
        plan.install_execution_leg(child_qty, limit_price, expires_market_minute)
    }

    /// 权威路由确认接受子单：直接登记执行腿在途子单身份与剩余数量，
    /// 并登记接受事实（幂等）。方向不符或非本计划子单返回 false。
    pub fn record_child_submitted(
        &mut self,
        account: AccountId,
        code: &StockId,
        side: Side,
        order_id: OrderId,
        qty: u32,
        trading_day: u64,
    ) -> Result<bool, PlanError> {
        let Some(plan_id) = self
            .by_account_stock
            .get(&account)
            .and_then(|by_stock| by_stock.get(code).copied())
        else {
            return Ok(false);
        };
        self.require_connected_child_origin(plan_id)?;
        let plan = self.investor_mut(plan_id)?;
        if plan.direction() != side || plan.execution().is_none() {
            return Ok(false);
        }
        plan.adopt_child_facts(order_id, qty, trading_day)?;
        Ok(true)
    }

    /// 子单被确认撤销/到期/拒单释放：清除执行腿在途子单引用（不回退已成交）。
    /// 非本计划子单返回 false；在途身份不符属于类型化错误。
    pub fn record_child_released(
        &mut self,
        account: AccountId,
        code: &StockId,
        order_id: OrderId,
        trading_day: u64,
        allow_beyond_horizon: bool,
    ) -> Result<bool, PlanError> {
        let Some(plan_id) = self
            .by_account_stock
            .get(&account)
            .and_then(|by_stock| by_stock.get(code).copied())
        else {
            return Ok(false);
        };
        self.require_connected_child_origin(plan_id)?;
        let plan = self.investor_mut(plan_id)?;
        if plan.active_child_order_id() != Some(order_id) {
            return Ok(false);
        }
        plan.record_child_order_canceled(order_id, trading_day, allow_beyond_horizon)?;
        Ok(true)
    }

    /// 按已发生的成交事实立即更新唯一 owner；完成后同步移出活跃索引。
    pub fn record_child_fill(
        &mut self,
        account: AccountId,
        code: &StockId,
        side: Side,
        order_id: OrderId,
        qty: u32,
        trading_day: u64,
    ) -> Result<bool, PlanError> {
        if let Some(entry) = self.active_entry(account, code) {
            self.require_connected_child_origin(entry.plan_id())?;
        }
        let Some(plan) = self.active_investor_plan(account, code) else {
            return Ok(false);
        };
        if plan.direction() != side || plan.active_child_order_id() != Some(order_id) {
            return Ok(false);
        }
        let id = plan.plan_id();
        let remaining = plan
            .active_child_remaining_qty()
            .ok_or(PlanError::MissingExecutionLeg { plan_id: id })?;
        self.apply(
            id,
            PlanEvent::ChildOrderFilled {
                order_id,
                qty,
                child_complete: qty == remaining,
                trading_day,
            },
        )?;
        Ok(true)
    }

    /// 恢复接缝：用真实订单簿剩余量重建在途子单剩余数量。
    pub(crate) fn restore_child_remaining_qty(
        &mut self,
        plan_id: PlanId,
        remaining: Option<u32>,
    ) -> Result<(), PlanError> {
        self.require_connected_child_origin(plan_id)?;
        let plan = self.investor_mut(plan_id)?;
        let Some(leg) = plan.execution.as_mut() else {
            return Err(PlanError::MissingExecutionLeg { plan_id });
        };
        match (plan.active_child_order_id, remaining) {
            (None, None) => Ok(()),
            (Some(_), Some(qty)) => {
                leg.active_child_remaining_qty = Some(qty);
                Ok(())
            }
            _ => Err(PlanError::InvalidChildRestore { plan_id }),
        }
    }

    /// 对一个计划应用一个事件：纯 (状态, 事件) 转移，返回事件后的状态。
    pub fn apply(&mut self, plan_id: PlanId, event: PlanEvent) -> Result<PlanStatus, PlanError> {
        let policy = self.policy;
        let plan = self.investor_mut(plan_id)?;
        let key = (plan.account(), *plan.code());
        let status = Self::apply_to_plan(plan, event, &policy)?;
        if plan.is_terminal() {
            self.remove_active_index(key.0, &key.1);
        }
        Ok(status)
    }

    /// Stage only plans touched by this event batch. A fact naming an absent or
    /// previously terminal plan is invalid. A fact after a completion earlier in
    /// this same batch is consumed without another transition. A failed transition
    /// publishes none of the staged changes.
    pub(crate) fn apply_active_events_atomically(
        &mut self,
        events: &[(PlanId, PlanEvent)],
    ) -> Result<Vec<Option<PlanStatus>>, PlanError> {
        let mut staged = BTreeMap::<PlanId, TradingPlan>::new();
        let mut outcomes = Vec::with_capacity(events.len());
        for (plan_id, event) in events {
            if !staged.contains_key(plan_id) {
                let plan = self.plan(*plan_id)?;
                if plan.is_terminal() {
                    return Err(PlanError::InvalidTransition {
                        plan_id: *plan_id,
                        from: plan.status(),
                        event: "pending routed fact",
                    });
                }
                staged.insert(*plan_id, plan.clone());
            }
            let plan = staged
                .get_mut(plan_id)
                .expect("the active plan was staged above");
            if plan.is_terminal() {
                outcomes.push(None);
                continue;
            }
            outcomes.push(Some(Self::apply_to_plan(
                plan,
                event.clone(),
                &self.policy,
            )?));
        }
        for (plan_id, plan) in staged {
            if plan.is_terminal() {
                self.remove_active_index(plan.account(), plan.code());
            }
            self.plans.insert(plan_id, PlanEntry::Investor(plan));
        }
        Ok(outcomes)
    }

    fn remove_active_index(&mut self, account: AccountId, code: &StockId) {
        if let Some(by_stock) = self.by_account_stock.get_mut(&account) {
            by_stock.remove(code);
            if by_stock.is_empty() {
                self.by_account_stock.remove(&account);
            }
        }
    }

    fn apply_to_plan(
        plan: &mut TradingPlan,
        event: PlanEvent,
        policy: &PlanPolicy,
    ) -> Result<PlanStatus, PlanError> {
        match event {
            PlanEvent::ObservedNoChange { trading_day } => plan.observe_no_change(trading_day)?,
            PlanEvent::ChildOrderAccepted {
                order_id,
                trading_day,
            } => plan.record_child_order_accepted(order_id, trading_day)?,
            PlanEvent::ChildOrderCanceled {
                order_id,
                trading_day,
            } => plan.record_child_order_canceled(order_id, trading_day, false)?,
            PlanEvent::ChildOrderFilled {
                order_id,
                qty,
                child_complete,
                trading_day,
            } => plan.record_real_fill(order_id, qty, child_complete, trading_day)?,
            PlanEvent::ChildOrderExcessFilled {
                order_id,
                qty,
                trading_day,
            } => plan.record_excess_fill(order_id, qty, trading_day)?,
            PlanEvent::Revised { revision } => plan.apply_revision(&revision, policy)?,
            PlanEvent::Paused {
                reason,
                trading_day,
            } => plan.pause(reason, trading_day)?,
            PlanEvent::Resumed {
                reason,
                trading_day,
            } => plan.resume(reason, trading_day)?,
            PlanEvent::Terminated {
                reason,
                trading_day,
            } => plan.terminate(reason, trading_day)?,
            PlanEvent::Expired { trading_day } => plan.expire(trading_day)?,
            PlanEvent::TradingDayEnded { trading_day } => plan.end_of_trading_day(trading_day)?,
        }
        Ok(plan.status())
    }
}

impl Default for PlanBook {
    fn default() -> Self {
        Self::new(PlanPolicy::default()).expect("default plan policy is valid by construction")
    }
}

#[cfg(test)]
mod origin_tests;

#[cfg(test)]
mod atomic_event_tests {
    use super::*;
    use crate::Side;

    fn open(rank: u32) -> PlanOpen {
        PlanOpen {
            account: AccountId(1),
            code: StockId(rank),
            direction: Side::Buy,
            target: PlanTarget::ShareCount(100),
            opinion: PlanOpinion {
                signal_score_bp: 3_000,
                source: OpinionSource::Blended,
            },
            confidence_bp: 8_000,
            urgency: Urgency::Normal,
            horizon_trading_days: 5,
            created_trading_day: 0,
        }
    }

    #[test]
    fn shadow_clone_shares_terminal_history_and_only_copies_touched_plan() {
        let mut plans = PlanBook::default();
        for _ in 0..300 {
            let plan = plans.create(open(0)).unwrap();
            plans
                .apply(
                    plan,
                    PlanEvent::Terminated {
                        reason: TerminationReason::Cancelled,
                        trading_day: 0,
                    },
                )
                .unwrap();
        }
        let live = plans.create(open(0)).unwrap();
        let mut shadow = plans.clone();
        assert!(std::ptr::eq(
            plans
                .plan(PlanId {
                    owner: AccountId(1),
                    sequence: 0
                })
                .unwrap(),
            shadow
                .plan(PlanId {
                    owner: AccountId(1),
                    sequence: 0
                })
                .unwrap()
        ));
        assert!(std::ptr::eq(
            plans.plan(live).unwrap(),
            shadow.plan(live).unwrap()
        ));
        shadow
            .record_review(live, 0, crate::Money::from_cents(1000), 1)
            .unwrap();
        assert!(std::ptr::eq(
            plans
                .plan(PlanId {
                    owner: AccountId(1),
                    sequence: 0
                })
                .unwrap(),
            shadow
                .plan(PlanId {
                    owner: AccountId(1),
                    sequence: 0
                })
                .unwrap()
        ));
        assert!(!std::ptr::eq(
            plans.plan(live).unwrap(),
            shadow.plan(live).unwrap()
        ));
        assert_ne!(
            plans.plan(live).unwrap().review(),
            shadow.plan(live).unwrap().review()
        );
        assert_eq!(
            plans.plan_ids().collect::<Vec<_>>(),
            shadow.plan_ids().collect::<Vec<_>>()
        );
        let restored: PlanBook =
            serde_json::from_value(serde_json::to_value(&shadow).unwrap()).unwrap();
        assert_eq!(restored, shadow);
        let legacy = PlanBookSave {
            policy: shadow.policy,
            owner_sequences: shadow.owner_sequences().collect(),
            plans: shadow.plans.iter().map(|(_, plan)| plan.clone()).collect(),
        };
        assert_eq!(
            serde_json::to_vec(&shadow).unwrap(),
            serde_json::to_vec(&legacy).unwrap()
        );
    }

    #[test]
    fn invalid_later_plan_event_does_not_commit_an_earlier_plan_transition() {
        let mut plans = PlanBook::default();
        let first = plans.create(open(0)).unwrap();
        let second = plans.create(open(1)).unwrap();
        let before = plans.clone();
        let result = plans.apply_active_events_atomically(&[
            (
                first,
                PlanEvent::ChildOrderAccepted {
                    order_id: crate::OrderId(1),
                    trading_day: 0,
                },
            ),
            (
                second,
                PlanEvent::ChildOrderFilled {
                    order_id: crate::OrderId(2),
                    qty: 100,
                    child_complete: true,
                    trading_day: 0,
                },
            ),
        ]);

        assert!(result.is_err());
        assert_eq!(plans, before);
    }

    #[test]
    fn active_index_ignores_terminal_history_and_survives_a_round_trip() {
        let mut plans = PlanBook::default();
        let first = plans.create(open(0)).unwrap();
        let second = plans.create(open(1)).unwrap();
        let other = plans
            .create(PlanOpen {
                account: AccountId(2),
                ..open(0)
            })
            .unwrap();
        plans
            .apply(
                first,
                PlanEvent::Terminated {
                    reason: TerminationReason::Cancelled,
                    trading_day: 0,
                },
            )
            .unwrap();
        let successor = plans.create(open(0)).unwrap();
        // 数值 owner → sequence 排序；该列表不代表受理优先级。
        assert_eq!(plans.active_plan_ids(), vec![second, successor, other]);
        assert_eq!(
            plans.active_plan_ids_for_account(AccountId(1)),
            vec![second, successor]
        );
        assert_eq!(plans.active_plan_ids_for_account(AccountId(2)), vec![other]);
        assert_eq!(
            plans
                .active_investor_codes(AccountId(1))
                .cloned()
                .collect::<Vec<_>>(),
            vec![StockId(0), StockId(1)]
        );
        assert_eq!(
            plans.plan_ids().collect::<Vec<_>>(),
            vec![first, second, successor, other]
        );

        let restored: PlanBook =
            serde_json::from_slice(&serde_json::to_vec(&plans).unwrap()).unwrap();
        assert_eq!(restored.active_plan_ids(), vec![second, successor, other]);
        assert_eq!(
            restored.active_plan_ids_for_account(AccountId(1)),
            vec![second, successor]
        );
        assert_eq!(
            restored
                .active_investor_plan(AccountId(1), &StockId(0))
                .unwrap()
                .plan_id(),
            successor
        );
    }

    #[test]
    fn atomic_completion_updates_the_active_index_only_after_success() {
        let mut plans = PlanBook::default();
        let first = plans.create(open(0)).unwrap();
        let second = plans.create(open(1)).unwrap();
        let accepted = PlanEvent::ChildOrderAccepted {
            order_id: crate::OrderId(1),
            trading_day: 0,
        };
        let filled = PlanEvent::ChildOrderFilled {
            order_id: crate::OrderId(1),
            qty: 100,
            child_complete: true,
            trading_day: 0,
        };
        let invalid = PlanEvent::ChildOrderFilled {
            order_id: crate::OrderId(2),
            qty: 100,
            child_complete: true,
            trading_day: 0,
        };
        assert!(plans
            .apply_active_events_atomically(&[
                (first, accepted.clone()),
                (first, filled.clone()),
                (second, invalid)
            ])
            .is_err());
        assert_eq!(plans.active_plan_ids(), vec![first, second]);
        plans
            .apply_active_events_atomically(&[(first, accepted), (first, filled)])
            .unwrap();
        assert_eq!(plans.active_plan_ids(), vec![second]);
        assert_eq!(plans.plan(first).unwrap().status(), PlanStatus::Completed);
    }
}
