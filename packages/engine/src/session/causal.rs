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
        self.prepare_runtime_causal_at(driver, time, self.tick())
    }

    pub(in crate::session) fn prepare_runtime_causal_at(
        &self,
        driver: &mut crate::runtime::drive::RuntimeDriver,
        time: FactTime,
        tick: u64,
    ) -> Result<RuntimeCausalDelta, StepFatal> {
        let raw = driver.take_causal_facts();
        match self.translate_runtime_causal(driver, tick, &raw) {
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
                    if source != OperationSource::PlayerExternal {
                        return Err(fail(format!(
                            "causal Submitted 来源尚未接通：{source:?} order={order:?}"
                        )));
                    }
                    let (order, account) = identity(order, owner)?;
                    if qty == 0 || touched.insert(order, (account, side, 0)).is_some() {
                        return Err(fail(format!("causal Submitted 数量或身份重复：{order:?}")));
                    }
                    kinds.push(CausalFactKind::Quote(quote(before)));
                    kinds.push(CausalFactKind::Submitted(OrderOrigin {
                        order,
                        account,
                        code,
                        company: self.state.issuer_of_stock(code),
                        plan: None,
                        decision: None,
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
                    if source != OperationSource::PlayerExternal {
                        return Err(fail(format!(
                            "causal Cancelled 来源尚未接通：{source:?} order={order:?}"
                        )));
                    }
                    let (order, account) = identity(order, owner)?;
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
        let kinds = self.translate_runtime_causal(driver, self.tick(), &facts)?;
        Ok(RuntimeCausalDelta { time, raw: facts, kinds })
    }

    pub(in crate::session) fn publish_runtime_causal(&mut self, delta: RuntimeCausalDelta) {
        for kind in delta.kinds {
            self.state.causal.record(delta.time, kind);
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
            .active_plan(account, code)
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
