//! B2 件 5：个人成交确认（交割单）——事实派生域。
//!
//! 数据源=驱动器已汇集的 `DeskTradeFact`（件 3 起带双腿实收费用三分项
//! +订单序号）；**receipt_id = trade_seq×2 + leg**（leg：0=卖腿/maker、
//! 1=买腿/taker——event_bridge 的全局单调 seq 派生，确定性、全局唯一、
//! 免新号段；对照 18725d07 旧收据语义差异：旧按订单粒度 Fill 回执
//! （envelope ledger 索引），新按成交腿粒度——语义等价（每腿每账户
//! 一条、含费用三分项），id 空间不同（登记）。
//!
//! 累积合同：Continue 链恰一次（TickRecord 单次投影）；Abort 零泄漏
//! （无 TickRecord 即无 receipt）；恢复续号=seq 恢复自驱动器游标
//! （**禁直接改 seq**——派生域天然单调）。

use crate::calendar::CivilDate;
use crate::fees::FeeComponents;
use crate::orderbook::Side;
use crate::stock::StockId;
use serde::{Deserialize, Serialize};

/// 一条个人成交确认（单腿视角）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PersonalTradeConfirmation {
    /// 全局唯一回执号＝trade_seq×2+leg（腿序见模块注释）。
    #[serde(with = "crate::u64_decimal")]
    #[ts(type = "string")]
    pub receipt_id: u64,
    /// 成交自然日。
    pub civil_date: CivilDate,
    /// 证券（数值 id）。
    pub stock: StockId,
    /// 腿方向（卖腿 Sell/买腿 Buy）。
    pub side: Side,
    /// 成交价（分）。
    #[serde(with = "price_cents_decimal")]
    #[ts(type = "string")]
    pub price_cents: i64,
    /// 成交量（股）。
    pub quantity_shares: u32,
    /// 本腿成交额（分＝价×量）。
    #[serde(with = "price_cents_decimal")]
    #[ts(type = "string")]
    pub gross_cents: i64,
    /// 本腿实收费用三分项（desk 扣费处直录——件 3 数据源）。
    pub actual_fees: FeeComponents,
}

/// i64 分的十进制字符串 serde（wire 契约——对齐 `Money` 字符串形态；
/// 价格/额恒非负，canonical 无前导零语义由 Money 同式保证）。
mod price_cents_decimal {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &i64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
        String::deserialize(deserializer)?
            .parse::<i64>()
            .map_err(serde::de::Error::custom)
    }
}

impl PersonalTradeConfirmation {
    /// 从一双腿事实派生两条确认（卖腿在前——leg 0；买腿在后——leg 1）。
    /// `trade_seq`＝本笔 `Event::Trade` 的全局 seq。
    pub(crate) fn from_legs(
        trade_seq: u64,
        civil_date: CivilDate,
        fact: &crate::runtime::DeskTradeFact,
    ) -> [Self; 2] {
        let gross = i64::from(fact.qty)
            .checked_mul(fact.price)
            .expect("成交额=价×量不溢出（settle_fill 上游已按 i64 校验）");
        [
            Self {
                receipt_id: trade_seq * 2,
                civil_date,
                stock: crate::stock::StockId(u32::from(fact.stock)),
                side: Side::Sell,
                price_cents: fact.price,
                quantity_shares: fact.qty,
                gross_cents: gross,
                actual_fees: fact.maker_fee_split,
            },
            Self {
                receipt_id: trade_seq * 2 + 1,
                civil_date,
                stock: crate::stock::StockId(u32::from(fact.stock)),
                side: Side::Buy,
                price_cents: fact.price,
                quantity_shares: fact.qty,
                gross_cents: gross,
                actual_fees: fact.taker_fee_split,
            },
        ]
    }
}

/// 驱动器侧待归户确认（owner→AccountId 映射在驱动器）。
#[derive(Clone, Debug)]
pub(crate) struct PendingTradeReceipt {
    /// 归户账户。
    pub account: crate::AccountId,
    /// 确认本体。
    pub confirmation: PersonalTradeConfirmation,
}

/// 个人历史分页请求（对照旧 `PersonalTradeHistoryRequest`——可空过滤
/// 项必须显式 null，缺失=拒绝）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PersonalTradeHistoryRequest {
    pub date_from: CivilDate,
    pub date_to: CivilDate,
    #[serde(deserialize_with = "required_nullable")]
    pub stock: Option<StockId>,
    #[serde(deserialize_with = "required_nullable")]
    pub side: Option<Side>,
    #[serde(deserialize_with = "required_nullable")]
    pub before_receipt: Option<TradeHistoryReceiptCursor>,
    #[serde(deserialize_with = "required_nullable")]
    pub as_of_receipt: Option<TradeHistoryReceiptCursor>,
    pub page_size: u32,
}

/// 回执排他游标（u64 十进制字符串——wire 契约）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(transparent)]
#[ts(export)]
pub struct TradeHistoryReceiptCursor(
    #[serde(with = "crate::u64_decimal")]
    #[ts(type = "string")]
    pub u64,
);

/// 个人历史分页结果。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PersonalTradeHistoryPage {
    pub request: PersonalTradeHistoryRequest,
    pub confirmations: Vec<PersonalTradeConfirmation>,
    pub next_cursor: Option<TradeHistoryReceiptCursor>,
    pub as_of_receipt: TradeHistoryReceiptCursor,
    pub start_date: CivilDate,
    pub current_date: CivilDate,
    pub settled_through: Option<CivilDate>,
}

fn required_nullable<'de, Value: Deserialize<'de>, Deserializer: serde::Deserializer<'de>>(
    deserializer: Deserializer,
) -> Result<Option<Value>, Deserializer::Error> {
    Option::<Value>::deserialize(deserializer)
}

impl super::GameSession {
    /// 个人历史分页查询（对照旧 `query_personal_trade_history`——纯查询
    /// 无副作用：不记 history_reads、不改日志；读己之写快照=
    /// `as_of_receipt` 默认回执水位）。
    pub fn query_personal_trade_history(
        &self,
        account: crate::AccountId,
        request: PersonalTradeHistoryRequest,
    ) -> Result<PersonalTradeHistoryPage, super::SessionError> {
        self.require_healthy()?;
        if !self.state.accounts.contains_key(&account) {
            return Err(super::SessionError::UnknownHistoryAccount(account));
        }
        if let Some(stock) = &request.stock {
            if !self.state.markets.contains_key(stock) {
                return Err(super::SessionError::UnknownHistoryStock(*stock));
            }
        }
        let as_of_receipt = request
            .as_of_receipt
            .unwrap_or(TradeHistoryReceiptCursor(self.state.receipt_watermark));
        if request.date_from > request.date_to
            || !(1..=100).contains(&request.page_size)
            || as_of_receipt.0 > self.state.receipt_watermark
            || request
                .before_receipt
                .is_some_and(|before| before.0 > as_of_receipt.0)
        {
            // 1262：交割单查询参数不合法（数值码——旧字符串变体已数值化）。
            return Err(super::SessionError::InvalidHistoryRead(
                crate::error::ValidationFault::bare(1262),
            ));
        }
        let mut rows = self
            .state
            .personal_trade_confirmations
            .get(&account)
            .into_iter()
            .flat_map(|history| history.iter_rev())
            .filter(|row| {
                row.receipt_id < as_of_receipt.0
                    && request
                        .before_receipt
                        .is_none_or(|before| row.receipt_id < before.0)
                    && row.civil_date >= request.date_from
                    && row.civil_date <= request.date_to
                    && request.stock.is_none_or(|stock| row.stock == stock)
                    && request.side.is_none_or(|side| row.side == side)
            });
        let confirmations: Vec<_> = rows
            .by_ref()
            .take(request.page_size as usize)
            .cloned()
            .collect();
        let next_cursor = if rows.next().is_some() {
            confirmations
                .last()
                .map(|row| TradeHistoryReceiptCursor(row.receipt_id))
        } else {
            None
        };
        Ok(PersonalTradeHistoryPage {
            request,
            confirmations,
            next_cursor,
            as_of_receipt,
            start_date: self.state.setup.start_date,
            current_date: self.civil_date(),
            settled_through: self.state.civil_clock.settled_through(),
        })
    }

    /// 个人确认日志只读（升序 Vec——查询面降序分页自行反转）。
    pub fn personal_trade_confirmations(
        &self,
        account: crate::AccountId,
    ) -> Option<Vec<PersonalTradeConfirmation>> {
        self.state
            .personal_trade_confirmations
            .get(&account)
            .map(|history| history.iter().cloned().collect())
    }

    /// 本人交割单游标分页（B2 补面——按 18725d07 基线恢复：降序、
    /// `before_receipt` 排他游标、每页 100 条；交割单表的 onOlder 分页依赖）。
    pub fn personal_trade_confirmations_page(
        &self,
        account: crate::AccountId,
        before_receipt: Option<u64>,
    ) -> Vec<PersonalTradeConfirmation> {
        self.state
            .personal_trade_confirmations
            .get(&account)
            .map(|history| {
                history
                    .iter_rev()
                    .filter(|confirmation| {
                        before_receipt.is_none_or(|cursor| confirmation.receipt_id < cursor)
                    })
                    .take(100)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 回执水位（as_of 默认上界——恒大于一切已发 receipt_id；跨存档
    /// 由 `next_receipt_base` 持久恢复）。
    pub fn receipt_watermark(&self) -> u64 {
        self.state.receipt_watermark
    }

    /// 驱动器累积入口（step 事件组装段——恰一次投影）。
    pub(crate) fn runtime_append_trade_receipts(
        &mut self,
        receipts: &[PendingTradeReceipt],
    ) -> Result<(), String> {
        if receipts.is_empty() {
            return Ok(());
        }
        self.mark_external_mutation();
        for pending in receipts {
            // 写时 undo：账户首写尾长（回滚截断源——每拍一记录）。
            if self.state.runtime_receipts_undo_ids.insert(pending.account) {
                self.state.runtime_receipts_undo.push((
                    pending.account,
                    self.state
                        .personal_trade_confirmations
                        .get(&pending.account)
                        .map_or(0, |history| history.len()),
                ));
            }
            let history = self
                .state
                .personal_trade_confirmations
                .entry(pending.account)
                .or_default();
            // 单调校验（防御式：同 id 重复=Continue 双投影的结构性 bug）。
            if history.last().is_some_and(|last| last.receipt_id >= pending.confirmation.receipt_id)
            {
                return Err(format!(
                    "个人确认 receipt_id 回退或重复：last>={:?} 新={}",
                    history.last().map(|l| l.receipt_id),
                    pending.confirmation.receipt_id
                ));
            }
            history.push(pending.confirmation.clone());
            let id = pending.confirmation.receipt_id;
            if self.state.receipt_watermark <= id {
                self.state.receipt_watermark = id + 1;
            }
        }
        Ok(())
    }
}
