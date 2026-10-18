//! 会话执行接缝：计划子单事实回写与订单记录。
//!
//! 母单的可恢复执行状态自 ADR-0015（2026-09-10 修订）起并入
//! `crate::plans::TradingPlan` 的执行子状态，由 `PlanBook` 唯一持有；
//! 旧的 `ParentOrderPlan` 镜像与 `parent_orders` 表已删除。

pub(in crate::session) mod reconcile_plan;
mod records;

use super::*;
