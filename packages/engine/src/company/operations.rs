//! 自然日经营演化编排（K4，任务 14）。
//!
//! [`CompanyOperations`] 把经济事件（需求/成本/履约/信用参数变化）、持久化
//! 到期队列（[`crate::company::scheduler`]）与四行业处理器（任务 8–11 的
//! `IndustrialBooks`/`BankBooks`/`InsuranceBooks`/`RealEstateBooks`）接成
//! 逐自然日的经营循环：**原始订单/交付/回款事件驱动会计**，经营层绝不直接
//! 随机改财务余额；RNG 只决定经济事件的发生与参数（分流见
//! [`crate::company::rng`]）。
//!
//! 同一处理器提供初始化专用前史生成（K2：开局前 2 个完整自然年度 + 当年
//! 截至开局前日；独立 init RNG 流；不创建历史证券成交、不组装公开报告——
//! 披露归任务 15；会话新局接线归任务 26）。

mod bank;
mod config;
mod core;
mod day;
mod dispatch;
mod error;
mod history;
mod industrial;
mod injections;
mod insurance;
mod real_estate;
mod state;

pub use bank::BankFlowParams;
pub use config::{CompanyOperationsConfig, FlowParams, IndustryBooks, OperatingCompanyConfig};
pub use core::{
    ActivatedShockRecord, CompanyDayReport, CompanyOperations, ExpiredShockRecord,
    OperatingCompany, PaymentFailureRecord,
};
pub use error::OperationsError;
pub use history::{generate_history, HistoryMeta};
pub use industrial::IndustrialFlowParams;
pub use insurance::InsuranceFlowParams;
pub use real_estate::RealEstateFlowParams;
pub use state::EconomyAggregates;

pub use crate::company::scheduler::{
    OperatingScheduler, ScheduledAction, ScheduledDue, ScheduledDueId, SchedulerError,
    SchedulerRequest,
};
