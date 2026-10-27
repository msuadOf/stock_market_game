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
        for fact in raw {
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
                DeskCausalKind::Quote(raw) => kinds.push(CausalFactKind::Quote(quote(raw))),
                DeskCausalKind::Unsupported { detail } => return Err(fail(detail.to_owned())),
            }
        }
        let count = u64::try_from(self.state.causal.fact_count())
            .map_err(|_| fail("causal 历史长度超出 sequence 域".to_owned()))?;
        count
            .checked_add(
                u64::try_from(kinds.len())
                    .map_err(|_| fail("causal delta 长度超出 sequence 域".to_owned()))?,
            )
            .ok_or_else(|| fail("causal sequence 溢出".to_owned()))?;
        Ok(kinds)
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
        let template = session.state.operations.company(&crate::company::CompanyId(0)).unwrap().clone();
        let operations = std::sync::Arc::make_mut(&mut session.state.operations);
        operations.catalog = registry.catalog().clone();
        operations.companies = registry.iter().map(|(id, entity)| {
            let mut operating = template.clone();
            operating.spec = entity.spec().clone();
            (*id, operating)
        }).collect();
        session.state.company_registry = std::sync::Arc::new(registry);
        let stock = session.stock_id("600101").unwrap();
        let company = session
            .state
            .company_registry
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
}
