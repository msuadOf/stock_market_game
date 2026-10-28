#[cfg(test)]
mod stock_clock_calendar_tests;

use super::*;
use std::sync::Arc;
use crate::diagnostics::causal::{
    CausalError, CausalFact, CausalFactKind, CausalReport, FactTime, OrderOrigin, Quote,
    StockFactClock, Termination,
};

/// 仅含已校验的本拍增量，最外层全部成功后才可发布。
pub(crate) struct RuntimeCausalDelta {
    pub time: FactTime,
    pub raw: Vec<crate::runtime::diagnostics::DeskCausalFact>,
    kinds: Vec<(FactTime, CausalFactKind)>,
}

impl GameSession {
    pub(in crate::session) fn prepare_runtime_causal_at(
        &self,
        driver: &mut crate::runtime::drive::RuntimeDriver,
        time: FactTime,
        tick: u64,
        plans: &crate::plans::PlanBook,
        calendar: &crate::runtime::drive::PreparedDriverCalendar,
    ) -> Result<RuntimeCausalDelta, StepFatal> {
        let (raw, drain_time, clocks) = driver.take_causal_facts();
        let result = self.translate_runtime_causal(driver, tick, &raw, driver.pending_decision_traces(), plans, drain_time)
            .and_then(|(kinds, start)| {
                let drain = self.runtime_causal_drain_boundary(drain_time, start)?;
                let tick_context = match calendar {
                    crate::runtime::drive::PreparedDriverCalendar::MarketTick(context) => context,
                    _ => return Err(causal_clock_failure("Tick causal 缺少原 MarketTickContext")),
                };
                let before = causal_tick_time(tick_context, None, false)?;
                self.prepare_timed_runtime_kinds(kinds, Some(tick_context), drain, &clocks).map(|kinds| (before, kinds))
            });
        match result {
            Ok((time, kinds)) => Ok(RuntimeCausalDelta { time, raw, kinds }),
            Err(fatal) => {
                driver.retain_terminal_causal(time, raw, fatal.clone());
                Err(fatal)
            }
        }
    }

    fn translate_runtime_causal(
        &self,
        driver: &crate::runtime::drive::RuntimeDriver,
        tick: u64,
        raw: &[crate::runtime::diagnostics::DeskCausalFact],
        roots: &[crate::runtime::diagnostics::GroupDecisionTrace],
        plans: &crate::plans::PlanBook,
        drain_time: Option<FactTime>,
    ) -> Result<(Vec<CausalFactKind>, Option<usize>), StepFatal> {
        use crate::runtime::diagnostics::{DeskCausalKind, OperationSource};
        let fail = |description: String| StepFatal::InvariantViolation {
            location: "runtime::diagnostics::prepare".to_owned(),
            description,
        };
        let mut kinds = Vec::new();
        let mut decisions = std::collections::BTreeMap::new();
        // 真实 observe 根先于本拍 desk receipt；只解析本次增量，不继承 collector 的旧 Decision。
        for trace in roots {
            let account = driver.account_by_owner(trace.owner)
                .ok_or_else(|| fail(format!("causal root owner 不在名册：{:?}", trace.owner)))?;
            if trace.record.account != account || trace.record.tick != tick
                || !matches!(trace.kind, AccountKind::Inst | AccountKind::Retail | AccountKind::Hot)
                || self.account(account).map(|account| account.kind()) != Some(trace.kind)
                || !matches!(trace.causal_facts.first(), Some(CausalFactKind::Decision { account: owner }) if *owner == account)
                || decisions.contains_key(&account) {
                return Err(fail(format!("causal root 身份、tick、首 Decision 或账户唯一性错配：{account:?}")));
            }
            if trace.kind != AccountKind::Inst
                && (!trace.plan_before.is_empty() || !trace.record.plan_ids.is_empty()
                    || !trace.record.plan_changes.is_empty() || !trace.record.budget_constraints.is_empty()) {
                return Err(fail(format!("causal 非机构 root 含机构计划或预算字段：{account:?}")));
            }
            if trace.kind == AccountKind::Hot
                && (!trace.record.source_report_ids.is_empty() || trace.record.expectation_method.is_some()) {
                return Err(fail(format!("causal Hot root 含未发生的公司材料或估值：{account:?}")));
            }
            let sequence = checked_runtime_sequence(self.state.causal.fact_count(), kinds.len()).map_err(&fail)?;
            decisions.insert(account, (sequence, trace));
            for (index, fact) in trace.causal_facts.iter().enumerate() {
                let valid = match fact {
                    CausalFactKind::Decision { account: owner } => index == 0 && *owner == account,
                    CausalFactKind::Acquisition { account: owner, .. } =>
                        *owner == account && matches!(trace.kind, AccountKind::Inst | AccountKind::Retail),
                    CausalFactKind::Budget { account: owner, .. } =>
                        *owner == account && trace.kind == AccountKind::Inst,
                    _ => false,
                };
                if !valid {
                    return Err(fail(format!("causal root 含非本人或超出 kind 能力的事实：{account:?} index={index}")));
                }
                kinds.push(fact.clone());
            }
        }
        // 仅本拍 touched orders；首次观察到旧单时保留真实累计值，不补造 Submitted。
        let mut touched = std::collections::BTreeMap::<OrderId, (AccountId, crate::Side, i64)>::new();
        let mut sources = std::collections::BTreeSet::new();
        let mut drain_start = None;
        for fact in raw.iter().copied() {
            use crate::runtime::retail_lifecycle::BatchContext;
            let drain = match fact.context {
                BatchContext::Tick { tick: actual, .. } if actual == tick && drain_start.is_none() => false,
                BatchContext::Drain { ended_date, final_tick, .. }
                    if final_tick == tick && ended_date == self.civil_date() => { drain_start.get_or_insert(kinds.len()); true },
                _ => return Err(fail(format!("causal batch 身份域或次序错配：session={tick} fact={fact:?}"))),
            };
            let stock = fact.context.stock();
            let code = StockId(u32::from(stock));
            if !self.state.markets.contains_key(&code) {
                return Err(fail(format!("causal stock 不存在：{code:?}")));
            }
            let quote = |raw: crate::runtime::diagnostics::DeskQuote| Quote {
                code,
                bid_cents: raw.bid_cents,
                ask_cents: raw.ask_cents,
                bid_depth: raw.bid_depth,
                ask_depth: raw.ask_depth,
            };
            let identity = |order: crate::runtime::OrderRef, owner: crate::runtime::Owner| {
                if order.0 != stock || order.1 > u64::from(u32::MAX) {
                    return Err(fail(format!(
                        "causal OrderRef 无效：stock={} order={order:?}",
                        stock
                    )));
                }
                if usize::try_from(owner.1).map_or(true, |local| local >= driver.accounts_per_group()) {
                    return Err(fail(format!("causal owner 局部坐标越界：{owner:?}")));
                }
                let account = driver
                    .account_by_owner(owner)
                    .ok_or_else(|| fail(format!("causal owner 不在名册：{owner:?}")))?;
                Ok((OrderId((u64::from(order.0) << 32) | order.1), account))
            };
            match fact.kind {
                DeskCausalKind::Submitted {
                    order,
                    owner,
                    side,
                    qty,
                    source,
                    before,
                } => {
                    if drain { return Err(fail("Drain 不得含 Submitted".to_owned())); }
                    let (order, account) = identity(order, owner)?;
                    let (plan, decision) = match source {
                        OperationSource::PlayerExternal => (None, None),
                        OperationSource::ObservedNpc { kind, observed_tick } => {
                            let (sequence, trace) = decisions.get(&account).ok_or_else(|| fail(format!(
                                "causal NPC 自动单缺当次真实 Decision：{account:?}"
                            )))?;
                            if !matches!(kind, AccountKind::Retail | AccountKind::Hot)
                                || trace.kind != kind || observed_tick != tick
                                || !trace.record.codes.contains(&code) {
                                return Err(fail(format!("causal NPC 自动单 kind、观察 tick 或证券错配：{account:?} {kind:?} {observed_tick} {code:?}")));
                            }
                            (None, Some(*sequence))
                        }
                        OperationSource::InstitutionPlan { plan, observed_tick } => {
                            let (sequence, trace) = decisions.get(&account).ok_or_else(|| fail(format!(
                                "causal 机构子单缺当次真实 Decision：{account:?} {plan:?}"
                            )))?;
                            if observed_tick != tick || plan.owner != account
                                || !trace.record.plan_ids.contains(&plan) {
                                return Err(fail(format!("causal 机构子单完整 PlanId/观察 tick 错配：{plan:?} {observed_tick}")));
                            }
                            let current = plans.plan(plan).map_err(|error| fail(format!(
                                "causal 机构子单 PlanId 在最终候选不存在：{plan:?} {error}"
                            )))?;
                            if *current.code() != code || current.direction() != side {
                                return Err(fail(format!("causal 机构子单 PlanId 证券或方向错配：{plan:?} {code:?} {side:?}")));
                            }
                            (Some(plan), Some(*sequence))
                        }
                        _ => return Err(fail(format!(
                            "causal Submitted 来源尚未接通：{source:?} order={order:?}"
                        ))),
                    };
                    if qty == 0 || touched.insert(order, (account, side, 0)).is_some() {
                        return Err(fail(format!("causal Submitted 数量或身份重复：{order:?}")));
                    }
                    kinds.push(CausalFactKind::Quote(quote(before)));
                    kinds.push(CausalFactKind::Submitted(OrderOrigin {
                        order,
                        account,
                        code,
                        company: self.state.issuer_of_stock(code),
                        plan,
                        decision,
                        side,
                        qty,
                    }));
                }
                DeskCausalKind::Cancelled {
                    order,
                    owner,
                    qty,
                    source,
                } => {
                    if drain { return Err(fail("Drain 不得含 Voluntary Cancelled".to_owned())); }
                    if !matches!(source, OperationSource::PlayerExternal | OperationSource::InstitutionCancel) {
                        return Err(fail(format!(
                            "causal Cancelled 来源尚未接通：{source:?} order={order:?}"
                        )));
                    }
                    let (order, account) = identity(order, owner)?;
                    if source == OperationSource::InstitutionCancel
                        && self.account(account).map(|account| account.kind()) != Some(AccountKind::Inst) {
                        return Err(fail(format!("causal 机构撤单 owner 非机构：{account:?}")));
                    }
                    kinds.push(CausalFactKind::Terminated {
                        order,
                        account,
                        code,
                        qty,
                        reason: Termination::Voluntary,
                    });
                }
                DeskCausalKind::Execution { ordinal, seller, buyer, qty, price_cents, side, before } => {
                    if !sources.insert((fact.context, ordinal)) {
                        return Err(fail(format!("causal Execution source 重复：{fact:?}")));
                    }
                    if drain {
                        let time = drain_time.ok_or_else(|| fail("Drain Execution 缺少本批已校验时间".to_owned()))?;
                        if time.phase != TradingPhase::ClosingAuction || side.is_some() {
                            return Err(fail("Drain Execution 必须为真实 ClosingAuction 且无主动方向".to_owned()));
                        }
                    } else {
                        let committed = driver.committed_calendar_tick().map_err(&fail)?;
                        if let Some(committed) = committed {
                            let auction = matches!(committed.calendar.phase_before(), TradingPhase::CallAuction | TradingPhase::ClosingAuction);
                            if committed.calendar.after_input().tick != tick || auction != side.is_none() {
                                return Err(fail("Execution 主动方向与真实 Calendar auction 相位错配".to_owned()));
                            }
                        } else if side.is_none() {
                            return Err(fail("auction Execution 缺少真实 Calendar".to_owned()));
                        }
                    }
                    if qty == 0 || price_cents <= 0 || seller.order == buyer.order || seller.owner == buyer.owner {
                        return Err(fail(format!("causal Execution 双腿或价量无效：{fact:?}")));
                    }
                    let gross = price_cents.checked_mul(i64::from(qty))
                        .ok_or_else(|| fail("causal gross 溢出".to_owned()))?;
                    let (sell_order, sell_account) = identity(seller.order, seller.owner)?;
                    let (buy_order, buy_account) = identity(buyer.order, buyer.owner)?;
                    for (order, account, leg_side, value_before) in [
                        (sell_order, sell_account, crate::Side::Sell, seller.value_before),
                        (buy_order, buy_account, crate::Side::Buy, buyer.value_before),
                    ] {
                        if value_before < 0 {
                            return Err(fail(format!("causal value_before 为负：{order:?}")));
                        }
                        if let Some(previous) = touched.get(&order) {
                            if *previous != (account, leg_side, value_before) {
                                return Err(fail(format!("causal 同单累计链或 Owner/side 错配：{order:?} before={value_before}")));
                            }
                        }
                        let after = value_before.checked_add(gross)
                            .ok_or_else(|| fail("causal after 溢出".to_owned()))?;
                        touched.insert(order, (account, leg_side, after));
                        kinds.push(CausalFactKind::Filled { order, account, code, qty, value_before, gross });
                    }
                    kinds.push(CausalFactKind::Execution {
                        code, maker: sell_order, taker: buy_order, side, qty,
                        price_cents, before: quote(before),
                    });
                }
                DeskCausalKind::DayEnded { order, owner, qty } => {
                    if !drain || qty == 0 { return Err(fail("DayEnded 必须来自真实非空 Drain 余单".to_owned())); }
                    let (order, account) = identity(order, owner)?;
                    kinds.push(CausalFactKind::Terminated { order, account, code, qty, reason: Termination::DayEnd });
                }
                DeskCausalKind::Quote(raw) => kinds.push(CausalFactKind::Quote(quote(raw))),
                DeskCausalKind::Unsupported { detail } => return Err(fail(detail.to_owned())),
            }
        }
        checked_runtime_sequence(self.state.causal.fact_count(), kinds.len()).map_err(fail)?;
        Ok((kinds, drain_start))
    }

    fn runtime_causal_drain_boundary(
        &self,
        time: Option<FactTime>,
        start: Option<usize>,
    ) -> Result<Option<(usize, FactTime)>, StepFatal> {
        let drain = if let Some(start) = start {
            let time = time.ok_or_else(|| StepFatal::InvariantViolation {
                location: "runtime::diagnostics::prepare".to_owned(), description: "非空 Drain 缺少本批已校验 Calendar 时间".to_owned(),
            })?;
            Some((start, time))
        } else {
            if time.is_some() { return Err(StepFatal::InvariantViolation {
                location: "runtime::diagnostics::prepare".to_owned(), description: "空 Drain 不得携带旧批时间".to_owned(),
            }); }
            None
        };
        Ok(drain)
    }

    /// 手动日结仅解析本次 Drain，不借用上一次低层 step 留下的 Tick 或 Decision。
    pub(crate) fn prepare_runtime_drain_causal(
        &self,
        driver: &mut crate::runtime::drive::RuntimeDriver,
        plans: &crate::plans::PlanBook,
    ) -> Result<Option<RuntimeCausalDelta>, StepFatal> {
        let (raw, drain_time, clocks) = driver.take_drain_causal_facts();
        if raw.is_empty() {
            self.runtime_causal_drain_boundary(drain_time, None)?;
            if !clocks.is_empty() { return Err(causal_clock_failure("空 Drain 不得携旧批 stock clocks")); }
            return Ok(None);
        }
        let time = match drain_time {
            Some(time) => time,
            None => {
                let fatal = StepFatal::InvariantViolation {
                    location: "runtime::diagnostics::prepare".to_owned(), description: "非空 Drain 缺少本批已校验 Calendar 时间".to_owned(),
                };
                driver.retain_terminal_causal_without_time(raw, fatal.clone());
                return Err(fatal);
            }
        };
        let result = self.translate_runtime_causal(driver, self.tick(), &raw, &[], plans, Some(time))
            .and_then(|(kinds, start)| {
                let drain = self.runtime_causal_drain_boundary(Some(time), start)?;
                self.prepare_timed_runtime_kinds(kinds, None, drain, &clocks)
            });
        match result {
            Ok(kinds) => Ok(Some(RuntimeCausalDelta { time, raw, kinds })),
            Err(fatal) => { driver.retain_terminal_causal(time, raw, fatal.clone()); Err(fatal) }
        }
    }

    // 纯 raw 翻译 fixture；真实入口只消费 caller 已持有的原 Arc。
    #[cfg(test)]
    fn prepare_runtime_causal_facts(
        &self,
        driver: &crate::runtime::drive::RuntimeDriver,
        facts: Vec<crate::runtime::diagnostics::DeskCausalFact>,
    ) -> Result<RuntimeCausalDelta, StepFatal> {
        let (kinds, start) = self.translate_runtime_causal(driver, self.tick(), &facts, driver.pending_decision_traces(), &self.state.plans, None)?;
        let drain = self.runtime_causal_drain_boundary(None, start)?;
        let (_, context) = self.prepare_market_calendar_tick()?;
        let time = causal_tick_time(&context, None, false)?;
        let kinds = self.prepare_timed_runtime_kinds(kinds, Some(&context), drain, &BTreeMap::new())?;
        Ok(RuntimeCausalDelta { time, raw: facts, kinds })
    }

    fn prepare_timed_runtime_kinds(
        &self,
        kinds: Vec<CausalFactKind>,
        tick: Option<&crate::runtime::MarketTickContext>,
        drain: Option<(usize, FactTime)>,
        clocks: &BTreeMap<StockId, u64>,
    ) -> Result<Vec<(FactTime, CausalFactKind)>, StepFatal> {
        if drain.is_none() && !clocks.is_empty() {
            return Err(causal_clock_failure("无 Drain 身份却携本股 clocks"));
        }
        if drain.is_some_and(|(_, time)| time.stock_clock.is_some()) {
            return Err(causal_clock_failure("Drain shared 根时间不得携 stock clock"));
        }
        kinds.into_iter().enumerate().map(|(index, kind)| {
            let code = kind.stock_code();
            let time = if let Some((_, drain_time)) = drain.filter(|(start, _)| index >= *start) {
                FactTime { stock_clock: code.map(|code| clocks.get(&code).copied()
                    .map(|completed_minutes| StockFactClock { code, completed_minutes })
                    .ok_or_else(|| causal_clock_failure(format!("Drain 缺本批证券 clock：{code:?}"))))
                    .transpose()?, ..drain_time }
            } else {
                causal_tick_time(tick.ok_or_else(|| causal_clock_failure("非 Drain 证券事实缺原 TickArc"))?, code, false)?
            };
            let sequence = checked_runtime_sequence(self.state.causal.fact_count(), index).map_err(causal_clock_failure)?;
            CausalFact { sequence, time, kind: kind.clone() }.validate_stock_clock()
                .map_err(|error| causal_clock_failure(error.to_string()))?;
            Ok((time, kind))
        }).collect()
    }

    pub(crate) fn publish_runtime_causal(&mut self, delta: RuntimeCausalDelta) {
        for (time, kind) in delta.kinds {
            if let CausalFactKind::Decision { account } = kind {
                self.state.causal.record_plan_root(time, account, [CausalFactKind::Decision { account }]);
            } else {
                self.state.causal.record(time, kind);
            }
        }
    }

    pub fn causal_facts(&self) -> &[CausalFact] {
        self.state.causal.facts()
    }

    pub fn causal_diagnostics(&self) -> Result<CausalReport, CausalError> {
        CausalReport::from_facts(self.state.seed, self.state.causal.facts())
    }

    pub(super) fn causal_time(&self) -> Result<FactTime, StepFatal> {
        if self.state.causal.active_tick_context.is_some() {
            let context = self.causal_tick_context()?;
            if context.before_input().tick != self.tick() {
                return Err(causal_clock_failure("root causal 时点不属于本拍 before 输入"));
            }
            return causal_tick_time(context, None, false);
        }
        self.causal_static_time()
    }

    pub(super) fn causal_static_time(&self) -> Result<FactTime, StepFatal> {
        let (_, projection) = self.prepare_market_calendar_projection()?;
        Ok(FactTime {
            phase: self.phase(),
            market_minute: projection.completed_shared_minute_count(),
            stock_clock: None,
            civil: self.observation_civil_instant(),
        })
    }

    pub(super) fn causal_tick_context(&self) -> Result<&Arc<crate::runtime::MarketTickContext>, StepFatal> {
        let context = self.state.causal.active_tick_context.as_ref()
            .ok_or_else(|| causal_clock_failure("证券 causal producer 缺少本拍原 TickArc"))?;
        context.validate_for(self.market_calendar_source(), &self.market_calendar_bindings()?, self.civil_date(),
            super::exchange_calendar_model::SessionTickLayout {
                ticks_per_day: self.state.setup.ticks_per_day,
                auction_ticks: self.state.setup.auction_ticks,
                closing_auction_ticks: self.state.setup.closing_auction_ticks,
            }, context.before_input())
            .map_err(|error| causal_clock_failure(error.to_string()))?;
        Ok(context)
    }

    pub(super) fn causal_stock_time(&self, code: StockId, after: bool) -> Result<FactTime, StepFatal> {
        let context = self.causal_tick_context()?;
        let expected_tick = if after { context.after_input().tick } else { context.before_input().tick };
        if expected_tick != self.tick() {
            return Err(causal_clock_failure("证券 causal 时点不属于本拍 before/after 输入"));
        }
        causal_tick_time(context, Some(code), after)
    }

    pub(super) fn causal_record(&mut self, kind: CausalFactKind) -> Result<(), StepFatal> {
        let time = match kind.stock_code() {
            Some(code) => self.causal_stock_time(code, false)?,
            None => self.causal_time()?,
        };
        self.causal_record_at(time, kind)
    }

    pub(super) fn causal_record_at(&mut self, time: FactTime, kind: CausalFactKind) -> Result<(), StepFatal> {
        CausalFact { sequence: self.state.causal.fact_count() as u64, time, kind: kind.clone() }
            .validate_stock_clock().map_err(|error| causal_clock_failure(error.to_string()))?;
        self.state.causal.record(time, kind);
        Ok(())
    }

    pub(super) fn causal_quote(&self, code: &StockId) -> Quote {
        let market = &self.state.markets[code];
        Quote {
            code: code.clone(),
            bid_cents: market.best_bid().map(|price| price.cents()),
            ask_cents: market.best_ask().map(|price| price.cents()),
            bid_depth: market.bid_depth().iter().map(|(_, qty)| qty).sum(),
            ask_depth: market.ask_depth().iter().map(|(_, qty)| qty).sum(),
        }
    }

    pub(super) fn causal_snapshot(&mut self, code: &StockId) -> Result<(), StepFatal> {
        self.causal_snapshot_at(self.causal_stock_time(*code, false)?, code)
    }

    pub(super) fn causal_snapshot_at(&mut self, time: FactTime, code: &StockId) -> Result<(), StepFatal> {
        self.causal_record_at(time, CausalFactKind::Quote(self.causal_quote(code)))
    }

    #[cfg(test)]
    pub(super) fn causal_submitted(&mut self, order: &Order, code: &StockId) -> Result<(), StepFatal> {
        let quote = self.causal_quote(code);
        let (_, context) = self.prepare_market_calendar_tick()?;
        self.state.causal.active_tick_context = Some(context);
        let result = self.causal_submitted_with_quote(order.owner, order.id, code, order.side, order.qty, quote);
        self.state.causal.active_tick_context = None;
        result
    }

    pub(super) fn causal_submitted_with_quote(
        &mut self,
        account: AccountId,
        order: OrderId,
        code: &StockId,
        side: crate::Side,
        qty: u32,
        quote: Quote,
    ) -> Result<(), StepFatal> {
        if quote.code != *code {
            return Err(causal_clock_failure("Submitted 与 before Quote 证券错配"));
        }
        self.causal_record(CausalFactKind::Quote(quote))?;
        let plan = self
            .state
            .plans
            .active_investor_plan(account, code)
            .filter(|plan| plan.direction() == side)
            .map(|plan| plan.plan_id());
        self.causal_record(CausalFactKind::Submitted(OrderOrigin {
            order,
            account,
            code: code.clone(),
            company: self.state.issuer_of_stock(*code),
            plan,
            decision: self.state.causal.decision_for(account),
            side,
            qty,
        }))
    }

    pub(super) fn causal_terminated(
        &mut self,
        order: (AccountId, OrderId, u32),
        code: &StockId,
        reason: Termination,
    ) -> Result<(), StepFatal> {
        self.causal_terminated_at(self.causal_stock_time(*code, false)?, order, code, reason)
    }

    pub(super) fn causal_terminated_at(
        &mut self,
        time: FactTime,
        order: (AccountId, OrderId, u32),
        code: &StockId,
        reason: Termination,
    ) -> Result<(), StepFatal> {
        self.causal_record_at(
            time,
            CausalFactKind::Terminated {
                account: order.0,
                order: order.1,
                qty: order.2,
                code: code.clone(),
                reason,
            },
        )
    }
}

fn causal_clock_failure(description: impl Into<String>) -> StepFatal {
    StepFatal::InvariantViolation { location: "causal::clock".to_owned(), description: description.into() }
}

fn causal_tick_time(context: &crate::runtime::MarketTickContext, code: Option<StockId>, after: bool) -> Result<FactTime, StepFatal> {
    let projection = if after { context.after_projection() } else { context.before_projection() };
    let stock_clock = code.map(|code| {
        let clock = projection.stock(code).map_err(|error| causal_clock_failure(error.to_string()))?;
        if clock.date_session_ordinal().is_none() {
            return Err(causal_clock_failure(format!("Closed 证券不得产生 causal 事实：{code:?}")));
        }
        Ok(StockFactClock { code, completed_minutes: clock.completed_minute_count() })
    }).transpose()?;
    Ok(FactTime { phase: context.phase_before(), market_minute: projection.completed_shared_minute_count(),
        stock_clock, civil: context.observation_instant_after() })
}

fn checked_runtime_sequence(count: usize, delta: usize) -> Result<u64, String> {
    let count = u64::try_from(count).map_err(|_| "causal 历史长度超出 sequence 域".to_owned())?;
    let delta = u64::try_from(delta).map_err(|_| "causal delta 长度超出 sequence 域".to_owned())?;
    count.checked_add(delta).ok_or_else(|| "causal sequence 溢出".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submitted_origin_resolves_company_from_the_stock_catalog() {
        let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
        setup.stocks[0].code = StockCode("600101".to_owned());
        let mut configs = crate::company::default_companies(setup.start_date).unwrap();
        configs.retain(|company| company.spec.code == "C-600101");
        // 保留独立 ID 负控；同类未上市实体与经营规格一起构造。
        let mut unlisted = configs[0].clone();
        unlisted.spec.code = "A-UNLISTED".to_owned();
        unlisted.spec.listed_stock = None;
        configs.push(unlisted);
        for company in &mut configs {
            if company.spec.listed_stock.is_some() {
                setup.stocks[0].total_shares = company.spec.issued_shares;
            } else {
                company.spec.code = "A-UNLISTED".to_owned();
            }
        }
        let registry = crate::company::CompanyRegistry::new(configs).unwrap();
        registry
            .validate_issuer_mapping(&[(
                setup.stocks[0].code.clone(),
                setup.stocks[0].total_shares,
            )])
            .unwrap();
        let mut session = GameSession::new(setup, 42).unwrap();
        // 单独验证诊断接缝：合法发行人表额外含一家未上市公司，不执行市场 tick。
        let template = session.state.companies.legacy().expect("此调用仅用于 Legacy 公司状态").operations.company(&crate::company::CompanyId(0)).unwrap().clone();
        let operations = std::sync::Arc::make_mut(&mut session.state.companies.legacy_mut().expect("此调用仅用于 Legacy 公司状态").operations);
        operations.catalog = registry.catalog().clone();
        operations.companies = registry.iter().map(|(id, entity)| {
            let mut operating = template.clone();
            operating.spec = entity.spec().clone();
            (*id, operating)
        }).collect();
        session.state.companies.legacy_mut().unwrap().registry = std::sync::Arc::new(registry);
        let stock = session.stock_id("600101").unwrap();
        let company = session
            .state.companies.legacy().expect("此调用仅用于 Legacy 公司状态").registry
            .catalog()
            .id("C-600101")
            .unwrap();
        assert_ne!(stock.0, company.0, "夹具必须区分股票目录与公司目录的 id");
        let quote = session.causal_quote(&stock);

        let (_, context) = session.prepare_market_calendar_tick().unwrap();
        session.state.causal.active_tick_context = Some(context);
        session.causal_submitted_with_quote(
            AccountId(0),
            OrderId(1),
            &stock,
            crate::Side::Buy,
            100,
            quote,
        ).unwrap();
        session.state.causal.active_tick_context = None;

        let origin = session
            .causal_facts()
            .iter()
            .find_map(|fact| match &fact.kind {
                CausalFactKind::Submitted(origin) => Some(origin),
                _ => None,
            })
            .expect("真实提交投影必须保留来源");
        assert_eq!(origin.code, stock);
        assert_eq!(origin.company, Some(company));
    }
    #[cfg(not(target_arch = "wasm32"))]
    mod runtime_prepare_tests {
        use super::*;

    #[test]
    fn d3_typed_origin_checks_final_plan_stock_side_and_current_root() {
        use crate::runtime::diagnostics::{DeskCausalFact, DeskCausalKind, DeskQuote, GroupDecisionTrace, OperationSource};
        let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
        let mut second = setup.stocks[0].clone();
        second.code = StockCode("600889".to_owned());
        setup.stocks.push(second);
        let mut game = GameSession::new(setup, 42).unwrap();
        let driver = crate::runtime::drive::RuntimeDriver::from_session(&mut game).unwrap();
        let account = game.state.accounts.iter().find_map(|(id, account)|
            (account.kind() == AccountKind::Inst).then_some(*id)).unwrap();
        let owner = (0..driver.accounts_per_group() as u32).map(|local| (0, local))
            .find(|owner| driver.account_by_owner(*owner) == Some(account)).unwrap();
        let mut plans = game.state.plans.clone();
        let mut create = |code| plans.create(crate::plans::PlanOpen {
            account, code, direction: crate::Side::Buy,
            target: crate::plans::PlanTarget::ShareCount(100),
            opinion: crate::plans::PlanOpinion { signal_score_bp: 8000, source: crate::plans::OpinionSource::Blended },
            confidence_bp: 8000, urgency: crate::plans::Urgency::Urgent,
            horizon_trading_days: 1, created_trading_day: 0,
        }).unwrap();
        let correct = create(StockId(0));
        let other_stock = create(StockId(1));
        // 模拟 prepared outer 的真实候选簿：新计划尚未安装到 Session。
        assert!(game.state.plans.plan(correct).is_err());
        let absent = crate::PlanId { owner: account, sequence: correct.sequence + 100 };
        let tick = 1;
        let root = GroupDecisionTrace {
            owner, kind: AccountKind::Inst,
            record: std::sync::Arc::new(crate::diagnostics::NpcDecisionTraceRecord {
                account, tick, source_report_ids: Vec::new(), expectation_method: None,
                plan_ids: vec![correct, other_stock, absent], plan_changes: Vec::new(),
                budget_constraints: Vec::new(), order_ids: Vec::new(), codes: vec![StockId(0), StockId(1)],
            }),
            causal_facts: vec![CausalFactKind::Decision { account }],
            plan_before: std::collections::BTreeMap::new(),
        };
        let submitted = |plan, side, observed_tick| DeskCausalFact {
            context: crate::runtime::retail_lifecycle::BatchContext::Tick { tick, round: 0, stock: 0 },
            kind: DeskCausalKind::Submitted {
                order: (0, 1), owner, side, qty: 100,
                source: OperationSource::InstitutionPlan { plan, observed_tick },
                before: DeskQuote { bid_cents: None, ask_cents: None, bid_depth: 0, ask_depth: 0 },
            },
        };
        let translate = |fact, roots: &[GroupDecisionTrace]| game.translate_runtime_causal(
            &driver, tick, &[fact], roots, &plans, None,
        ).map(|(kinds, _)| kinds);
        let kinds = translate(submitted(correct, crate::Side::Buy, tick), std::slice::from_ref(&root)).unwrap();
        assert!(matches!(&kinds[2], CausalFactKind::Submitted(origin)
            if origin.plan == Some(correct) && origin.decision == Some(0)));
        for (fact, roots, expected) in [
            (submitted(other_stock, crate::Side::Buy, tick), vec![root.clone()], "证券或方向错配"),
            (submitted(correct, crate::Side::Sell, tick), vec![root.clone()], "证券或方向错配"),
            (submitted(absent, crate::Side::Buy, tick), vec![root.clone()], "在最终候选不存在"),
            (submitted(correct, crate::Side::Buy, tick + 1), vec![root.clone()], "观察 tick 错配"),
            (submitted(correct, crate::Side::Buy, tick), Vec::new(), "缺当次真实 Decision"),
        ] {
            let error = translate(fact, &roots).unwrap_err();
            assert!(matches!(error, StepFatal::InvariantViolation { ref location, ref description }
                if location == "runtime::diagnostics::prepare" && description.contains(expected)), "{error:?}");
        }
        assert!(game.causal_facts().is_empty(), "只读校验失败和成功均不能提前发布");
    }

    #[test]
    fn d3_observed_npc_source_requires_current_roster_kind_code_and_root_capabilities() {
        use crate::runtime::diagnostics::{DeskCausalFact, DeskCausalKind, DeskQuote, GroupDecisionTrace, OperationSource};
        let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
        setup.npcs.hot_count = 1;
        let mut second = setup.stocks[0].clone();
        second.code = StockCode("600889".to_owned());
        setup.stocks.push(second);
        let mut game = GameSession::new(setup, 42).unwrap();
        let driver = crate::runtime::drive::RuntimeDriver::from_session(&mut game).unwrap();
        let tick = 1;
        for kind in [AccountKind::Retail, AccountKind::Hot] {
            let account = game.state.accounts.iter().find_map(|(id, row)| (row.kind() == kind).then_some(*id)).unwrap();
            let owner = (0..driver.accounts_per_group() as u32).map(|local| (0, local))
                .find(|owner| driver.account_by_owner(*owner) == Some(account)).unwrap();
            let root = GroupDecisionTrace {
                owner, kind,
                record: std::sync::Arc::new(crate::diagnostics::NpcDecisionTraceRecord {
                    account, tick, source_report_ids: Vec::new(), expectation_method: None,
                    plan_ids: Vec::new(), plan_changes: Vec::new(), budget_constraints: Vec::new(),
                    order_ids: Vec::new(), codes: vec![StockId(0)],
                }),
                causal_facts: vec![CausalFactKind::Decision { account }],
                plan_before: std::collections::BTreeMap::new(),
            };
            let submitted = |source_kind, observed_tick| DeskCausalFact {
                context: crate::runtime::retail_lifecycle::BatchContext::Tick { tick, round: 0, stock: 0 },
                kind: DeskCausalKind::Submitted {
                    order: (0, 1), owner, side: Side::Buy, qty: 100,
                    source: OperationSource::ObservedNpc { kind: source_kind, observed_tick },
                    before: DeskQuote { bid_cents: None, ask_cents: None, bid_depth: 0, ask_depth: 0 },
                },
            };
            let translate = |fact, roots: &[GroupDecisionTrace]| game.translate_runtime_causal(
                &driver, tick, &[fact], roots, &game.state.plans, None,
            ).map(|(kinds, _)| kinds);
            let kinds = translate(submitted(kind, tick), std::slice::from_ref(&root)).unwrap();
            assert!(matches!(&kinds[2], CausalFactKind::Submitted(origin)
                if origin.account == account && origin.plan.is_none() && origin.decision == Some(0)));
            let other_kind = if kind == AccountKind::Retail { AccountKind::Hot } else { AccountKind::Retail };
            let mut wrong_kind = root.clone();
            wrong_kind.kind = other_kind;
            let mut wrong_code = root.clone();
            std::sync::Arc::make_mut(&mut wrong_code.record).codes = vec![StockId(1)];
            let mut budget = root.clone();
            budget.causal_facts.push(CausalFactKind::Budget { account, available_cents: 0, allocated_cents: Vec::new() });
            for (fact, roots, expected) in [
                (submitted(kind, tick + 1), vec![root.clone()], "观察 tick 或证券错配"),
                (submitted(other_kind, tick), vec![root.clone()], "kind、观察 tick 或证券错配"),
                (submitted(AccountKind::Player, tick), vec![root.clone()], "kind、观察 tick 或证券错配"),
                (submitted(AccountKind::IssuerRepurchase, tick), vec![root.clone()], "kind、观察 tick 或证券错配"),
                (submitted(kind, tick), Vec::new(), "缺当次真实 Decision"),
                (submitted(kind, tick), vec![wrong_kind], "身份、tick、首 Decision 或账户唯一性错配"),
                (submitted(kind, tick), vec![wrong_code], "观察 tick 或证券错配"),
                (submitted(kind, tick), vec![root.clone(), root.clone()], "身份、tick、首 Decision 或账户唯一性错配"),
                (submitted(kind, tick), vec![budget], "超出 kind 能力"),
            ] {
                let error = translate(fact, &roots).unwrap_err();
                assert!(matches!(error, StepFatal::InvariantViolation { ref location, ref description }
                    if location == "runtime::diagnostics::prepare" && description.contains(expected)), "{error:?}");
            }
            if kind == AccountKind::Hot {
                let mut acquisition = root.clone();
                let now = game.observation_civil_instant();
                acquisition.causal_facts.push(CausalFactKind::Acquisition {
                    account, company: crate::company::CompanyId(0), publication: 1, published: now, acquired: now,
                });
                let error = translate(submitted(kind, tick), &[acquisition]).unwrap_err();
                assert!(matches!(error, StepFatal::InvariantViolation { ref location, ref description }
                    if location == "runtime::diagnostics::prepare" && description.contains("超出 kind 能力")), "Hot 必须在能力门拒绝 Acquisition：{error:?}");
            }
        }
        assert!(game.causal_facts().is_empty(), "typed 只读正负校验均不能提前发布");
    }

    fn d2_raw_fixture() -> (GameSession, crate::runtime::drive::RuntimeDriver, Vec<crate::runtime::diagnostics::DeskCausalFact>) {
        use crate::runtime::diagnostics::{DeskCausalFact, DeskCausalKind, DeskExecutionLeg, DeskQuote, OperationSource};
        let mut game = super::super::super::runtime_diagnostics_tests::d2_game();
        let driver = crate::runtime::drive::RuntimeDriver::from_session(&mut game).unwrap();
        let owner_for = |account| (0..16).flat_map(|group| (0..16).map(move |local| (group,local)))
            .find(|owner| driver.account_by_owner(*owner) == Some(AccountId(account))).unwrap();
        let seller = owner_for(2); let buyer = owner_for(0);
        let before = DeskQuote { bid_cents: None, ask_cents: None, bid_depth:0, ask_depth:0 };
        let submitted = |order,owner,side| DeskCausalFact { context: crate::runtime::retail_lifecycle::BatchContext::Tick { tick:game.tick(), round:0, stock:1 },
            kind:DeskCausalKind::Submitted { order,owner,side,qty:200,source:OperationSource::PlayerExternal,before } };
        let facts = vec![submitted((1,7),seller,crate::Side::Sell),submitted((1,8),buyer,crate::Side::Buy),
            DeskCausalFact { context: crate::runtime::retail_lifecycle::BatchContext::Tick { tick:game.tick(),round:0,stock:1 },kind:DeskCausalKind::Execution {
                ordinal:0,seller:DeskExecutionLeg { order:(1,7), owner:seller,value_before:0 },
                buyer:DeskExecutionLeg { order:(1,8),owner:buyer,value_before:0 },qty:100,price_cents:995,side:Some(crate::Side::Buy),before,
            }}];
        (game,driver,facts)
    }

    #[test]
    fn d2_prepare_real_typed_facts_expand_legs_and_round_stock_ordinals_reset() {
        use crate::runtime::diagnostics::DeskCausalKind;
        let (game,driver,mut facts) = d2_raw_fixture();
        let mut next = facts[2]; next.context=crate::runtime::retail_lifecycle::BatchContext::Tick { tick:game.tick(),round:1,stock:1 };
        if let DeskCausalKind::Execution { seller,buyer,.. }=&mut next.kind {
            seller.value_before=99_500; buyer.value_before=99_500;
        }
        facts.push(next);
        let delta=game.prepare_runtime_causal_facts(&driver,facts.clone()).unwrap();
        assert_eq!(delta.kinds.len(),10);
        assert!(matches!(delta.kinds[4].1,CausalFactKind::Filled { order:OrderId(4294967303),value_before:0,gross:99_500,.. }));
        assert!(matches!(delta.kinds[5].1,CausalFactKind::Filled { order:OrderId(4294967304),value_before:0,gross:99_500,.. }));
        assert!(matches!(delta.kinds[7].1,CausalFactKind::Filled { value_before:99_500,.. }));
        let mut other=facts[2];other.context=crate::runtime::retail_lifecycle::BatchContext::Tick { tick:game.tick(),round:0,stock:0 };
        if let DeskCausalKind::Execution { seller,buyer,.. }=&mut other.kind { seller.order.0=0;buyer.order.0=0; }
        facts.push(other);
        game.prepare_runtime_causal_facts(&driver,facts).unwrap();
    }

    #[test]
    fn d2_prepare_rejects_repeated_source_broken_chain_domains_and_checked_overflow() {
        use crate::runtime::diagnostics::DeskCausalKind;
        let (game,driver,facts)=d2_raw_fixture();
        for mutation in 0..13 {
            let mut bad=facts.clone();
            if mutation==0 { bad.push(bad[2]); }
            else if mutation==1 {
                let mut next=bad[2];next.context=crate::runtime::retail_lifecycle::BatchContext::Tick { tick:game.tick(),round:1,stock:1 };
                if let DeskCausalKind::Execution { seller,buyer,.. }=&mut next.kind { seller.value_before=99_500;buyer.value_before=0; }
                bad.push(next);
            } else if mutation==2 { bad[2].context=crate::runtime::retail_lifecycle::BatchContext::Tick { tick:game.tick()+1,round:0,stock:1 }; }
            else if let DeskCausalKind::Execution { seller,buyer,qty,price_cents,.. }=&mut bad[2].kind {
                match mutation {
                    3=>seller.order.0=0,
                    4=>seller.order.1=u64::from(u32::MAX)+1,
                    5=>seller.owner.1=u32::MAX,
                    6=>*qty=0,
                    7=>*price_cents=0,
                    8=>*price_cents=i64::MAX,
                    9=>seller.value_before=-1,
                    10=>seller.value_before=1,
                    11=>buyer.order=seller.order,
                    _=>buyer.owner=seller.owner,
                }
            }
            assert!(game.prepare_runtime_causal_facts(&driver,bad).is_err(),"必须拒绝raw变体{mutation}");
        }
        // 旧单首次 before 可非零，但 after 的 checked 边界仍不可越过。
        let mut old=vec![facts[2]];
        if let DeskCausalKind::Execution { seller,.. }=&mut old[0].kind {seller.value_before=i64::MAX;}
        let error=game.prepare_runtime_causal_facts(&driver,old).err().unwrap();
        assert!(format!("{error:?}").contains("after 溢出"));
        if usize::BITS == 64 {
            assert!(checked_runtime_sequence(usize::MAX, 1).is_err());
        } else {
            assert_eq!(checked_runtime_sequence(usize::MAX, 1).unwrap(), u64::from(u32::MAX) + 1);
        }
    }


    #[test]
    fn d3_real_opening_and_continuous_execution_reject_opposite_aggressor_domains() {
        use crate::runtime::diagnostics::DeskCausalKind;
        for auction in [false, true] {
            let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
            setup.ticks_per_day = 8;
            setup.auction_ticks = if auction { 3 } else { 0 };
            setup.closing_auction_ticks = 0;
            setup.stocks[0].float_shares = 2_000;
            setup.float_allocation = crate::FloatAllocation::class_percentages(1.0, 0.0, 0.0, crate::WithinKindDistribution::EqualPercentage);
            setup.strategy_params.retail.arrival_rate = 0.0;
            let mut game = GameSession::new(setup, 21).unwrap();
            let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut game).unwrap();
            let upper = driver.world_public_views().iter().map(|view| view.daily_upper_limit).collect::<Vec<_>>();
            let intents = [(AccountId(1), Side::Sell), (AccountId(0), Side::Buy)].into_iter().enumerate()
                .map(|(index, (owner, side))| ReceiptBearingIntent { owner,
                    intent: Intent::PlaceLimit { code: StockId(0), side, price: LimitPrice::Fixed(Money::from_cents(1_000)), qty: 100 },
                    account_ordinal: index as u64, stock_ordinal: index as u64,
                }).collect::<Vec<_>>();
            driver.inject_player_intents(&intents, &upper).unwrap();
            if auction { game.runtime_step_with_install(&mut driver).expect("真实开盘申报窗只受理"); }
            let input = driver.prepare_driver_step(&game).unwrap();
            let (raw, drain_time, _) = driver.take_causal_facts();
            assert!(drain_time.is_none());
            let index = raw.iter().position(|fact| matches!(fact.kind, DeskCausalKind::Execution { .. })).expect("必须真有成交才可检查方向域");
            let tick = driver.world_committed_tick();
            let before = serde_json::to_value(game.causal_facts()).unwrap();
            let positive = game.translate_runtime_causal(&driver, tick, &raw, driver.pending_decision_traces(), &input.plans, None).unwrap();
            assert_eq!(positive.0.iter().filter(|fact| matches!(fact, CausalFactKind::Execution { side, .. } if side.is_none() == auction)).count(), 1);
            let mut bad = raw.clone();
            if let DeskCausalKind::Execution { side, .. } = &mut bad[index].kind { *side = if auction { Some(Side::Buy) } else { None }; }
            let error = game.translate_runtime_causal(&driver, tick, &bad, driver.pending_decision_traces(), &input.plans, None).unwrap_err();
            assert!(error.to_string().contains("主动方向与真实 Calendar auction 相位错配"), "{error}");
            assert_eq!(serde_json::to_value(game.causal_facts()).unwrap(), before, "正负只读校验均不得发布");
        }
    }


    #[test]
    fn d3_prepare_rejects_wrong_drain_domains_and_tick_after_drain() {
        use crate::runtime::{diagnostics::DeskCausalKind, retail_lifecycle::BatchContext};
        let (game, mut driver, raw) = d2_raw_fixture();
        assert!(driver.committed_calendar_tick().unwrap().is_none());
        assert!(game.prepare_runtime_drain_causal(&mut driver, &game.state.plans).unwrap().is_none(), "真实空批不构造市场时间或要求 TickArc");
        let mut end = raw[0];
        end.context = BatchContext::Drain { ended_date: game.civil_date(), final_tick: game.tick(), stock: 1 };
        if let DeskCausalKind::Submitted { order, owner, qty, .. } = end.kind { end.kind = DeskCausalKind::DayEnded { order, owner, qty }; }
        let positive = game.translate_runtime_causal(&driver, game.tick(), &[end], &[], &game.state.plans, None).unwrap();
        assert_eq!(positive.1, Some(0));
        assert!(matches!(positive.0[0], CausalFactKind::Terminated { reason: Termination::DayEnd, .. }));
        for mutation in 0..6 {
            let mut bad = vec![end];
            match mutation {
                0 => bad[0].kind = raw[0].kind,
                1 => if let DeskCausalKind::DayEnded { order, owner, qty } = end.kind {
                    bad[0].kind = DeskCausalKind::Cancelled { order, owner, qty, source: crate::runtime::diagnostics::OperationSource::PlayerExternal };
                },
                2 => bad[0].context = BatchContext::Drain { ended_date: game.civil_date().next().unwrap(), final_tick: game.tick(), stock: 1 },
                3 => bad[0].context = BatchContext::Drain { ended_date: game.civil_date(), final_tick: game.tick() + 1, stock: 1 },
                4 => bad[0].context = raw[0].context,
                _ => bad.push(raw[0]),
            }
            assert!(game.translate_runtime_causal(&driver, game.tick(), &bad, &[], &game.state.plans, None).is_err(), "必须拒绝 Drain 身份域变体{mutation}");
        }
        assert!(game.causal_facts().is_empty());
    }

    }

}
