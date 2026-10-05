//! 自然日经营演化编排（经营与信息披露，自然日经营演化）。
//!
//! [`CompanyOperations`] 把经济事件（需求/成本/履约/信用参数变化）、持久化
//! 到期队列（[`crate::company::scheduler`]）与四行业处理器（
//! `IndustrialBooks`/`BankBooks`/`InsuranceBooks`/`RealEstateBooks`）接成
//! 逐自然日的经营循环：**原始订单/交付/回款事件驱动会计**，经营层绝不直接
//! 随机改财务余额；RNG 只决定经济事件的发生与参数（分流见
//! [`crate::company::rng`]）。
//!
//! 同一处理器提供初始化专用前史生成（会计与资金边界：开局前 2 个完整自然年度 + 当年
//! 截至开局前日；独立 init RNG 流；不创建历史证券成交、不组装公开报告——
//! 公开库由 information 组装，新局由 session::company_assembly 装配）。

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
mod maturity_payments;
mod real_estate;
mod restore;
mod settlements;
mod state;

pub use bank::BankFlowParams;
pub use config::{CompanyOperationsConfig, FlowParams, IndustryBooks, OperatingCompanyConfig};
pub use core::OperatingReportCorrection;
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
