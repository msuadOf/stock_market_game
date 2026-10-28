use super::*;
use crate::diagnostics::causal::{
    CausalError, CausalFact, CausalFactKind, CausalReport, FactTime, OrderOrigin, Quote,
    Termination,
};

/// 仅含已校验的本拍增量，最外层全部成功后才可发布。
pub(in crate::session) struct RuntimeCausalDelta {
    pub time: FactTime,
    pub raw: Vec<crate::runtime::diagnostics::DeskCausalFact>,
    kinds: Vec<CausalFactKind>,
}

impl GameSession {
    pub(in crate::session) fn prepare_runtime_causal(
        &self,
        driver: &mut crate::runtime::drive::RuntimeDriver,
        time: FactTime,
    ) -> Result<RuntimeCausalDelta, StepFatal> {
        self.prepare_runtime_causal_at(driver, time, self.tick(), &self.state.plans)
    }

    pub(in crate::session) fn prepare_runtime_causal_at(
        &self,
        driver: &mut crate::runtime::drive::RuntimeDriver,
        time: FactTime,
        tick: u64,
        plans: &crate::plans::PlanBook,
    ) -> Result<RuntimeCausalDelta, StepFatal> {
        let raw = driver.take_causal_facts();
        match self.translate_runtime_causal(driver, tick, &raw, driver.pending_decision_traces(), plans) {
            Ok(kinds) => Ok(RuntimeCausalDelta { time, raw, kinds }),
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
    ) -> Result<Vec<CausalFactKind>, StepFatal> {
        use crate::runtime::diagnostics::{DeskCausalKind, OperationSource};
        let fail = |description: String| StepFatal::InvariantViolation {
            location: "runtime::diagnostics::prepare".to_owned(),
            description,
        };
        if tick.is_multiple_of(self.state.setup.ticks_per_day) {
            driver.ensure_causal_day_end_supported().map_err(fail)?;
        }
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
        for fact in raw.iter().copied() {
            if fact.tick != tick {
                return Err(fail(format!(
                    "causal tick 错位：fact={} session={}",
                    fact.tick,
                    tick
                )));
            }
            let code = StockId(u32::from(fact.stock));
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
                if order.0 != fact.stock || order.1 > u64::from(u32::MAX) {
                    return Err(fail(format!(
                        "causal OrderRef 无效：stock={} order={order:?}",
                        fact.stock
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
                    if !sources.insert((fact.tick, fact.round, fact.stock, ordinal)) {
                        return Err(fail(format!("causal Execution source 重复：{fact:?}")));
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
                        code, maker: sell_order, taker: buy_order, side: Some(side), qty,
                        price_cents, before: quote(before),
                    });
                }
                DeskCausalKind::Quote(raw) => kinds.push(CausalFactKind::Quote(quote(raw))),
                DeskCausalKind::Unsupported { detail } => return Err(fail(detail.to_owned())),
            }
        }
        checked_runtime_sequence(self.state.causal.fact_count(), kinds.len()).map_err(fail)?;
        Ok(kinds)
    }

    // 保留直接纯校验测试接缝；真实入口按 E2 捕获原 raw 与 terminal 首错。
    fn prepare_runtime_causal_facts(
        &self,
        driver: &crate::runtime::drive::RuntimeDriver,
        time: FactTime,
        facts: Vec<crate::runtime::diagnostics::DeskCausalFact>,
    ) -> Result<RuntimeCausalDelta, StepFatal> {
        let kinds = self.translate_runtime_causal(driver, self.tick(), &facts, driver.pending_decision_traces(), &self.state.plans)?;
        Ok(RuntimeCausalDelta { time, raw: facts, kinds })
    }

    pub(in crate::session) fn publish_runtime_causal(&mut self, delta: RuntimeCausalDelta) {
        for kind in delta.kinds {
            if let CausalFactKind::Decision { account } = kind {
                self.state.causal.record_plan_root(delta.time, account, [CausalFactKind::Decision { account }]);
            } else {
                self.state.causal.record(delta.time, kind);
            }
        }
    }

    pub fn causal_facts(&self) -> &[CausalFact] {
        self.state.causal.facts()
    }

    pub fn causal_diagnostics(&self) -> Result<CausalReport, CausalError> {
        CausalReport::from_facts(self.state.seed, self.state.causal.facts())
    }

    pub(super) fn causal_time(&self) -> FactTime {
        let day_tick = self.state.tick - u64::from(self.state.day) * self.state.setup.ticks_per_day;
        let continuous = self.state.setup.ticks_per_day
            - self.state.setup.auction_ticks
            - self.state.setup.closing_auction_ticks;
        let elapsed = day_tick
            .saturating_sub(self.state.setup.auction_ticks)
            .min(continuous);
        let market_minute = u64::from(self.state.day) * 240 + elapsed * 240 / continuous;
        FactTime {
            phase: self.phase(),
            market_minute,
            civil: self.observation_civil_instant(),
        }
    }

    pub(super) fn causal_record(&mut self, kind: CausalFactKind) {
        self.causal_record_at(self.causal_time(), kind);
    }

    pub(super) fn causal_record_at(&mut self, time: FactTime, kind: CausalFactKind) {
        self.state.causal.record(time, kind);
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

    pub(super) fn causal_snapshot(&mut self, code: &StockId) {
        self.causal_snapshot_at(self.causal_time(), code);
    }

    pub(super) fn causal_snapshot_at(&mut self, time: FactTime, code: &StockId) {
        self.causal_record_at(time, CausalFactKind::Quote(self.causal_quote(code)));
    }

    #[cfg(test)]
    pub(super) fn causal_submitted(&mut self, order: &Order, code: &StockId) {
        let quote = self.causal_quote(code);
        self.causal_submitted_with_quote(order.owner, order.id, code, order.side, order.qty, quote);
    }

    pub(super) fn causal_submitted_with_quote(
        &mut self,
        account: AccountId,
        order: OrderId,
        code: &StockId,
        side: crate::Side,
        qty: u32,
        quote: Quote,
    ) {
        self.causal_record(CausalFactKind::Quote(quote));
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
        }));
    }

    pub(super) fn causal_terminated(
        &mut self,
        order: (AccountId, OrderId, u32),
        code: &StockId,
        reason: Termination,
    ) {
        self.causal_terminated_at(self.causal_time(), order, code, reason);
    }

    pub(super) fn causal_terminated_at(
        &mut self,
        time: FactTime,
        order: (AccountId, OrderId, u32),
        code: &StockId,
        reason: Termination,
    ) {
        self.causal_record_at(
            time,
            CausalFactKind::Terminated {
                account: order.0,
                order: order.1,
                qty: order.2,
                code: code.clone(),
                reason,
            },
        );
    }
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

        session.causal_submitted_with_quote(
            AccountId(0),
            OrderId(1),
            &stock,
            crate::Side::Buy,
            100,
            quote,
        );

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
            tick, round: 0, stock: 0,
            kind: DeskCausalKind::Submitted {
                order: (0, 1), owner, side, qty: 100,
                source: OperationSource::InstitutionPlan { plan, observed_tick },
                before: DeskQuote { bid_cents: None, ask_cents: None, bid_depth: 0, ask_depth: 0 },
            },
        };
        let translate = |fact, roots: &[GroupDecisionTrace]| game.translate_runtime_causal(
            &driver, tick, &[fact], roots, &plans,
        );
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
                tick, round: 0, stock: 0,
                kind: DeskCausalKind::Submitted {
                    order: (0, 1), owner, side: Side::Buy, qty: 100,
                    source: OperationSource::ObservedNpc { kind: source_kind, observed_tick },
                    before: DeskQuote { bid_cents: None, ask_cents: None, bid_depth: 0, ask_depth: 0 },
                },
            };
            let translate = |fact, roots: &[GroupDecisionTrace]| game.translate_runtime_causal(
                &driver, tick, &[fact], roots, &game.state.plans,
            );
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
        let submitted = |order,owner,side| DeskCausalFact { tick:game.tick(), round:0, stock:1,
            kind:DeskCausalKind::Submitted { order,owner,side,qty:200,source:OperationSource::PlayerExternal,before } };
        let facts = vec![submitted((1,7),seller,crate::Side::Sell),submitted((1,8),buyer,crate::Side::Buy),
            DeskCausalFact { tick:game.tick(),round:0,stock:1,kind:DeskCausalKind::Execution {
                ordinal:0,seller:DeskExecutionLeg { order:(1,7), owner:seller,value_before:0 },
                buyer:DeskExecutionLeg { order:(1,8),owner:buyer,value_before:0 },qty:100,price_cents:995,side:crate::Side::Buy,before,
            }}];
        (game,driver,facts)
    }

    #[test]
    fn d2_prepare_real_typed_facts_expand_legs_and_round_stock_ordinals_reset() {
        use crate::runtime::diagnostics::DeskCausalKind;
        let (game,driver,mut facts) = d2_raw_fixture();
        let mut next = facts[2]; next.round=1;
        if let DeskCausalKind::Execution { seller,buyer,.. }=&mut next.kind {
            seller.value_before=99_500; buyer.value_before=99_500;
        }
        facts.push(next);
        let delta=game.prepare_runtime_causal_facts(&driver,game.causal_time(),facts.clone()).unwrap();
        assert_eq!(delta.kinds.len(),10);
        assert!(matches!(delta.kinds[4],CausalFactKind::Filled { order:OrderId(4294967303),value_before:0,gross:99_500,.. }));
        assert!(matches!(delta.kinds[5],CausalFactKind::Filled { order:OrderId(4294967304),value_before:0,gross:99_500,.. }));
        assert!(matches!(delta.kinds[7],CausalFactKind::Filled { value_before:99_500,.. }));
        let mut other=facts[2];other.stock=0;
        if let DeskCausalKind::Execution { seller,buyer,.. }=&mut other.kind { seller.order.0=0;buyer.order.0=0; }
        facts.push(other);
        game.prepare_runtime_causal_facts(&driver,game.causal_time(),facts).unwrap();
    }

    #[test]
    fn d2_prepare_rejects_repeated_source_broken_chain_domains_and_checked_overflow() {
        use crate::runtime::diagnostics::DeskCausalKind;
        let (game,driver,facts)=d2_raw_fixture();
        for mutation in 0..13 {
            let mut bad=facts.clone();
            if mutation==0 { bad.push(bad[2]); }
            else if mutation==1 {
                let mut next=bad[2];next.round=1;
                if let DeskCausalKind::Execution { seller,buyer,.. }=&mut next.kind { seller.value_before=99_500;buyer.value_before=0; }
                bad.push(next);
            } else if mutation==2 { bad[2].tick=game.tick()+1; }
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
            assert!(game.prepare_runtime_causal_facts(&driver,game.causal_time(),bad).is_err(),"必须拒绝raw变体{mutation}");
        }
        // 旧单首次 before 可非零，但 after 的 checked 边界仍不可越过。
        let mut old=vec![facts[2]];
        if let DeskCausalKind::Execution { seller,.. }=&mut old[0].kind {seller.value_before=i64::MAX;}
        let error=game.prepare_runtime_causal_facts(&driver,game.causal_time(),old).err().unwrap();
        assert!(format!("{error:?}").contains("after 溢出"));
        if usize::BITS == 64 {
            assert!(checked_runtime_sequence(usize::MAX, 1).is_err());
        } else {
            assert_eq!(checked_runtime_sequence(usize::MAX, 1).unwrap(), u64::from(u32::MAX) + 1);
        }
    }

    }

}
