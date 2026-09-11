//! K5 个人关注列表（任务 25）：发现、保留与淡出的可恢复纯状态。
//!
//! 每个自然人独立持有（K5 不共享账户状态）。条目只由真实关注事件产生：
//! [`PersonalWatchlist::record_attention`] 在注意力候选被接受、个体实际
//! 关注该股票时由接线（任务 26）调用——公共曝光只提高发现机会，新曝光
//! 不等于已读（获知只能经 `information::record_acquisition`，K4）。
//!
//! 保留与淡出：未持仓/无活跃计划条目上限
//! [`MAX_UNHELD_WATCHLIST_STOCKS`]（8），按最后实际关注分钟驱逐，同分钟按
//! StockCode 稳定破同分（与 `RetailExperienceState::prune_watchlist`、
//! `PersonalPriceMemory::prune` 的降序 `(minute, code)` 约定一致）。受保护
//! 集合（持仓 ∪ 活跃计划股票）由调用方传入，**永不**被驱逐——活跃计划终止
//! 后调用方不再列入保护，该股票即恢复可淡出。
//!
//! 列表级注意力时钟 `latest_attention_minute` 单调不减：一切条目分钟 ≤ 该
//! 值（登记路径与恢复边界共同维护），用于拒绝时间回拨与篡改存档。
//! 会话接线（观察被接受 → 登记 → 修剪）属于任务 26；本模块是纯状态。

use std::collections::{BTreeMap, BTreeSet};

use super::MAX_UNHELD_WATCHLIST_STOCKS;
use crate::StockCode;

/// 个人关注列表失败。绝不静默吞掉（铁律二）。
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum WatchlistError {
    #[error(
        "watchlist time cannot go backwards: attempted {attempted} after last attention {last}"
    )]
    TimeWentBackwards { attempted: u64, last: u64 },
    #[error("attention at market minute {attempted} is in the future (current minute {now})")]
    ObservationInFuture { attempted: u64, now: u64 },
    /// 恢复边界：条目分钟越过列表注意力时钟（篡改/损坏存档）。
    #[error("inconsistent watchlist state: {detail}")]
    InconsistentState { detail: String },
}

/// 单股关注条目（最小事实：最后实际关注的市场分钟）。
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct WatchedStock {
    /// 本人最近一次实际关注该股的绝对交易分钟；驱逐排序键。
    #[serde(with = "super::u64_decimal")]
    #[ts(type = "string")]
    pub last_observed_market_minute: u64,
}

/// 一个自然人独立持有的个人关注列表；不与其他账户共享。
#[derive(Clone, Debug, Default, PartialEq, Eq, ts_rs::TS)]
#[ts(export)]
pub struct PersonalWatchlist {
    pub stocks: BTreeMap<StockCode, WatchedStock>,
    /// 列表级注意力时钟：所有条目分钟 ≤ 该值；修剪移除条目不回拨它。
    #[ts(type = "string")]
    pub latest_attention_minute: u64,
}

/// 存档 DTO（恢复走 [`PersonalWatchlist::from_parts`] 校验）。
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PersonalWatchlistSave {
    stocks: BTreeMap<StockCode, WatchedStock>,
    #[serde(with = "super::u64_decimal")]
    latest_attention_minute: u64,
}

impl serde::Serialize for PersonalWatchlist {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        PersonalWatchlistSave {
            stocks: self.stocks.clone(),
            latest_attention_minute: self.latest_attention_minute,
        }
        .serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for PersonalWatchlist {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let save = PersonalWatchlistSave::deserialize(deserializer)?;
        Self::from_parts(save.stocks, save.latest_attention_minute)
            .map_err(serde::de::Error::custom)
    }
}

impl PersonalWatchlist {
    pub fn new() -> Self {
        Self::default()
    }

    /// 查询单股关注条目（任务 26/27 接线与恢复的面）。
    pub fn stock(&self, code: &StockCode) -> Option<&WatchedStock> {
        self.stocks.get(code)
    }

    /// 关注条目数。
    pub fn stock_count(&self) -> usize {
        self.stocks.len()
    }

    /// 登记一次真实关注（注意力候选被接受时；接线归任务 26）。
    ///
    /// 守卫：观察分钟不得晚于当前权威市场分钟（未来观察拒绝）；不得早于
    /// 列表注意力时钟（回拨拒绝——条目分钟 ≤ 时钟的不变量由本方法与恢复
    /// 边界共同维护，单一检查即覆盖单条目回拨）。返回是否新纳入关注。
    pub fn record_attention(
        &mut self,
        code: &StockCode,
        market_minute: u64,
        now_market_minute: u64,
    ) -> Result<bool, WatchlistError> {
        if market_minute > now_market_minute {
            return Err(WatchlistError::ObservationInFuture {
                attempted: market_minute,
                now: now_market_minute,
            });
        }
        if market_minute < self.latest_attention_minute {
            return Err(WatchlistError::TimeWentBackwards {
                attempted: market_minute,
                last: self.latest_attention_minute,
            });
        }
        let newly_watched = !self.stocks.contains_key(code);
        self.stocks
            .entry(code.clone())
            .or_default()
            .last_observed_market_minute = market_minute;
        self.latest_attention_minute = market_minute;
        Ok(newly_watched)
    }

    /// 按最后实际关注分钟驱逐未受保护股票，最多保留 8 个。
    ///
    /// `protected` = 持仓 ∪ 活跃计划股票（调用方组合；计划终止后不再列入，
    /// 该股票即恢复可淡出）。受保护条目**不受上限约束**——cap 超限永远不
    /// 能驱逐它们。未受保护股票按 `(last_observed_minute, StockCode)` 降序
    /// 保留前 [`MAX_UNHELD_WATCHLIST_STOCKS`] 个：同分钟代码较大者保留。
    pub fn prune(&mut self, protected: &BTreeSet<StockCode>) {
        let mut unprotected: Vec<(u64, StockCode)> = self
            .stocks
            .iter()
            .filter(|(code, _)| !protected.contains(*code))
            .map(|(code, entry)| (entry.last_observed_market_minute, code.clone()))
            .collect();
        unprotected.sort_by(|left, right| right.cmp(left));
        for (_, code) in unprotected.into_iter().skip(MAX_UNHELD_WATCHLIST_STOCKS) {
            self.stocks.remove(&code);
        }
    }

    /// 恢复边界：条目分钟不得越过列表注意力时钟。任何失败 ⇒ 状态不产生
    /// （`PublicLibrary`/`PlanBook` 先例）；修剪历史允许时钟高于现存条目。
    pub fn from_parts(
        stocks: BTreeMap<StockCode, WatchedStock>,
        latest_attention_minute: u64,
    ) -> Result<Self, WatchlistError> {
        for (code, entry) in &stocks {
            if entry.last_observed_market_minute > latest_attention_minute {
                return Err(WatchlistError::InconsistentState {
                    detail: format!(
                        "stock {code:?} attended at {} after the list clock {latest_attention_minute}",
                        entry.last_observed_market_minute
                    ),
                });
            }
        }
        Ok(Self {
            stocks,
            latest_attention_minute,
        })
    }
}
