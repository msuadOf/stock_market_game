//! 公司回购热执行来源的严格值类型，不接管计划分配、资金簿或组内身份。

use super::{PlanExecutionLeg, PlanId};
use crate::company::CompanyId;
use crate::stock::StockId;

#[derive(Copy, Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub enum RepurchaseExecutionStatus {
    Funded,
    Draining,
    Closed,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, serde::Serialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct RepurchaseExecutionPlan {
    plan_id: PlanId,
    issuer: CompanyId,
    stock: StockId,
    status: RepurchaseExecutionStatus,
    execution: Option<PlanExecutionLeg>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RepurchaseExecutionPlanWire {
    plan_id: PlanId,
    issuer: CompanyId,
    stock: StockId,
    status: RepurchaseExecutionStatus,
    #[serde(deserialize_with = "deserialize_required_execution")]
    execution: Option<PlanExecutionLeg>,
}

fn deserialize_required_execution<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<PlanExecutionLeg>, D::Error> {
    <Option<PlanExecutionLeg> as serde::Deserialize>::deserialize(deserializer)
}

impl<'de> serde::Deserialize<'de> for RepurchaseExecutionPlan {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = RepurchaseExecutionPlanWire::deserialize(deserializer)?;
        let plan = Self {
            plan_id: wire.plan_id,
            issuer: wire.issuer,
            stock: wire.stock,
            status: wire.status,
            execution: wire.execution,
        };
        plan.validate().map_err(serde::de::Error::custom)?;
        Ok(plan)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepurchaseExecutionPlanError {
    ClosedCannotDrain,
    ClosedWithActiveChild,
    InvalidExecutionParameters,
}

impl std::fmt::Display for RepurchaseExecutionPlanError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::ClosedCannotDrain => "Closed 回购执行计划不能重新进入 Draining",
            Self::ClosedWithActiveChild => "Closed 回购执行计划仍有活动子单余量",
            Self::InvalidExecutionParameters => "回购执行腿数量、价格或活动余量非法",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for RepurchaseExecutionPlanError {}

impl RepurchaseExecutionPlan {
    /// 仅准备未发布的批准候选；调用方须在同一事务核信用并成功 commit 后安装。
    pub(super) fn for_approval_candidate(
        plan_id: PlanId,
        issuer: CompanyId,
        stock: StockId,
    ) -> Self {
        Self {
            plan_id,
            issuer,
            stock,
            status: RepurchaseExecutionStatus::Funded,
            execution: None,
        }
    }

    /// 调用方须先完成并验证资金 commit；本值不会自证资金事实，也不分配身份。
    /// 不得仅凭输入 `PlanId` 调用本构造器并把它当作注资授权。
    pub fn from_committed_funding(
        plan_id: PlanId,
        issuer: CompanyId,
        stock: StockId,
        execution: Option<PlanExecutionLeg>,
    ) -> Self {
        Self {
            plan_id,
            issuer,
            stock,
            status: RepurchaseExecutionStatus::Funded,
            execution,
        }
    }

    pub const fn plan_id(&self) -> PlanId {
        self.plan_id
    }

    pub const fn issuer(&self) -> CompanyId {
        self.issuer
    }

    pub const fn stock(&self) -> StockId {
        self.stock
    }

    pub const fn stock_ref(&self) -> &StockId {
        &self.stock
    }

    pub const fn status(&self) -> RepurchaseExecutionStatus {
        self.status
    }

    pub const fn execution(&self) -> Option<PlanExecutionLeg> {
        self.execution
    }

    pub const fn execution_ref(&self) -> Option<&PlanExecutionLeg> {
        self.execution.as_ref()
    }

    /// 停止续发新 child；重复调用保持 Draining，Closed 为终态。
    pub fn begin_draining(&mut self) -> Result<(), RepurchaseExecutionPlanError> {
        match self.status {
            RepurchaseExecutionStatus::Funded => {
                self.status = RepurchaseExecutionStatus::Draining;
                Ok(())
            }
            RepurchaseExecutionStatus::Draining => Ok(()),
            RepurchaseExecutionStatus::Closed => {
                Err(RepurchaseExecutionPlanError::ClosedCannotDrain)
            }
        }
    }

    /// 校验本值类型可自行证明的结构约束，不代替上层资金与真实成交勾稽。
    pub fn validate(&self) -> Result<(), RepurchaseExecutionPlanError> {
        if self.execution.is_some_and(|leg| {
            leg.child_qty() == 0
                || leg.limit_price().cents() <= 0
                || leg
                    .active_child_remaining_qty()
                    .is_some_and(|qty| qty == 0 || qty > leg.child_qty())
        }) {
            return Err(RepurchaseExecutionPlanError::InvalidExecutionParameters);
        }
        if self.status == RepurchaseExecutionStatus::Closed
            && self
                .execution
                .is_some_and(|leg| leg.active_child_remaining_qty().is_some())
        {
            return Err(RepurchaseExecutionPlanError::ClosedWithActiveChild);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan_id() -> PlanId {
        serde_json::from_str(r#"{"owner":"9007199254740993","sequence":"7"}"#).expect("合法 PlanId")
    }

    fn execution_leg(active_child_remaining_qty: Option<u32>) -> PlanExecutionLeg {
        serde_json::from_str(&format!(
            r#"{{"child_qty":100,"limit_price":"1234","expires_market_minute":30,"active_child_remaining_qty":{}}}"#,
            active_child_remaining_qty.map_or_else(|| "null".to_owned(), |qty| qty.to_string())
        ))
        .expect("合法 PlanExecutionLeg")
    }

    fn wire(status: &str, active_child_remaining_qty: Option<u32>) -> String {
        let execution = active_child_remaining_qty.map(|qty| {
            format!(
                r#"{{"child_qty":100,"limit_price":"1234","expires_market_minute":30,"active_child_remaining_qty":{qty}}}"#
            )
        });
        let execution: Option<PlanExecutionLeg> =
            execution.map(|value| serde_json::from_str(&value).expect("执行腿fixture是JSON对象"));
        serde_json::json!({
            "plan_id": {"owner":"9007199254740993", "sequence":"7"},
            "issuer": 12,
            "stock": 33,
            "status": status,
            "execution": execution,
        })
        .to_string()
    }

    #[test]
    fn 开始排空后停止续发且重复调用保持排空状态() {
        let mut plan = RepurchaseExecutionPlan::from_committed_funding(
            plan_id(),
            CompanyId(12),
            StockId(33),
            Some(execution_leg(Some(100))),
        );
        let original_id = plan.plan_id();
        plan.begin_draining().expect("首次进入 Draining");
        assert_eq!(plan.status(), RepurchaseExecutionStatus::Draining);
        plan.begin_draining().expect("重复进入 Draining 幂等");
        assert_eq!(plan.status(), RepurchaseExecutionStatus::Draining);
        assert_eq!(plan.plan_id(), original_id);
        assert_eq!(
            plan.execution()
                .and_then(|leg| leg.active_child_remaining_qty()),
            Some(100)
        );
        assert_eq!(plan.issuer(), CompanyId(12));
        assert_eq!(plan.stock(), StockId(33));
    }

    #[test]
    fn 严格closed_wire保留终态且不能重新进入draining() {
        let mut plan = serde_json::from_str::<RepurchaseExecutionPlan>(&wire("Closed", None))
            .expect("合法 Closed wire 可恢复终态");
        assert_eq!(plan.status(), RepurchaseExecutionStatus::Closed);
        assert_eq!(plan.validate(), Ok(()));
        assert_eq!(
            plan.begin_draining(),
            Err(RepurchaseExecutionPlanError::ClosedCannotDrain)
        );
        assert_eq!(plan.status(), RepurchaseExecutionStatus::Closed);
    }

    #[test]
    fn closed带活动子单的严格wire被拒绝() {
        let error = serde_json::from_str::<RepurchaseExecutionPlan>(&wire("Closed", Some(1)))
            .expect_err("活动child仍存在时不得表达为 Closed");
        assert!(error.to_string().contains("活动子单余量"));
    }

    #[test]
    fn 真实身份与执行来源严格往返且拒绝未知字段() {
        let plan = RepurchaseExecutionPlan::from_committed_funding(
            plan_id(),
            CompanyId(12),
            StockId(33),
            Some(execution_leg(Some(100))),
        );
        let encoded = serde_json::to_string(&plan).expect("编码成功");
        let decoded: RepurchaseExecutionPlan =
            serde_json::from_str(&encoded).expect("严格wire往返成功");
        assert_eq!(decoded, plan);
        assert_eq!(decoded.plan_id().owner.0, 9_007_199_254_740_993);
        assert_eq!(decoded.plan_id().sequence, 7);
        assert_eq!(decoded.issuer(), CompanyId(12));
        assert_eq!(decoded.stock(), StockId(33));

        let with_unknown = encoded.trim_end_matches('}').to_owned() + ",\"event_id\":\"x\"}";
        assert!(serde_json::from_str::<RepurchaseExecutionPlan>(&with_unknown).is_err());
    }

    #[test]
    fn 非规范_plan_id数字编码与未知执行字段均拒绝() {
        let malformed_id = wire("Funded", None).replace("\"sequence\":\"7\"", "\"sequence\":7");
        assert!(serde_json::from_str::<RepurchaseExecutionPlan>(&malformed_id).is_err());

        let malformed_leg = wire("Funded", Some(10)).replace(
            "\"active_child_remaining_qty\":10",
            "\"active_child_remaining_qty\":10,\"unrecognized\":true",
        );
        assert!(serde_json::from_str::<RepurchaseExecutionPlan>(&malformed_leg).is_err());

        assert!(
            serde_json::from_str::<RepurchaseExecutionPlan>(&wire("Unrecognized", None)).is_err()
        );

        let mut missing_required: serde_json::Value =
            serde_json::from_str(&wire("Funded", None)).expect("完整wire");
        missing_required
            .as_object_mut()
            .expect("wire对象")
            .remove("execution");
        assert!(serde_json::from_value::<RepurchaseExecutionPlan>(missing_required).is_err());
    }
}
