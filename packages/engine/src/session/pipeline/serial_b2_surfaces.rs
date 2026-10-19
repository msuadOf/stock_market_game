//! B2 串行数据面安装（wasm/旧管线 finalize 的 runtime 等价安装）。
//!
//! W3 接线实证缺陷 1：`step_runtime_enabled()` 在 wasm32 恒 false ⇒ web
//! 产品走串行管线（旧管线），但 B2 三安装面（`market_minute_bars` /
//! receipts 入账 / `retained_market_days` 冻结）全在 runtime 路径
//! （`step_via_runtime` 安装链）⇒ wasm 四查询面恒空。
//!
//! 等价合同（单一事实源，不为 wasm 写第二套聚合逻辑）：
//! - **分钟桶聚合/键推导**＝`runtime::protocol::minute_bar_apply` /
//!   `continuous_minute_of_day`（desk `apply_trade` 同函数；竞价清算
//!   固定分钟同 `OPEN_AUCTION_BAR_MINUTE`/`CLOSING_AUCTION_BAR_MINUTE`
//!   常量）；数据源＝旧管线事件流 `Event::Trade`（成交权威）。
//! - **receipts**＝`PersonalTradeConfirmation` 同结构 + 旧管线
//!   `ReceiptBatch::fill_facts`（逐笔双腿 Fill 收据——每腿费用三分项为
//!   envelope 实收权威，与 desk `DeskTradeFact` 腿粒度同义；对照
//!   18725d07 旧收据按订单 Fill 回执的语义基线）；累积入口＝
//!   `runtime_append_trade_receipts`（runtime 同一面，单调/水位合同
//!   共享）。`receipt_id`＝ledger Fill 收据全局索引（持久单调——
//!   `next_receipt_base` 续号，与 runtime 的 trade_seq×2+leg 为不同
//!   id 空间；同会话单管线不混跑，跨管线存档混载由累积入口的单调
//!   校验显式拒绝）。
//! - **retained 冻结**＝`assemble_retained_day_snapshot`（runtime driver
//!   编排同函数：开市判定+日K逐位对账+快照组装）+ `set_runtime_
//!   retained_day` 镜像 push；bars 源＝日终迁移清空前的当日分钟桶
//!   镜像（runtime 侧为 world 日结 Drain 捎带——同值同拍）。
//!
//! 接线点＝`finalize_continuous_tick`（旧管线唯一提交路径——native
//! `step_with_commit_evidence`/wasm `step()` 串行分叉同走此处）。

use super::fill_fact::{FillFact, ReceiptBatch};
use super::StepFatal;
use crate::runtime::{
    minute_bar_apply, continuous_minute_of_day, CLOSING_AUCTION_BAR_MINUTE,
    OPEN_AUCTION_BAR_MINUTE, MinuteBarPhase,
};
use crate::session::protocol::EventFact;
use crate::{Event, GameSession, TradingPhase};

impl GameSession {
    /// 串行安装段 1（finalize 每拍、ends_day 迁移**之前**调用）：
    /// 本拍 `Event::Trade` 聚合进当日分钟桶镜像 + Fill 收据双腿入账。
    ///
    /// 桶键：本拍存在该股 `AuctionCompleted` ⇒ 竞价清算成交归固定分钟
    /// （开盘 565 / 收盘 900——desk `execute_clearing_at` 同常量；按
    /// `AuctionCompleted.phase` 区分）；否则连续竞价按 tick 推导墙钟
    /// 分钟（基准＝step 后 tick−1——desk `trade_minute_key` 的
    /// cur_tick−1 同式：成交撮合语义属上一拍连续段）。
    pub(super) fn install_serial_b2_surfaces(
        &mut self,
        facts: &[EventFact],
        receipts: &ReceiptBatch,
    ) -> Result<(), StepFatal> {
        self.install_serial_minute_bars(facts)?;
        self.install_serial_trade_receipts(receipts)?;
        Ok(())
    }

    /// 分钟桶聚合（数据源=事件流 Trade）。
    fn install_serial_minute_bars(&mut self, facts: &[EventFact]) -> Result<(), StepFatal> {
        // 竞价清算拍标记：股 → 固定分钟键（AuctionCompleted.phase 区分
        // 开盘/收盘——旧管线竞价撮合只在清算拍产生 Trade）。
        let mut clearing: std::collections::BTreeMap<crate::StockId, (u16, MinuteBarPhase)> =
            std::collections::BTreeMap::new();
        for fact in facts {
            if let Event::AuctionCompleted { code, phase, .. } = &fact.event {
                let key = match phase {
                    TradingPhase::CallAuction => {
                        (OPEN_AUCTION_BAR_MINUTE, MinuteBarPhase::OpenAuction)
                    }
                    TradingPhase::ClosingAuction => {
                        (CLOSING_AUCTION_BAR_MINUTE, MinuteBarPhase::ClosingAuction)
                    }
                    // 竞价完成事件的 phase 恒为竞价相位之一——其他值=
                    // 事件契约破坏，显式失败。
                    other => {
                        return Err(invariant(&format!(
                            "AuctionCompleted.phase 非竞价相位：{other:?}"
                        )))
                    }
                };
                clearing.insert(code.clone(), key);
            }
        }
        if !facts.iter().any(|fact| matches!(fact.event, Event::Trade { .. })) {
            return Ok(());
        }
        let setup = &self.state.setup;
        let tpd = u64::from(setup.ticks_per_day);
        // 撮合窗基准=step 后 tick−1（与 desk cur_tick−1 同式）。
        let basis = self.state.tick.saturating_sub(1);
        for fact in facts {
            let Event::Trade {
                code, price, qty, ..
            } = &fact.event
            else {
                continue;
            };
            let minute_key = match clearing.get(code) {
                Some(key) => *key,
                // 连续段成交：共用纯函数（内部校验连续窗——其他相位
                // tick 显式失败，与 desk trade_minute_key 的相位判别同义）。
                None => continuous_minute_of_day(
                    tpd,
                    setup.auction_ticks,
                    setup.closing_auction_ticks,
                    basis,
                )
                .map_err(|e| invariant(&format!("串行分钟归一失败：{e:?}")))?,
            };
            let bars = self
                .state
                .market_minute_bars
                .entry(code.clone())
                .or_default();
            minute_bar_apply(bars, minute_key.0, minute_key.1, *price, *qty)
                .map_err(|e| invariant(&format!("串行分钟桶聚合失败：{e:?}")))?;
        }
        self.mark_external_mutation();
        Ok(())
    }

    /// Fill 收据双腿入账（数据源=旧管线 ReceiptBatch 的逐笔 Fill 事实）。
    fn install_serial_trade_receipts(&mut self, receipts: &ReceiptBatch) -> Result<(), StepFatal> {
        let fills: Vec<&FillFact> = receipts.fill_facts().iter().collect();
        if fills.is_empty() {
            return Ok(());
        }
        let civil_date = self.civil_date();
        let mut pending: Vec<crate::session::personal_trade_history::PendingTradeReceipt> =
            Vec::with_capacity(fills.len());
        for fact in fills {
            pending.push(
                serial_fill_confirmation(fact, civil_date)
                    .map_err(|detail| invariant(&detail))?,
            );
        }
        self.runtime_append_trade_receipts(&pending)
            .map_err(|detail| invariant(&detail))?;
        Ok(())
    }

    /// 串行交易日冻结（finalize ends_day 段、`TradingDayEndTransition`
    /// **之后**调用；`day_bars`＝迁移清空**前** take 的当日镜像）。
    pub(super) fn freeze_serial_trading_day(
        &mut self,
        day_bars: Vec<Option<Vec<crate::runtime::MinuteBarFact>>>,
    ) -> Result<(), StepFatal> {
        // 同日幂等（runtime freeze_retained_trading_day 同式）。
        if self
            .retained_market_days()
            .last()
            .is_some_and(|day| day.0.date == self.civil_date())
        {
            return Ok(());
        }
        let date = self.civil_date();
        let snapshot = self
            .assemble_retained_day_snapshot(day_bars, date)
            .map_err(|detail| invariant(&detail))?;
        self.set_runtime_retained_day(snapshot);
        Ok(())
    }

    /// 串行休市/未冻结自然日补条目（`end_civil_day` 串行分支——
    /// runtime `freeze_closed_retained_day_if_needed` 等价：末条日期==
    /// settled 跳过；交易日已由 ends_day 段冻结，到达此处的都是无
    /// tick 自然日，按每股日历如实记 Closed/开市空桶）。
    pub(crate) fn freeze_serial_closed_retained_day_if_needed(
        &mut self,
        settled: crate::calendar::CivilDate,
    ) -> Result<(), StepFatal> {
        if self
            .retained_market_days()
            .last()
            .is_some_and(|day| day.0.date == settled)
        {
            return Ok(());
        }
        let empty: Vec<Option<Vec<crate::runtime::MinuteBarFact>>> =
            vec![Some(Vec::new()); self.stock_count_for_runtime()];
        let snapshot = self
            .assemble_retained_day_snapshot(empty, settled)
            .map_err(|detail| invariant(&detail))?;
        self.set_runtime_retained_day(snapshot);
        Ok(())
    }

    /// 日终迁移前快照当日分钟桶（take 语义——镜像清空由
    /// `TradingDayEndTransition` 承载，此处取走当日完整 bars 供迁移后
    /// 冻结对账）。
    pub(super) fn take_serial_minute_bars_for_day(
        &mut self,
    ) -> Vec<Option<Vec<crate::runtime::MinuteBarFact>>> {
        let n = self.stock_count_for_runtime();
        let mut out = Vec::with_capacity(n);
        for idx in 0..n {
            let code = crate::StockId(u32::try_from(idx).expect("股下标在 u32"));
            out.push(Some(std::mem::take(
                self.state.market_minute_bars.entry(code).or_default(),
            )));
        }
        out
    }
}

/// 单腿 Fill 事实 → 个人成交确认（price=成交额÷量的精确恢复——单笔
/// Fill 的 gross=price×qty，整除失败=事实链破坏，显式失败不编造）。
fn serial_fill_confirmation(
    fact: &FillFact,
    civil_date: crate::calendar::CivilDate,
) -> Result<crate::session::personal_trade_history::PendingTradeReceipt, String> {
    let key = fact.key();
    let qty = u64::from(fact.qty());
    if qty == 0 {
        return Err(format!("串行 Fill 事实量为零（账户 {:?}）", key.account));
    }
    let gross = fact.gross().cents();
    let price_cents = gross
        .checked_div(i64::try_from(qty).map_err(|_| "量转 i64 溢出")?)
        .filter(|price| price.checked_mul(i64::try_from(qty).expect("同上非负")) == Some(gross))
        .ok_or_else(|| {
            format!(
                "串行 Fill 成交额÷量非整除（账户 {:?}，gross={gross}，qty={qty}——事实链破坏",
                key.account
            )
        })?;
    Ok(crate::session::personal_trade_history::PendingTradeReceipt {
        account: key.account,
        confirmation: crate::session::PersonalTradeConfirmation {
            // receipt_id＝ledger Fill 收据全局索引（单调唯一，持久续号）。
            receipt_id: fact.index().0,
            civil_date,
            stock: key.stock.clone(),
            side: key.side,
            price_cents,
            quantity_shares: fact.qty(),
            gross_cents: gross,
            actual_fees: fact.charged(),
        },
    })
}

/// 数值错误统一换算（位置锚定本模块）。
fn invariant(detail: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: detail.to_owned(),
        location: "pipeline::serial_b2_surfaces".to_owned(),
    }
}
