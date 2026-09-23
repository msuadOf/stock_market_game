//! K5 个人价格记忆（行 135）：本人真实见过的价格锚点，与主动读取公开历史的
//! 事件记录相互区分，不虚构观察经历。
//!
//! 每个自然人的记忆独立（K5 不共享账户状态）。单股条目只由两类真实事件修改：
//! - [`PersonalPriceMemory::observe_price`]：本人当时看见的市场价。已观察高低
//!   只累计本人所见样本，首次观察之前的历史价格永远不会被追认为亲历。
//! - [`PersonalPriceMemory::record_public_history_read`]：专业技术分析主动读取
//!   公开历史（K5 唯一读取来源；任务 22/25 若新增来源需扩展为显式来源枚举）。
//!   读取行为以「来源=公开历史读取 + 时间戳」记录，不改变任何本人所见锚点。
//!
//! 记忆上限复用持仓 + 8 个未持仓股票（[`MAX_UNHELD_WATCHLIST_STOCKS`]）。
//! 驱逐按最后实际接触时间（本人观察与公开读取取较晚者）排序，同分钟按
//! StockCode 稳定破同分（与 `RetailExperienceState::prune_watchlist` 的
//! 降序 `(minute, code)` 约定一致）。受保护集合（持仓 ∪ 活跃计划股票）由
//! 调用方传入——计划链接由任务 25 接入，此处以集合参数为文档化接缝。
//!
//! 会话接线（与注意力/决策流的串联）属于任务 25/26；本模块是可恢复的纯状态。

use std::collections::{BTreeMap, BTreeSet};

use super::MAX_UNHELD_WATCHLIST_STOCKS;
use crate::{Money, StockCode};

/// 个人价格记忆失败。绝不静默吞掉（铁律二）。
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PriceMemoryError {
    #[error("{field} must be positive, got {cents} cents")]
    NonPositiveMoney { field: &'static str, cents: i64 },
    #[error(
        "price memory time cannot go backwards: attempted {attempted} after last touch {last}"
    )]
    TimeWentBackwards { attempted: u64, last: u64 },
    #[error(
        "stock {code} has never been personally observed; a public-history read cannot create memory"
    )]
    UnobservedStock { code: String },
}

/// 单股个人价格记忆。时间窗 = [首次观察, 最近观察] 的市场分钟。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct StockPriceMemory {
    /// 本人第一次观察到该股价格的绝对交易分钟。
    #[serde(with = "super::u64_decimal")]
    #[ts(type = "string")]
    pub first_observed_minute: u64,
    pub first_observed_price: Money,
    /// 本人最近一次观察到该股价格的绝对交易分钟。
    #[serde(with = "super::u64_decimal")]
    #[ts(type = "string")]
    pub last_observed_minute: u64,
    pub last_observed_price: Money,
    /// 首次观察以来本人见过的最高价；公开历史读取不改变它。
    pub observed_high: Money,
    /// 首次观察以来本人见过的最低价；公开历史读取不改变它。
    pub observed_low: Money,
    /// 最近一次主动读取公开历史的绝对交易分钟（读取被记录，不冒充亲历）。
    #[serde(with = "super::optional_u64_decimal")]
    #[ts(type = "string | null")]
    pub last_public_history_read_minute: Option<u64>,
    /// 累计主动读取公开历史次数。
    pub public_history_read_count: u32,
    /// 最近一次实际接触（本人观察或公开读取取较晚者）的市场分钟；驱逐排序键。
    #[serde(with = "super::u64_decimal")]
    #[ts(type = "string")]
    pub last_touched_minute: u64,
}

/// 一个自然人独立持有的个人价格记忆；不与其他账户共享。
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PersonalPriceMemory {
    pub stocks: BTreeMap<StockCode, StockPriceMemory>,
}

impl PersonalPriceMemory {
    /// 查询单股记忆（任务 22 聚合的记忆查询面）。
    pub fn stock(&self, code: &StockCode) -> Option<&StockPriceMemory> {
        self.stocks.get(code)
    }

    /// 记忆条目数。
    pub fn stock_count(&self) -> usize {
        self.stocks.len()
    }

    /// 记录一次本人真实观察到的市场价格。
    ///
    /// 新条目以该价格为首次/最近观察与已观察高低；已有条目推进最近观察并
    /// 更新高低。时间不允许回拨（相对该条目的最后接触分钟）。
    pub fn observe_price(
        &mut self,
        code: &StockCode,
        price: Money,
        market_minute: u64,
    ) -> Result<(), PriceMemoryError> {
        if price.cents() <= 0 {
            return Err(PriceMemoryError::NonPositiveMoney {
                field: "observed price",
                cents: price.cents(),
            });
        }
        match self.stocks.get_mut(code) {
            Some(entry) => {
                ensure_time_not_backwards(market_minute, entry.last_touched_minute)?;
                entry.last_observed_minute = market_minute;
                entry.last_observed_price = price;
                entry.observed_high = entry.observed_high.max(price);
                entry.observed_low = entry.observed_low.min(price);
                entry.last_touched_minute = market_minute;
            }
            None => {
                self.stocks.insert(
                    code.clone(),
                    StockPriceMemory {
                        first_observed_minute: market_minute,
                        first_observed_price: price,
                        last_observed_minute: market_minute,
                        last_observed_price: price,
                        observed_high: price,
                        observed_low: price,
                        last_public_history_read_minute: None,
                        public_history_read_count: 0,
                        last_touched_minute: market_minute,
                    },
                );
            }
        }
        Ok(())
    }

    /// 记录一次主动读取公开历史（K5：专业技术分析）。
    ///
    /// 读取不改变本人所见锚点与已观察高低，只记录读取事件并刷新最近接触
    /// 时间；从未本人观察过的股票没有记忆条目，读取不能凭空创造亲历。
    pub fn record_public_history_read(
        &mut self,
        code: &StockCode,
        market_minute: u64,
    ) -> Result<(), PriceMemoryError> {
        let entry = self
            .stocks
            .get_mut(code)
            .ok_or_else(|| PriceMemoryError::UnobservedStock {
                code: code.0.clone(),
            })?;
        ensure_time_not_backwards(market_minute, entry.last_touched_minute)?;
        entry.last_public_history_read_minute = Some(market_minute);
        entry.public_history_read_count = entry
            .public_history_read_count
            .checked_add(1)
            .expect("read count is bounded by observed read events per session");
        entry.last_touched_minute = market_minute;
        Ok(())
    }

    /// 按最近接触时间驱逐未受保护股票，最多保留 8 个。
    ///
    /// `protected` = 持仓 ∪ 活跃计划股票（调用方组合；计划链接在任务 25 接入）。
    /// 未受保护股票按 `(last_touched_minute, StockCode)` 降序保留前
    /// [`MAX_UNHELD_WATCHLIST_STOCKS`] 个：同分钟时代码较大者保留。
    pub fn prune(&mut self, protected: &BTreeSet<StockCode>) {
        let mut unprotected: Vec<(u64, StockCode)> = self
            .stocks
            .iter()
            .filter(|(code, _)| !protected.contains(*code))
            .map(|(code, entry)| (entry.last_touched_minute, code.clone()))
            .collect();
        unprotected.sort_by(|left, right| right.cmp(left));
        for (_, code) in unprotected.into_iter().skip(MAX_UNHELD_WATCHLIST_STOCKS) {
            self.stocks.remove(&code);
        }
    }
}

fn ensure_time_not_backwards(attempted: u64, last: u64) -> Result<(), PriceMemoryError> {
    if attempted < last {
        return Err(PriceMemoryError::TimeWentBackwards { attempted, last });
    }
    Ok(())
}
