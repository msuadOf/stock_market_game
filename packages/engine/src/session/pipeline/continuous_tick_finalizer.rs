//! 在 Continuous private candidate 上执行 stock_processing 收尾与唯一一次 ReceiptAggregation/Projection。

use super::{
    adaptive_plan_chain::PlanChainFactConsumption,
    continuous_lifecycle_projection::project_continuous_retail_lifecycle,
    continuous_matching::IncrementalContinuousStockFinish,
    day_end::TradingDayEndTransition,
    event_collection::{collect_events, EventFact},
    execution_fact_producers::{adapt_continuous_execution_facts, adapt_continuous_facts},
    market_tick_transaction::MarketTransactionError as ContinuousTransactionError,
    session_execution_transaction::{
        SessionExecutionTransactionError, SessionExecutionTransactionOutput,
    },
    account_settlement::prepare_settlement_transaction_with_beliefs,
    receipt_aggregation::apply_owned_receipt_transaction,
    AccountValidationOutput, EventStableKey, IntentCandidateBatch, ReceiptSource, StepFatal,
};
use crate::session::{completed_market_minute_count, RetailOrderDiagnosticEvent};
use std::collections::BTreeMap;
use crate::{Event, GameSession, StockId};
#[cfg(test)]
use crate::Money;

pub(super) type ContinuousTickBoundary = super::phase::MarketTickBoundary;

pub(super) struct ContinuousLifecycleProjectionInput<'a> {
    pub(super) candidates: &'a IntentCandidateBatch,
    pub(super) validation: &'a AccountValidationOutput,
    pub(super) consumed: &'a PlanChainFactConsumption,
}

pub(super) struct ContinuousTickFinalizationContext<'a> {
    pub(super) boundary: ContinuousTickBoundary,
    pub(super) day_end_event_base: u64,
    pub(super) lifecycle: ContinuousLifecycleProjectionInput<'a>,
}

impl ContinuousTickBoundary {
    pub(super) fn capture(session: &GameSession) -> Result<Self, StepFatal> {
        super::phase::PhasePolicy::capture(session).boundary(session)
    }
}

/// `session` 是 Continuous 的可丢弃 candidate；stock_processing 在日终清簿前冻结行情与深度。
/// Settlement 使用原 tick 的 market minute，之后才推进时钟并按既定日界规则解锁 T+1。
pub(super) fn finalize_continuous_tick(
    session: &mut GameSession,
    finish: IncrementalContinuousStockFinish,
    mut facts: Vec<EventFact>,
    preceding_receipts: &[super::EnvelopeReceipt],
    context: ContinuousTickFinalizationContext<'_>,
) -> Result<SessionExecutionTransactionOutput, ContinuousTransactionError> {
    let ContinuousTickFinalizationContext {
        boundary,
        day_end_event_base,
        lifecycle,
    } = context;
    let finalize_error = ContinuousTransactionError::Finalization;
    if session.state.next_receipt_base != session.state.envelope_ledger.next_receipt_index() {
        return Err(finalize_error(invariant(
            "session and ledger receipt cursors disagree",
        )));
    }
    facts.extend(adapt_continuous_execution_facts(&finish.detached_facts).map_err(finalize_error)?);
    for worker in &finish.workers {
        facts.extend(
            adapt_continuous_facts(&worker.place_facts, &worker.cancel_facts, &worker.trades)
                .map_err(finalize_error)?,
        );
        // T4（ADR-0042 §6-T4「行情」面）：本股行情已由股票 owner 在收尾一次
        // 计算（worker.market_data patch），此处仅机械安装——同位 extend 保持
        // 事件顺序与 seq 派号；在场检查保留（缺席条目不以默认值掩盖）。
        install_worker_market_data(session, &mut facts, worker.market.code(), &worker.market_data)
            .map_err(finalize_error)?;
    }

    // T5（批次八十·内层事务归并）：一次汇合——worker 输出→receipt 聚合→
    // 按账户结算在收尾**单点**执行并直接安装（原 stock_execution_transaction
    // 生产包装层与中间 detach 载体退役，测试组合面保留为 cfg(test) 件；
    // 相位见证与错误分类逐字保持）。
    let stock_execution_error = |error| {
        ContinuousTransactionError::SessionExecution(
            SessionExecutionTransactionError::StockExecution(error),
        )
    };
    let mut markets = BTreeMap::new();
    let mut created_envelopes = Vec::new();
    let mut worker_batches = Vec::new();
    let mut terminal_keys = Vec::new();
    for worker in finish.workers {
        let code = worker.market.code().clone();
        if markets.contains_key(&code) {
            return Err(stock_execution_error(
                super::stock_execution_transaction::StockExecutionTransactionError::DuplicateStockWorker { code },
            ));
        }
        created_envelopes.extend(worker.created_envelopes);
        worker_batches.push(worker.receipts);
        terminal_keys.extend(worker.terminal_keys);
        markets.insert(code, worker.market);
    }
    crate::verification_evidence::enter_phase(super::TickPhase::ReceiptAggregation);
    let compute = session.compute_context();
    let (ledger_candidate, receipts) =
        apply_owned_receipt_transaction(
            session.state.envelope_ledger.clone(),
            created_envelopes,
            worker_batches,
            terminal_keys,
            &compute,
        )
        .map_err(|error| {
            stock_execution_error(
                super::stock_execution_transaction::StockExecutionTransactionError::ReceiptAggregation(error),
            )
        })?;
    crate::verification_evidence::enter_phase(super::TickPhase::SettlementShadow);
    // Settlement 消费完整审计链（PreSeal Release 前缀+本股票批）；借用的合并
    // 载体只搬元数据并复用聚合事实。输出保持股票批契约：不含前缀。
    let settlement_input = receipts
        .prepend_releases(preceding_receipts)
        .map_err(|error| {
            stock_execution_error(
                super::stock_execution_transaction::StockExecutionTransactionError::ReceiptAggregation(error),
            )
        })?;
    let prepared = prepare_settlement_transaction_with_beliefs(
        &compute,
        &session.state.accounts,
        &session.state.retail_experience,
        &session.state.belief_participants,
        &session.state.retail_projection_seen,
        crate::experience::ExperienceMoment {
            civil_date: session.civil_date(),
            market_minute: session.current_market_minute(),
            trading_day: u64::from(session.state.day),
        },
        &settlement_input,
        session.state.setup.t1_enabled,
    )
    .map_err(|error| {
        stock_execution_error(
            super::stock_execution_transaction::StockExecutionTransactionError::Settlement(error),
        )
    })?;
    crate::verification_evidence::enter_phase(super::TickPhase::DerivationAudit);
    session.state.envelope_ledger = ledger_candidate;
    session.state.next_receipt_base = session.state.envelope_ledger.next_receipt_index();
    session.state.accounts.extend(prepared.account_patch);
    session
        .state
        .retail_experience
        .extend(prepared.retail_patch);
    for (id, book) in prepared.belief_patch {
        let mut participant = session
            .state
            .belief_participants
            .remove(&id)
            .expect("prepared institutional belief participant is missing");
        *participant.belief_mut() = book;
        session.state.belief_participants.insert(id, participant);
    }
    session.state.retail_projection_seen = prepared.seen;
    for (code, market) in markets {
        session.state.markets.insert(code, market);
    }
    project_continuous_retail_lifecycle(
        session,
        lifecycle.candidates,
        lifecycle.validation,
        &finish.execution_facts,
        &receipts,
        lifecycle.consumed,
    )
    .map_err(finalize_error)?;
    for receipt in &receipts {
        if matches!(receipt.local_key.source(), ReceiptSource::Auction(_))
            && receipt.kind == super::ReceiptKind::Fill
        {
            let fact = receipts.fill_for(receipt).map_err(finalize_error)?;
            super::adaptive_plan_chain::project_aggregated_fill(session, receipt, fact)
                .map_err(finalize_error)?;
            let qty = fact.qty();
            #[cfg(feature = "simulation-diagnostics")]
            {
                let time = session.causal_time();
                session
                    .state
                    .causal
                    .record_auction_fill(
                        time,
                        receipt.envelope.order,
                        receipt.envelope.account,
                        &receipt.envelope.stock,
                        qty,
                        receipt.value_before.cents(),
                        receipt.value_after.cents(),
                    )
                    .map_err(|error| finalize_error(invariant(error)))?;
            }
            if session
                .state
                .retail_experience
                .contains_key(&receipt.envelope.account)
            {
                session
                    .state
                    .last_retail_order_events
                    .push(RetailOrderDiagnosticEvent::Filled {
                        account: receipt.envelope.account,
                        code: receipt.envelope.stock,
                        side: receipt.envelope.side,
                        order_id: receipt.envelope.order,
                        qty,
                    });
            }
        }
    }
    session.state.tick = boundary.tick_after;

    // B2 串行数据面安装（W3 缺陷 1 修复）：本拍 Trade 事件聚合进当日
    // 分钟桶镜像 + Fill 收据双腿入账——ends_day 迁移**之前**执行（迁移
    // 清桶，此处桶=当日完整）。与 runtime 的 install_market_views/
    // receipts 入账等价（共用同一套纯函数与 session 累积面——见
    /// serial_b2_surfaces 模块注释）。
    session
        .install_serial_b2_surfaces(&facts, &receipts)
        .map_err(finalize_error)?;

    if boundary.ends_day {
        facts.extend(
            ContinuousDayEndLifecycleProjection::new(
                session,
                &receipts,
                day_end_event_base,
            )
            .apply()
            .map_err(finalize_error)?,
        );
        // 当日分钟桶镜像在迁移内清空——迁移前 take 当日完整 bars，
        // 迁移（日K已提交）后冻结保留快照（runtime freeze_retained_
        // trading_day 等价：组装+日K对账+镜像 push）。
        let day_bars = session.take_serial_minute_bars_for_day();
        facts.extend(
            TradingDayEndTransition::new(session)
                .apply()
                .map_err(|error| {
                    finalize_error(invariant(&format!("continuous DayEnd failed: {error}")))
                })?,
        );
        session
            .freeze_serial_trading_day(day_bars)
            .map_err(finalize_error)?;
    }
    let mode_after = super::phase::PhasePolicy::capture(session).matching_mode();
    for market in session.state.markets.values_mut() {
        market
            .set_matching_mode(mode_after)
            .map_err(|error| finalize_error(invariant(&error.to_string())))?;
    }
    let collected = collect_events(facts, session.state.seq).map_err(finalize_error)?;
    session.state.seq = collected.next_seq;
    Ok(SessionExecutionTransactionOutput {
        facts: collected.facts,
        receipts,
        settlement: prepared.output,
    })
}

/// 只投影 DayEnd release；Settlement 与共同日终转换由外层继续执行。
struct ContinuousDayEndLifecycleProjection<'a> {
    candidate: &'a mut GameSession,
    releases: Vec<&'a super::EnvelopeReceipt>,
    day_end_event_base: u64,
    facts: Vec<EventFact>,
    #[cfg(feature = "simulation-diagnostics")]
    day_end_causal_time: crate::diagnostics::causal::FactTime,
    #[cfg(feature = "simulation-diagnostics")]
    cleared_quote_codes: std::collections::BTreeSet<StockId>,
}

impl<'a> ContinuousDayEndLifecycleProjection<'a> {
    fn new(
        session: &'a mut GameSession,
        receipts: &'a [super::EnvelopeReceipt],
        day_end_event_base: u64,
    ) -> Self {
        #[cfg(feature = "simulation-diagnostics")]
        let day_end_causal_time = {
            let mut time = session.causal_time();
            time.phase = TradingPhase::Continuous;
            time
        };
        let mut releases = receipts
            .iter()
            .filter(|receipt| matches!(receipt.local_key.source(), ReceiptSource::DayEnd(_)))
            .collect::<Vec<_>>();
        releases.sort_by(|left, right| left.envelope.cmp(&right.envelope));
        #[cfg(feature = "simulation-diagnostics")]
        let cleared_quote_codes = releases
            .iter()
            .map(|release| release.envelope.stock.clone())
            .collect::<std::collections::BTreeSet<_>>();

        Self {
            candidate: session,
            releases,
            day_end_event_base,
            facts: Vec::new(),
            #[cfg(feature = "simulation-diagnostics")]
            day_end_causal_time,
            #[cfg(feature = "simulation-diagnostics")]
            cleared_quote_codes,
        }
    }

    fn apply(mut self) -> Result<Vec<EventFact>, StepFatal> {
        for (ordinal, release) in std::mem::take(&mut self.releases).into_iter().enumerate() {
            self.apply_release(ordinal, release)?;
        }
        self.finish()
    }

    fn apply_release(
        &mut self,
        ordinal: usize,
        release: &super::EnvelopeReceipt,
    ) -> Result<(), StepFatal> {
        let key = &release.envelope;
        #[cfg(feature = "simulation-diagnostics")]
        self.candidate.causal_terminated_at(
            self.day_end_causal_time,
            (key.account, key.order, release.qty_before),
            &key.stock,
            crate::diagnostics::causal::Termination::DayEnd,
        );
        self.candidate
            .record_parent_order_canceled(key.account, &key.stock, key.order)
            .map_err(|error| invariant(&error.to_string()))?;
        self.candidate
            .remove_npc_order_lifecycle(key.account, &key.stock, key.order);
        if self
            .candidate
            .state
            .retail_experience
            .contains_key(&key.account)
        {
            self.candidate.state.last_retail_order_events.push(
                RetailOrderDiagnosticEvent::Canceled {
                    account: key.account,
                    code: key.stock.clone(),
                    order_id: key.order,
                    remaining_qty: release.qty_before,
                },
            );
        }
        let local_index = u64::try_from(ordinal)
            .ok()
            .and_then(|ordinal| self.day_end_event_base.checked_add(ordinal))
            .ok_or_else(|| invariant("continuous DayEnd event index overflow"))?;
        self.facts.push(owned_event(
            Event::OrderCanceled {
                seq: 0,
                account: key.account,
                code: key.stock.clone(),
                id: key.order,
                remaining_qty: release.qty_before,
            },
            local_index,
        ));

        Ok(())
    }

    fn finish(self) -> Result<Vec<EventFact>, StepFatal> {
        #[cfg(feature = "simulation-diagnostics")]
        for code in self.cleared_quote_codes {
            self.candidate
                .causal_snapshot_at(self.day_end_causal_time, &code);
        }
        // 计划日终迁移由 TradingDayEndTransition 统一扫描唯一 owner，
        // 本投影仅记录确认释放，不再维护第二份执行计划或待同步队列。
        Ok(self.facts)
    }
}

/// T4（行情面）：机械安装股票 owner 的行情 patch——无业务决策；错误文案与
/// 原 finalize 行情段逐字一致（在场检查保留，缺席条目不以默认值掩盖）。
fn install_worker_market_data(
    session: &mut GameSession,
    facts: &mut Vec<EventFact>,
    code: &StockId,
    data: &super::continuous_matching::ContinuousMarketData,
) -> Result<(), StepFatal> {
    if let Some(candle) = &data.candle {
        session
            .state
            .candle_book
            .install_active(code.clone(), candle.clone());
    }
    if let Some(history) = &data.price_history {
        let entry = session
            .state
            .price_history
            .get_mut(code)
            .ok_or_else(|| invariant("continuous market has no price history"))?;
        *entry = history.clone();
    }
    if let Some(minutes) = &data.minute_closes {
        let entry = session
            .state
            .market_minute_closes
            .get_mut(code)
            .ok_or_else(|| invariant("continuous market has no minute history"))?;
        *entry = minutes.clone();
    }
    facts.extend(data.events.iter().cloned());
    Ok(())
}

fn owned_event(event: Event, index: u64) -> EventFact {
    EventFact {
        key: EventStableKey::for_event(&event, index),
        event,
    }
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::continuous_tick_finalizer".to_owned(),
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use crate::session::pipeline::{Envelope, EnvelopeAudit, EnvelopeKey, FeeComponents};
    use crate::{AccountId, OrderId, Side};

    fn candidate() -> GameSession {
        let mut setup = crate::session::npc_working_quote_tests::two_stock_quote_setup();
        setup.npcs.retail_count = 1;
        setup.npcs.inst_count = 0;
        GameSession::new(setup, 42).unwrap()
    }

    fn release(code: StockId, account: AccountId, order: u64) -> super::super::EnvelopeReceipt {
        let envelope = Envelope::tick_start_existing(
            EnvelopeKey {
                account,
                stock: code,
                order: OrderId(order),
                side: Side::Sell,
            },
            Money::ZERO,
            100,
            EnvelopeAudit {
                limit: Money::from_cents(1_000),
                remaining_qty: 100,
                filled_qty: 0,
                filled_value: Money::ZERO,
                nominal: FeeComponents::ZERO,
                charged: FeeComponents::ZERO,
            },
        );
        super::super::order_receipts::day_end_release_receipt(
            &envelope,
            u32::try_from(order).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn empty_day_end_release_projection_has_no_order_outbox() {
        let mut session = candidate();
        let before = session.state.last_retail_order_events.clone();
        let facts = ContinuousDayEndLifecycleProjection::new(&mut session, &[], 10)
            .apply()
            .unwrap();
        assert!(facts.is_empty());
        assert_eq!(session.state.last_retail_order_events, before);
    }

    #[test]
    fn day_end_release_projection_orders_envelopes_and_keeps_partial_candidate_on_index_error() {
        let mut session = candidate();
        let code = session.state.markets.keys().next().unwrap().clone();
        let receipts = [
            release(code.clone(), AccountId(1), 11),
            release(code, AccountId(0), 10),
        ];
        let facts = ContinuousDayEndLifecycleProjection::new(&mut session, &receipts, 20)
            .apply()
            .unwrap();
        assert_eq!(
            facts.iter().map(|fact| &fact.event).collect::<Vec<_>>(),
            vec![
                &Event::OrderCanceled {
                    seq: 0,
                    account: AccountId(0),
                    code: receipts[1].envelope.stock.clone(),
                    id: OrderId(10),
                    remaining_qty: 100
                },
                &Event::OrderCanceled {
                    seq: 0,
                    account: AccountId(1),
                    code: receipts[0].envelope.stock.clone(),
                    id: OrderId(11),
                    remaining_qty: 100
                },
            ]
        );
        assert_ne!(facts[0].key, facts[1].key);
        assert_eq!(session.state.last_retail_order_events.len(), 1);

        let mut session = candidate();
        let error = ContinuousDayEndLifecycleProjection::new(&mut session, &receipts, u64::MAX)
            .apply()
            .unwrap_err();
        assert_eq!(error, invariant("continuous DayEnd event index overflow"));
        assert_eq!(
            session.state.last_retail_order_events.len(),
            1,
            "index 错误前已有的 candidate 生命周期写入保留，外层丢弃整个 candidate"
        );
    }
}
