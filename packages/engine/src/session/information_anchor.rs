//! 从同股真实 day／clock projection 映射个人信息锚。
//! 来源必须由 caller 校验同 owner、bindings、date 与观察阶段；此模块不重造 clock。

use super::exchange_calendar_model::{
    CalendarClockModelError, CalendarClockProjection, CalendarDayModel,
};
use crate::calendar::DayStatus;
use crate::observation::information_clock::InformationTradingAnchor;
use crate::stock::StockId;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ClockAnchorError {
    #[error(transparent)]
    Calendar(#[from] CalendarClockModelError),
    #[error("同股 day 与 clock 投影不一致：{stock:?}")]
    ProjectionMismatch { stock: StockId },
}

/// 此处只转换真实日历事实；不运行观察、估值、期限衰减或任何提交。
pub(crate) fn information_trading_anchor(
    day: &CalendarDayModel,
    clock: &CalendarClockProjection,
    stock: StockId,
) -> Result<InformationTradingAnchor, ClockAnchorError> {
    let day_row = day.stock(stock)?;
    let clock_row = clock.stock(stock)?;
    let before = day_row.sessions_before_date();
    let completed = clock_row.completed_session_count();
    match day_row.status() {
        DayStatus::Trading => {
            if clock_row.date_session_ordinal() != Some(before)
                || !(completed == before || before.checked_add(1) == Some(completed))
            {
                return Err(ClockAnchorError::ProjectionMismatch { stock });
            }
            Ok(InformationTradingAnchor::TradingDay { ordinal: before })
        }
        DayStatus::Closed(_) => {
            if clock_row.date_session_ordinal().is_some() || completed != before {
                return Err(ClockAnchorError::ProjectionMismatch { stock });
            }
            match before.checked_sub(1) {
                None => Ok(InformationTradingAnchor::NoPriorSession),
                Some(ordinal) => Ok(InformationTradingAnchor::DayBefore { ordinal }),
            }
        }
    }
}

/// 全目录只映射一次，供同一观察批的本人信念共享；不替代 caller 的 source 校验。
pub(crate) fn information_trading_anchors(
    day: &CalendarDayModel,
    clock: &CalendarClockProjection,
) -> Result<std::collections::BTreeMap<StockId, InformationTradingAnchor>, ClockAnchorError> {
    day.stocks()
        .keys()
        .map(|&stock| Ok((stock, information_trading_anchor(day, clock, stock)?)))
        .collect()
}

impl super::GameSession {
    /// 在本次真实静点只映射一次，不为 Closed 或 Completed 造 Tick。
    pub(crate) fn prepare_information_anchors(
        &self,
    ) -> Result<
        std::sync::Arc<std::collections::BTreeMap<StockId, InformationTradingAnchor>>,
        super::StepFatal,
    > {
        let (day, projection) = self.prepare_market_calendar_projection()?;
        information_trading_anchors(day.day_model(), &projection)
            .map(std::sync::Arc::new)
            .map_err(|error| super::StepFatal::InvariantViolation {
                description: error.to_string(),
                location: "GameSession::prepare_information_anchors".to_owned(),
            })
    }
}

#[cfg(test)]
mod export_tests;
