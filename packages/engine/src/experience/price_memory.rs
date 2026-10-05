//! 个人价格记忆只保存本人真实见过的价格锚点，不虚构观察经历。
//!
//! 每个自然人的记忆独立，不共享账户状态。单股条目只由两类真实事件修改：
//! - [`PersonalPriceMemory::observe_price`]：本人当时看见的市场价。已观察高低
//!   只累计本人所见样本，首次观察之前的历史价格永远不会被追认为亲历。
//! - [`PersonalPriceMemory::record_public_history_read`]：已观察股票的实际技术分析
//!   读取，更新独立 [`PersonalHistoryReadLedger`] 并刷新该条目的最近接触时间。
//!   玩家可读取尚未观察的公开历史，仍只写入独立读取簿，不创建价格锚点。
//!
//! 记忆上限复用持仓 + 8 个未持仓股票（[`MAX_UNHELD_WATCHLIST_STOCKS`]）。
//! 驱逐按最后实际接触时间（本人观察与本人历史读取取较晚者）排序，同分钟按
//! StockCode 稳定破同分（与 `RetailExperienceState::prune_watchlist` 的
//! 降序 `(minute, code)` 约定一致）。受保护集合（持仓 ∪ 活跃计划股票）由
//! 调用方传入；会话按持仓和活跃计划组合集合，本模块只消费该集合。
//!
//! 会话串联注意力与决策流；本模块是可恢复的纯状态。

use std::collections::{BTreeMap, BTreeSet};

use super::{
    history_reads::PersonalHistoryReadLedger, retention::RetentionCandidates,
    MAX_UNHELD_WATCHLIST_STOCKS,
};
use crate::{Money, StockCode};

/// 个人价格记忆失败。绝不静默吞掉（铁律二）。
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PriceMemoryError {
    #[error("{field} must be positive, got {cents} cents")]
    NonPositiveMoney { field: &'static str, cents: i64 },
    #[error("price memory time cannot go backwards: attempted {attempted} after {last}")]
    TimeWentBackwards { attempted: u64, last: u64 },
    #[error(
        "stock {code} has never been personally observed; a public-history read cannot create memory"
    )]
    UnobservedStock { code: String },
    #[error("public-history read could not be recorded: {0}")]
    HistoryRead(String),
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
    /// 最近一次实际接触（本人观察或公开读取取较晚者）的共享账户市场分钟；驱逐排序键。
    #[serde(with = "super::u64_decimal")]
    #[ts(type = "string")]
    pub last_touched_minute: u64,
}

impl StockPriceMemory {
    fn first_observation(price: Money, market_minute: u64, shared_touch_minute: u64) -> Self {
        Self {
            first_observed_minute: market_minute,
            first_observed_price: price,
            last_observed_minute: market_minute,
            last_observed_price: price,
            observed_high: price,
            observed_low: price,
            last_touched_minute: shared_touch_minute,
        }
    }

    fn observe_price(
        &mut self,
        price: Money,
        market_minute: u64,
        shared_touch_minute: u64,
    ) -> Result<(), PriceMemoryError> {
        self.ensure_time_not_backwards(market_minute, shared_touch_minute)?;
        self.last_observed_minute = market_minute;
        self.last_observed_price = price;
        self.observed_high = self.observed_high.max(price);
        self.observed_low = self.observed_low.min(price);
        self.last_touched_minute = shared_touch_minute;
        Ok(())
    }

    fn record_public_history_read(
        &mut self,
        code: &StockCode,
        market_minute: u64,
        history_read_market_minute: u64,
        reads: &mut PersonalHistoryReadLedger,
    ) -> Result<(), PriceMemoryError> {
        self.ensure_time_not_backwards(market_minute, history_read_market_minute)?;
        reads
            .record(code, history_read_market_minute)
            .map_err(|error| PriceMemoryError::HistoryRead(error.to_string()))?;
        self.last_touched_minute = history_read_market_minute;
        Ok(())
    }

    fn ensure_time_not_backwards(
        &self,
        market_minute: u64,
        shared_touch_minute: u64,
    ) -> Result<(), PriceMemoryError> {
        if market_minute < self.last_observed_minute {
            return Err(PriceMemoryError::TimeWentBackwards {
                attempted: market_minute,
                last: self.last_observed_minute,
            });
        }
        if shared_touch_minute < self.last_touched_minute {
            return Err(PriceMemoryError::TimeWentBackwards {
                attempted: shared_touch_minute,
                last: self.last_touched_minute,
            });
        }
        Ok(())
    }
}

/// 一个自然人独立持有的个人价格记忆；不与其他账户共享。
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PersonalPriceMemory {
    pub stocks: BTreeMap<StockCode, StockPriceMemory>,
}

impl PersonalPriceMemory {
    /// 查询单股记忆，供候选评分聚合个人判断输入。
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
    /// 更新高低。证券观察分钟与共享账户接触分钟分别校验单调，不跨时钟比较。
    pub fn observe_price(
        &mut self,
        code: &StockCode,
        price: Money,
        market_minute: u64,
        shared_touch_minute: u64,
    ) -> Result<(), PriceMemoryError> {
        if price.cents() <= 0 {
            return Err(PriceMemoryError::NonPositiveMoney {
                field: "observed price",
                cents: price.cents(),
            });
        }
        match self.stocks.get_mut(code) {
            Some(entry) => entry.observe_price(price, market_minute, shared_touch_minute)?,
            None => {
                self.stocks.insert(
                    code.clone(),
                    StockPriceMemory::first_observation(price, market_minute, shared_touch_minute),
                );
            }
        }
        Ok(())
    }

    /// 记录一次专业技术分析主动读取公开历史的事件。
    ///
    /// 读取不改变本人所见锚点与已观察高低；`market_minute` 按证券交易所时钟校验
    /// 观察时间，`history_read_market_minute` 按共享账户时钟登记读取事件及最近接触时间。
    /// 从未本人观察过的股票没有记忆条目，读取不能凭空创造亲历。
    pub fn record_public_history_read(
        &mut self,
        code: &StockCode,
        market_minute: u64,
        history_read_market_minute: u64,
        reads: &mut PersonalHistoryReadLedger,
    ) -> Result<(), PriceMemoryError> {
        let entry = self
            .stocks
            .get_mut(code)
            .ok_or_else(|| PriceMemoryError::UnobservedStock {
                code: code.0.clone(),
            })?;
        entry.record_public_history_read(code, market_minute, history_read_market_minute, reads)
    }

    /// 按最近接触时间驱逐未受保护股票，最多保留 8 个。
    ///
    /// `protected` = 持仓 ∪ 活跃计划股票（由调用方组合）。
    /// 未受保护股票按 `(last_touched_minute, StockCode)` 降序保留前
    /// [`MAX_UNHELD_WATCHLIST_STOCKS`] 个：同分钟时代码较大者保留。
    pub fn prune(&mut self, protected: &BTreeSet<StockCode>) {
        let kept = RetentionCandidates::from_contacts(
            self.stocks
                .iter()
                .map(|(code, entry)| (code, entry.last_touched_minute)),
            protected,
            MAX_UNHELD_WATCHLIST_STOCKS,
        )
        .select_kept_unprotected();
        self.stocks
            .retain(|code, _| protected.contains(code) || kept.contains(code));
    }
}

#[cfg(test)]
mod protection_tests {
    use super::*;

    #[test]
    fn prune_uses_shared_contact_time_when_exchange_clocks_differ() {
        let mut memory = PersonalPriceMemory::default();
        let mut reads = PersonalHistoryReadLedger::default();
        for index in 0..8 {
            memory
                .observe_price(
                    &StockCode(format!("000{index:03}")),
                    Money::from_cents(100),
                    1000,
                    1000,
                )
                .unwrap();
        }
        let reopened = StockCode("600101".to_owned());
        memory
            .observe_price(&reopened, Money::from_cents(100), 120, 1120)
            .unwrap();
        memory
            .record_public_history_read(&reopened, 120, 1120, &mut reads)
            .unwrap();
        memory.prune(&BTreeSet::new());
        assert_eq!(memory.stock_count(), 8);
        assert!(memory.stock(&reopened).is_some());
        assert!(memory.stock(&StockCode("000000".to_owned())).is_none());
        assert_eq!(memory.stock(&reopened).unwrap().last_observed_minute, 120);
        assert_eq!(memory.stock(&reopened).unwrap().last_touched_minute, 1120);
    }

    #[test]
    fn stock_samples_and_shared_contacts_are_independently_monotonic() {
        let mut memory = PersonalPriceMemory::default();
        let mut reads = PersonalHistoryReadLedger::default();
        let code = StockCode("600101".to_owned());
        memory
            .observe_price(&code, Money::from_cents(100), 120, 1120)
            .unwrap();
        memory
            .record_public_history_read(&code, 120, 1130, &mut reads)
            .unwrap();
        memory
            .observe_price(&code, Money::from_cents(110), 125, 1140)
            .unwrap();
        let before = memory.clone();
        assert_eq!(
            memory.observe_price(&code, Money::from_cents(90), 124, 1150),
            Err(PriceMemoryError::TimeWentBackwards {
                attempted: 124,
                last: 125
            })
        );
        assert_eq!(
            memory.observe_price(&code, Money::from_cents(90), 126, 1139),
            Err(PriceMemoryError::TimeWentBackwards {
                attempted: 1139,
                last: 1140
            })
        );
        assert_eq!(
            memory.record_public_history_read(&code, 124, 1150, &mut reads),
            Err(PriceMemoryError::TimeWentBackwards {
                attempted: 124,
                last: 125
            })
        );
        assert_eq!(
            memory.record_public_history_read(&code, 125, 1139, &mut reads),
            Err(PriceMemoryError::TimeWentBackwards {
                attempted: 1139,
                last: 1140
            })
        );
        assert_eq!(memory, before);
        assert_eq!(reads.stocks[&code].read_count, 1);
        assert_eq!(memory.stock(&code).unwrap().first_observed_minute, 120);
        assert_eq!(memory.stock(&code).unwrap().last_observed_minute, 125);
        assert_eq!(memory.stock(&code).unwrap().last_touched_minute, 1140);
    }

    #[test]
    fn price_memory_read_uses_separate_ledger_and_preserves_observation_anchors() {
        let code = StockCode("600101".to_owned());
        let mut memory = PersonalPriceMemory::default();
        let mut reads = PersonalHistoryReadLedger::default();
        assert!(memory
            .record_public_history_read(&code, 1, 1, &mut reads)
            .is_err());
        assert!(memory.stocks.is_empty());
        memory
            .observe_price(&code, Money::from_cents(100), 2, 2)
            .unwrap();
        memory
            .record_public_history_read(&code, 3, 3, &mut reads)
            .unwrap();
        let before = memory.clone();
        assert_eq!(
            memory.observe_price(&code, Money::ZERO, 1, 1),
            Err(PriceMemoryError::NonPositiveMoney {
                field: "observed price",
                cents: 0
            })
        );
        assert_eq!(
            memory.record_public_history_read(&code, 2, 2, &mut reads),
            Err(PriceMemoryError::TimeWentBackwards {
                attempted: 2,
                last: 3
            })
        );
        assert_eq!(memory, before);
        assert_eq!(reads.stocks[&code].read_count, 1);
        reads.stocks.get_mut(&code).unwrap().read_count = u32::MAX;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            memory.record_public_history_read(&code, 4, 4, &mut reads)
        }));
        assert!(result.is_err());
        let entry = &memory.stocks[&code];
        assert_eq!(reads.stocks[&code].last_read_market_minute, 3);
        assert_eq!(entry.last_touched_minute, 3);
        assert_eq!(entry.last_observed_minute, 2);
        assert_eq!(entry.observed_high, Money::from_cents(100));
        assert_eq!(entry.observed_low, Money::from_cents(100));
    }
}
