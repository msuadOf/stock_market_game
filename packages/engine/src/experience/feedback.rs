//! K5 经历反馈（行 136，任务 20）：把真实经历接入信心、忍耐与风险压力。
//!
//! 状态只登记**事实**：受挫事件（双时钟日期）、每股持仓生命周期（入场/本人观察）
//! 与清仓退出历史（含冷静期）。忍耐/信心/风险压力档位是读取时从事实派生的
//! 输入（任务 22/23 聚合），不在状态里另存一份可漂移的副本；账户损益复用
//! 既有 `reference_equity`/`peak_equity`/`consecutive_failed_buys` 字段。
//!
//! 双时钟纪律（K1）：公历日期、市场分钟、交易日序分别记录、分别单调，
//! 互不换算。任何事件都不允许把任一时钟回拨；评估时刻早于已登记经历
//! （= 状态里有"未来经历"）同样类型化拒绝。
//!
//! 写入仍只允许两类真实事件（ADR-0013）：已结算成交与被接受注意力的本人
//! 观察。同一订单跨 tick 部分成交沿用 legacy 心理去重——失败只由成交后的
//! 本人不利观察或亏损清仓确认，未成交/被撤计划没有任何写入路径。
//! 衰减（每 20 个交易日无新受挫减弱一档）只调影响档，**永不删除**登记的
//! 亏损事实；真实获利退出沿用 legacy 恢复规则（计数减一）。
//!
//! 会话接线（把 session 侧 fill/observe 调用切到 `*_dated` 变体）属任务
//! 25/26；存档恢复边界调用 [`ExperienceFeedback::validate`] 属任务 27。
//!
//! 双时钟写入方法（`*_dated`）在 [`lifecycle`]；任务 22/23 消费的读取输入
//! （失败影响衰减/长期被套/风险压力）在 [`inputs`]。

mod inputs;
mod lifecycle;

use std::collections::BTreeMap;

use super::{require_positive, AppendOnlyHistory, ExperienceError, RetailExperienceState};
use crate::calendar::CivilDate;
use crate::{Money, Side, StockCode};

/// 每 20 个交易日无新受挫，失败影响减弱一档（K5；ADR-0013 衰减取代条款）。
pub const FAILURE_DECAY_TRADING_DAYS: u64 = 20;
/// 长期被套要求持有满 20 个交易日（K5）。
pub const LONG_STUCK_TRADING_DAYS: u64 = 20;

/// 一次经历事件的双时钟时刻。三个分量分别单调，绝不互相换算。
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ExperienceMoment {
    pub civil_date: CivilDate,
    #[serde(with = "super::u64_decimal")]
    #[ts(type = "string")]
    pub market_minute: u64,
    #[serde(with = "super::u64_decimal")]
    #[ts(type = "string")]
    pub trading_day: u64,
}

/// 一次被确认的受挫（失败买入经历）。日期 = 确认时刻（本人观察或亏损清仓）。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct FailureEventRecord {
    pub code: StockCode,
    #[serde(with = "super::optional_u64_decimal")]
    #[ts(type = "string | null")]
    pub order_id: Option<u64>,
    pub moment: ExperienceMoment,
}

/// 一段持仓的本人观察（最新所见价；成交价同样是本人亲历价）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct OwnObservation {
    pub price: Money,
    pub moment: ExperienceMoment,
}

/// 一段持仓生命周期（从真实建仓/开局分配到清仓）。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct HoldingEpoch {
    pub entry_moment: ExperienceMoment,
    pub last_own_observation: Option<OwnObservation>,
}

/// 一次清仓退出。冷静期历史只增不减：再入场解除活跃冷却，不抹去这里的记录。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ExitRecord {
    pub code: StockCode,
    #[serde(with = "super::u64_decimal")]
    #[ts(type = "string")]
    pub cooldown_until_market_minute: u64,
    pub realized_profit: bool,
    pub moment: ExperienceMoment,
}

/// 逐账户经历反馈事实。默认空 = 新账户或尚未接双时钟事件。
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ExperienceFeedback {
    /// 账户最近一次经历事件的双时钟时刻（各分量取最大）。
    pub latest_moment: Option<ExperienceMoment>,
    /// 受挫事件登记（追加式；衰减只作用于读取，不删条目）。
    #[ts(as = "Vec<FailureEventRecord>")]
    pub failure_events: AppendOnlyHistory<FailureEventRecord>,
    /// 每股当前持仓生命周期（未持仓/仅关注列表的股票没有条目）。
    pub stocks: BTreeMap<StockCode, HoldingEpoch>,
    /// 清仓退出历史（追加式，含冷静期截止与盈亏事实）。
    #[ts(as = "Vec<ExitRecord>")]
    pub exit_records: AppendOnlyHistory<ExitRecord>,
}

impl ExperienceFeedback {
    /// 序列化守卫：未接双时钟事件的默认反馈不写入存档——旧档字节保持不变
    /// （K1 不留迁移器；缺省字段由 `serde(default)` 恢复为空反馈）。
    pub(crate) fn is_empty(&self) -> bool {
        self.latest_moment.is_none()
            && self.failure_events.is_empty()
            && self.stocks.is_empty()
            && self.exit_records.is_empty()
    }

    /// 双时钟单调守卫（纯检查，不改状态）。
    pub(crate) fn ensure_moment_forward(
        &self,
        moment: ExperienceMoment,
    ) -> Result<(), ExperienceError> {
        let Some(latest) = self.latest_moment else {
            return Ok(());
        };
        if moment.civil_date < latest.civil_date {
            return Err(ExperienceError::CivilTimeWentBackwards {
                attempted: moment.civil_date,
                last: latest.civil_date,
            });
        }
        if moment.market_minute < latest.market_minute {
            return Err(ExperienceError::MarketMinuteWentBackwards {
                attempted: moment.market_minute,
                last: latest.market_minute,
            });
        }
        if moment.trading_day < latest.trading_day {
            return Err(ExperienceError::TradingDayWentBackwards {
                attempted: moment.trading_day,
                last: latest.trading_day,
            });
        }
        Ok(())
    }

    /// 事件提交成功后推进账户时钟（各分量取最大）。
    pub(crate) fn advance_clocks(&mut self, moment: ExperienceMoment) {
        self.latest_moment = Some(match self.latest_moment {
            None => moment,
            Some(latest) => ExperienceMoment {
                civil_date: latest.civil_date.max(moment.civil_date),
                market_minute: latest.market_minute.max(moment.market_minute),
                trading_day: latest.trading_day.max(moment.trading_day),
            },
        });
    }

    /// 读取守卫：评估时刻不得早于已登记经历（否则状态里含"未来经历"）。
    pub(crate) fn ensure_as_of_reached(
        &self,
        as_of: &ExperienceMoment,
    ) -> Result<(), ExperienceError> {
        let Some(latest) = self.latest_moment else {
            return Ok(());
        };
        if as_of.civil_date < latest.civil_date
            || as_of.market_minute < latest.market_minute
            || as_of.trading_day < latest.trading_day
        {
            return Err(ExperienceError::AsOfBeforeLatestEvent {
                as_of: *as_of,
                latest,
            });
        }
        Ok(())
    }

    /// 恢复边界一致性校验（任务 27 在存档恢复时调用；篡改的存档在此显式失败）。
    pub fn validate(&self) -> Result<(), ExperienceError> {
        let inconsistent = |detail: &str| ExperienceError::InconsistentFeedback {
            detail: detail.to_string(),
        };
        let latest_day = self.latest_moment.map_or(u64::MAX, |m| m.trading_day);
        let mut last_failure_day = 0;
        for event in &self.failure_events {
            if event.moment.trading_day < last_failure_day {
                return Err(inconsistent(
                    "failure events must be ordered by trading day",
                ));
            }
            last_failure_day = event.moment.trading_day;
            if event.moment.trading_day > latest_day {
                return Err(inconsistent(
                    "failure event is dated after the latest moment",
                ));
            }
        }
        let mut last_exit_day = 0;
        for record in &self.exit_records {
            if record.moment.trading_day < last_exit_day {
                return Err(inconsistent("exit records must be ordered by trading day"));
            }
            last_exit_day = record.moment.trading_day;
            if record.moment.trading_day > latest_day {
                return Err(inconsistent("exit record is dated after the latest moment"));
            }
            if record.cooldown_until_market_minute <= record.moment.market_minute {
                return Err(inconsistent(
                    "exit cooldown must extend past the exit minute",
                ));
            }
        }
        for epoch in self.stocks.values() {
            if epoch.entry_moment.trading_day > latest_day {
                return Err(inconsistent("holding epoch starts after the latest moment"));
            }
            if epoch
                .last_own_observation
                .is_some_and(|obs| obs.moment.trading_day < epoch.entry_moment.trading_day)
            {
                return Err(inconsistent("own observation precedes the epoch entry"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod history_storage_tests {
    use super::*;

    #[test]
    fn cloned_feedback_shares_history_until_a_real_event_is_added() {
        let date = CivilDate::from_iso("2030-01-01").unwrap();
        let code = StockCode("600101".to_owned());
        let mut original = ExperienceFeedback::default();
        for ordinal in 1..=65 {
            let moment = ExperienceMoment {
                civil_date: date,
                market_minute: ordinal,
                trading_day: ordinal,
            };
            original.latest_moment = Some(moment);
            original.failure_events.push(FailureEventRecord {
                code: code.clone(),
                order_id: Some(ordinal),
                moment,
            });
            original.exit_records.push(ExitRecord {
                code: code.clone(),
                cooldown_until_market_minute: ordinal + 120,
                realized_profit: false,
                moment,
            });
        }
        assert_eq!(original.validate(), Ok(()));

        let mut candidate = original.clone();
        assert!(std::ptr::eq(
            &candidate.failure_events[0],
            &original.failure_events[0]
        ));
        assert!(std::ptr::eq(
            &candidate.exit_records[0],
            &original.exit_records[0]
        ));

        let moment = ExperienceMoment {
            civil_date: date,
            market_minute: 66,
            trading_day: 66,
        };
        candidate.failure_events.push(FailureEventRecord {
            code: code.clone(),
            order_id: Some(66),
            moment,
        });
        candidate.exit_records.push(ExitRecord {
            code,
            cooldown_until_market_minute: 186,
            realized_profit: false,
            moment,
        });
        assert_eq!(original.failure_events.len(), 65);
        assert_eq!(original.exit_records.len(), 65);
        assert_eq!(candidate.failure_events.len(), 66);
        assert_eq!(candidate.exit_records.len(), 66);
        for (index, order_id) in [(0, 1), (31, 32), (32, 33), (64, 65), (65, 66)] {
            assert_eq!(candidate.failure_events[index].order_id, Some(order_id));
            assert_eq!(
                candidate.exit_records[index].cooldown_until_market_minute,
                order_id + 120
            );
        }
        assert!(std::ptr::eq(
            &candidate.failure_events[0],
            &original.failure_events[0]
        ));
        assert!(std::ptr::eq(
            &candidate.exit_records[0],
            &original.exit_records[0]
        ));

        let encoded = serde_json::to_value(&original).unwrap();
        assert!(encoded["failure_events"].is_array());
        assert!(encoded["exit_records"].is_array());
        assert_eq!(
            serde_json::from_value::<ExperienceFeedback>(encoded).unwrap(),
            original
        );
        let typescript = <ExperienceFeedback as ts_rs::TS>::decl(&ts_rs::Config::default());
        assert!(typescript.contains("failure_events: Array<FailureEventRecord>"));
        assert!(typescript.contains("exit_records: Array<ExitRecord>"));
    }
}
