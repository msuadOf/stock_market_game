//! 前史生成（K2：开局前 2 个完整自然年度 + 当年截至开局前日）。
//!
//! 同一处理器 + 初始化专用 RNG 流（`RngStream::InitHistory`）：账套 as_of
//! 必须由调用方设为前史首日前一天（行业账套不暴露 as_of——带借款的账套会
//! 在首次计息对不齐时显式失败；无借款账套依赖调用方契约，登记 issues）。
//! 前史不创建历史证券成交、不组装公开报告、不触碰任何交易账户（结构上
//! 无该路径）；完成即切换 live RNG 流。

use crate::calendar::{CivilDate, TradingCalendar};
use crate::company::operations::config::CompanyOperationsConfig;
use crate::company::operations::core::CompanyOperations;
use crate::company::operations::error::OperationsError;

/// 前史元数据（任务 15 组装开局已发布版本时标记 `SeededPrehistory` 用）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct HistoryMeta {
    /// 前史最后一个已生成自然日（= 开局前日）。
    pub generated_through: CivilDate,
}

/// 前史首日 = 开局年 − 2 年的 1 月 1 日（两个完整自然年度的起点）。
fn history_start(start: CivilDate) -> Result<CivilDate, OperationsError> {
    CivilDate::from_ymd(start.year() - 2, 1, 1).map_err(OperationsError::Date)
}

/// 生成前史并返回定位在 `start_date` 的经营编排引擎。
pub fn generate_history(
    config: CompanyOperationsConfig,
    start_date: CivilDate,
) -> Result<CompanyOperations, OperationsError> {
    let calendar = TradingCalendar::default_v1()?;
    let floor = calendar.policy().init_only_min_start();
    let ceiling = calendar.policy().runtime_max_end();
    if start_date > ceiling {
        return Err(OperationsError::Calendar(
            crate::calendar::CalendarError::AfterRuntimeCeiling {
                date: start_date,
                ceiling,
            },
        ));
    }
    let start = history_start(start_date)?;
    if start < floor {
        return Err(OperationsError::HistoryBeforeInitFloor {
            requested: start,
            floor,
        });
    }
    let end = start_date.prev()?;
    let mut ops = CompanyOperations::build(config, start, true)?;
    ops.submit_rolling_interest(start)?;
    let mut cursor = start;
    while cursor <= end {
        ops.advance_civil_day(cursor)?;
        cursor = cursor.next()?;
    }
    // 循环自然推进到 start_date（advance 已把 next_expected 设为开局日）。
    ops.finish_history(end);
    Ok(ops)
}
