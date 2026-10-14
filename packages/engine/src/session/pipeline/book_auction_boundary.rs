//! 在现有 OrderBook 上计算指示行情和一次清算，成交腿交给公共 Envelope 转换。

use super::{
    order_receipts, private_stock_ledger::PrivateStockLedger, Envelope, EnvelopeKey,
    EnvelopeReceipt, ReceiptKind, ReceiptSource, StepFatal,
};
use crate::{GameConfig, Market, Money, OrderId, Side, StockExchange, Trade};
use std::collections::BTreeMap;

#[derive(Debug)]
pub(super) struct BookAuctionSummary {
    pub(super) indicative_price: Option<Money>,
    pub(super) matched_volume: u64,
    pub(super) imbalance: u64,
    pub(super) completed: bool,
    pub(super) clearing_price: Option<Money>,
    pub(super) clearing_volume: u64,
}

pub(super) struct BookAuctionFinish {
    pub(super) summary: BookAuctionSummary,
    pub(super) receipts: Vec<EnvelopeReceipt>,
    pub(super) terminal_keys: Vec<EnvelopeKey>,
    pub(super) trades: Vec<Trade>,
}

pub(super) fn finish_book_auction(
    market: &mut Market,
    ledger: &mut PrivateStockLedger,
    config: &GameConfig,
    exchange: StockExchange,
    complete: bool,
    closing: bool,
) -> Result<BookAuctionFinish, StepFatal> {
    let (selection, imbalance) = market
        .auction_indicative(exchange)
        .map_err(|error| invariant(&error.to_string()))?;
    let mut output = BookAuctionFinish {
        summary: BookAuctionSummary {
            indicative_price: selection.map(|item| item.0),
            matched_volume: selection.map_or(0, |item| item.1),
            imbalance,
            completed: complete,
            clearing_price: None,
            clearing_volume: 0,
        },
        receipts: Vec::new(),
        terminal_keys: Vec::new(),
        trades: Vec::new(),
    };
    if !complete {
        return Ok(output);
    }
    // source 编号只描述 Envelope 身份，不参与清算优先级。
    let mut by_id = BTreeMap::<OrderId, (EnvelopeKey, u32)>::new();
    for (index, (key, _)) in ledger.iter().enumerate() {
        let source = u32::try_from(index).map_err(|_| invariant("Auction source 身份溢出"))?;
        if by_id.insert(key.order, (key.clone(), source)).is_some() {
            return Err(invariant("同股活动 Envelope 重复 OrderId"));
        }
    }
    let cleared = market
        .execute_auction_clearing(exchange)
        .map_err(|error| invariant(&error.to_string()))?;
    output.summary.clearing_price = cleared.price;
    output.summary.clearing_volume = cleared.matched_volume;
    let mut states = BTreeMap::<EnvelopeKey, Envelope>::new();
    let mut ordinals = BTreeMap::<EnvelopeKey, u64>::new();
    for trade in &cleared.trades {
        let maker = by_id
            .get(&trade.maker_order_id)
            .ok_or_else(|| invariant("清算 maker 不在 live ledger"))?;
        let taker = by_id
            .get(&trade.taker_order_id)
            .ok_or_else(|| invariant("清算 taker 不在 live ledger"))?;
        if maker.0.account != trade.maker
            || taker.0.account != trade.taker
            || maker.0.side == taker.0.side
        {
            return Err(invariant("清算成交腿身份或方向不符"));
        }
        let gross = trade
            .price
            .mul_shares(trade.qty)
            .map_err(|error| invariant(&error.to_string()))?;
        for ((key, source), value_before) in [
            (maker, trade.maker_filled_value_before),
            (taker, trade.taker_filled_value_before),
        ] {
            if !states.contains_key(key) {
                states.insert(key.clone(), ledger.get(key)?.clone());
            }
            let ordinal = ordinals.entry(key.clone()).or_insert(0);
            let receipt = order_receipts::fill_receipt(
                states.get_mut(key).expect("已捕获清算腿 Envelope"),
                ReceiptSource::Auction(*source),
                *ordinal,
                value_before,
                trade.qty,
                gross,
                config,
            )?;
            *ordinal = ordinal
                .checked_add(1)
                .ok_or_else(|| invariant("Auction transition ordinal 溢出"))?;
            if receipt.qty_after == 0 {
                output.terminal_keys.push(key.clone());
            }
            output.receipts.push(receipt);
        }
    }
    if !closing {
        // Fill 与同源 Rollover 属于一条完整链；只在构造完整批次后应用一次，
        // 不把从 ordinal 1 开始的承接腿拆成一个假定 ordinal 0 的新批次。
        for (key, initial) in ledger.iter() {
            let envelope = states.get(key).unwrap_or(initial);
            if envelope.live() == super::ResVec::ZERO {
                continue;
            }
            let source = by_id
                .get(&key.order)
                .ok_or_else(|| invariant("开盘剩单缺少清算 source 身份"))?
                .1;
            let ordinal = ordinals.get(key).copied().unwrap_or(0);
            output.receipts.push(order_receipts::boundary_receipt(
                envelope,
                ReceiptSource::Auction(source),
                ordinal,
                ReceiptKind::Rollover,
            )?);
        }
    }
    ledger.apply_private_for_stock_round(&mut output.receipts, &output.terminal_keys)?;
    if !closing {
        market
            .set_matching_mode(crate::orderbook::MatchingMode::Continuous)
            .map_err(|error| invariant(&error.to_string()))?;
    }
    output.trades = cleared.trades;
    Ok(output)
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::book_auction_boundary".to_owned(),
    }
}
