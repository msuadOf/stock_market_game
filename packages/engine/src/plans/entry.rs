//! 唯一 PlanBook 内互斥的个人判断与发行人授权，不建立第二个索引或身份分配器。

use super::{
    PlanExecutionLeg, PlanId, RepurchaseExecutionPlan, RepurchaseExecutionStatus, TradingPlan,
};
use crate::{AccountId, StockId};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PlanOrigin {
    Investor,
    IssuerRepurchase,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub enum PlanEntry {
    Investor(TradingPlan),
    IssuerRepurchase(RepurchaseExecutionPlan),
}

impl PlanEntry {
    pub fn plan_id(&self) -> PlanId {
        match self {
            Self::Investor(plan) => plan.plan_id(),
            Self::IssuerRepurchase(plan) => plan.plan_id(),
        }
    }

    pub fn account(&self) -> AccountId {
        self.plan_id().owner
    }

    pub fn code(&self) -> &StockId {
        match self {
            Self::Investor(plan) => plan.code(),
            Self::IssuerRepurchase(plan) => plan.stock_ref(),
        }
    }

    pub fn origin(&self) -> PlanOrigin {
        match self {
            Self::Investor(_) => PlanOrigin::Investor,
            Self::IssuerRepurchase(_) => PlanOrigin::IssuerRepurchase,
        }
    }

    pub fn is_terminal(&self) -> bool {
        match self {
            Self::Investor(plan) => plan.is_terminal(),
            Self::IssuerRepurchase(plan) => plan.status() == RepurchaseExecutionStatus::Closed,
        }
    }

    pub fn execution(&self) -> Option<&PlanExecutionLeg> {
        match self {
            Self::Investor(plan) => plan.execution(),
            Self::IssuerRepurchase(plan) => plan.execution_ref(),
        }
    }
}

impl From<TradingPlan> for PlanEntry {
    fn from(plan: TradingPlan) -> Self {
        Self::Investor(plan)
    }
}
