//! 结构化错误载体（ADR 0041 错误契约）。
//!
//! 引擎错误不再携带字符串：数字错误码 + 数值上下文。人话文本只在边界
//! （server 日志/前端文本表）格式化；引擎 `Display` 仅输出码与数值（调试用）。
//!
//! ## 错误码分段（u16，全引擎唯一，登记于 agents/engine-max-performance/string-free-inventory.md）
//!
//! - 1000-1599：构造/装配校验（session 装配、初始分配、公司装配）
//! - 1600-1799：市场/订单簿校验
//! - 2000-2799：存档校验（persistence、saved_runtime）
//! - 3000-3299：协议/自然日（civil/protocol）
//! - 4000-4499：公司域校验（company/operations）
//! - 5000-5299：信息/披露域
//! - 9000-9499：计划域
//!
//! 段内序号由各模块顺序分配；新增站点在清单文档登记语义（码=什么错、
//! 上下文各槽位含义），保证可追溯。

use serde::{Deserialize, Serialize};

/// 校验失败的结构化载体：无字符串。context 槽位语义按错误码在清单文档登记。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct ValidationFault {
    /// 全引擎唯一的数字错误码（见模块级分段表）。
    pub code: u16,
    /// 数值上下文（id/数量/金额分/打包日期 yyyymmdd 等），槽位含义随错误码登记。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<u64>,
}

impl ValidationFault {
    /// 携带数值上下文构造；上下文槽位语义须在清单文档登记。
    pub fn new(code: u16, context: impl Into<Vec<u64>>) -> ValidationFault {
        ValidationFault {
            code,
            context: context.into(),
        }
    }

    /// 无上下文的纯码构造。
    pub fn bare(code: u16) -> ValidationFault {
        ValidationFault {
            code,
            context: Vec::new(),
        }
    }
}

impl std::fmt::Display for ValidationFault {
    /// 调试/边界日志用：只输出码与数值，不产生人话（文本表在边界维护）。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "fault[{:04}] {:?}", self.code, self.context)
    }
}

/// 日期上下文打包：yyyymmdd（年份非负窗内恒安全）。
pub fn ymd_context(year: i32, month: u8, day: u8) -> u64 {
    (year as i64 * 10_000 + month as i64 * 100 + day as i64) as u64
}
