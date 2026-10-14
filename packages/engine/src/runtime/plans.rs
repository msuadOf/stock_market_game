//! 计划链续行（批 6，F §4.3）：跨股多步操作序列经 Continue 轮往返直至排空。
//!
//! 业务语义照搬脊树 `plan_chain_candidates.rs`/`adaptive.rs`（计划步骤=条件
//! 判断+下单；根命令→结果→续行），执行形态改为 **Continue 轮**：
//! - 轮 0：根命令（计划首步）与普通决策同轮发出；
//! - 轮 k>0（Continue）：组先落账本轮实体/FillFact（批 2 可逆日志），再推进
//!   计划步骤——**跳过普通 due 筛选**（F §4.3）；
//! - 续行上报：任一计划仍有后续步骤 ⇒ `GroupReport.continuation=true`，
//!   协调者广播 Continue(round+1)；
//! - 终止：全部计划完成；轮数上限兜底（异常形状显式 `RoundsExceeded`）。
//! - 跨股：单步产物 `ops` 可指向任意交易部（组分段 flush 已按股路由）。

use super::FillFact;

/// 计划单步产物：新操作（可跨股）+ 计划是否还有后续步骤。
#[derive(Clone, Debug, Default)]
pub struct PlanStepOutput {
    /// 本步要发的新操作（复用组端实体扣除路径；可跨股）。
    pub ops: Vec<super::decision::PendingOp>,
    /// 是否还有后续步骤（true ⇒ 续行上报）。
    pub has_more_steps: bool,
}

/// 计划链抽象：步骤推进由已见 FillFact（执行结果）驱动。
pub trait PlanChain: Send {
    /// 计划身份（诊断面）。
    fn id(&self) -> u64;
    /// 推进一步：读本 tick 已见的成交事实（计划自行按 owner/order 过滤），
    /// 产出新 ops 与完成标志。根命令以空 `fills` 调用。
    fn next_step(&mut self, fills: &[FillFact]) -> PlanStepOutput;
}
