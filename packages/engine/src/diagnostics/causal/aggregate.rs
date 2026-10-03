use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn seconds_between(end: CivilInstant, start: CivilInstant) -> i64 {
    end.date().days_since(start.date()) * 86_400 + i64::from(end.second_of_day())
        - i64::from(start.second_of_day())
}

impl CausalReport {
    pub fn from_facts(seed: u64, facts: &[CausalFact]) -> Result<Self, CausalError> {
        CausalReportBuilder::new(seed, facts).build_report()
    }
}

struct CausalReportBuilder<'a> {
    seed: u64,
    facts: &'a [CausalFact],
    orders: BTreeMap<OrderId, OrderLifecycle>,
    fills: BTreeSet<(OrderId, i64)>,
    information_delays: Vec<(u64, i64)>,
    market_volume: u64,
    unmatched_fills: BTreeMap<OrderId, (u64, i128)>,
}

impl<'a> CausalReportBuilder<'a> {
    fn new(seed: u64, facts: &'a [CausalFact]) -> Self {
        Self {
            seed,
            facts,
            orders: BTreeMap::new(),
            fills: BTreeSet::new(),
            information_delays: Vec::new(),
            market_volume: 0,
            unmatched_fills: BTreeMap::new(),
        }
    }

    fn consume_fact(&mut self, index: usize, fact: &CausalFact) -> Result<(), CausalError> {
        if fact.sequence != index as u64 {
            return Err(CausalError::Sequence(fact.sequence));
        }
        match &fact.kind {
            CausalFactKind::ObservationRestart => return Err(CausalError::RestoredObservation),
            CausalFactKind::Budget {
                available_cents,
                allocated_cents,
                ..
            } => {
                let sum = allocated_cents.iter().try_fold(0_i64, |sum, value| {
                    sum.checked_add(*value).ok_or(CausalError::Overflow)
                })?;
                if allocated_cents.iter().any(|value| *value < 0) || sum > *available_cents {
                    return Err(CausalError::Budget(fact.sequence));
                }
            }
            CausalFactKind::Submitted(origin) => self.accept_submission(fact, origin)?,
            CausalFactKind::Filled { .. } => self.accept_fill(fact)?,
            CausalFactKind::Terminated { .. } => self.accept_termination(fact)?,
            CausalFactKind::Acquisition {
                published,
                acquired,
                ..
            } => {
                let seconds = seconds_between(*acquired, *published);
                if seconds < 0 {
                    return Err(CausalError::Time(fact.sequence));
                }
                self.information_delays.push((fact.sequence, seconds));
            }
            CausalFactKind::Execution { .. } => self.reconcile_execution(fact)?,
            CausalFactKind::Quote(_) | CausalFactKind::Decision { .. } => {}
        }
        Ok(())
    }

    fn accept_submission(
        &mut self,
        fact: &CausalFact,
        origin: &OrderOrigin,
    ) -> Result<(), CausalError> {
        if let Some(decision) = origin.decision {
            if decision >= fact.sequence
                || !self.facts.iter().any(|source| {
                    source.sequence == decision
                        && matches!(source.kind, CausalFactKind::Decision { account } if account == origin.account)
                })
            {
                return Err(CausalError::OrderMismatch(origin.order));
            }
        }
        if self.orders.contains_key(&origin.order) {
            return Err(CausalError::DuplicateOrder(origin.order));
        }
        self.orders.insert(
            origin.order,
            OrderLifecycle {
                origin: origin.clone(),
                submitted_at: fact.time,
                source_sequence: fact.sequence,
                filled_qty: 0,
                canceled_qty: 0,
                aborted_qty: 0,
                open_qty: u64::from(origin.qty),
                terminal_reason: None,
                lifetime_market_minutes: None,
                lifetime_civil_seconds: None,
                censored_reason: Some("still_open_at_observation_end"),
                filled_value: 0,
            },
        );
        Ok(())
    }

    fn accept_fill(&mut self, fact: &CausalFact) -> Result<(), CausalError> {
        let CausalFactKind::Filled {
            order,
            account,
            code,
            qty,
            value_before,
            gross,
        } = &fact.kind
        else {
            unreachable!("accept_fill 仅接收 Filled fact");
        };

        let row = self
            .orders
            .get_mut(order)
            .ok_or(CausalError::OrderMismatch(*order))?;
        if row.origin.account != *account || row.origin.code != *code {
            return Err(CausalError::OrderMismatch(*order));
        }
        if *qty == 0
            || !self.fills.insert((*order, *value_before))
            || row.filled_value != *value_before
            || *gross <= 0
        {
            return Err(CausalError::FillMismatch(*order));
        }
        row.filled_value = row
            .filled_value
            .checked_add(*gross)
            .ok_or(CausalError::Overflow)?;
        row.filled_qty = row
            .filled_qty
            .checked_add(u64::from(*qty))
            .ok_or(CausalError::Overflow)?;
        row.open_qty = row
            .open_qty
            .checked_sub(u64::from(*qty))
            .ok_or(CausalError::Conservation(*order))?;
        let pending = self.unmatched_fills.entry(*order).or_insert((0, 0));
        pending.0 = pending
            .0
            .checked_add(u64::from(*qty))
            .ok_or(CausalError::Overflow)?;
        pending.1 = pending
            .1
            .checked_add(i128::from(*gross))
            .ok_or(CausalError::Overflow)?;
        row.finish_lifecycle(fact)?;
        Ok(())
    }

    fn accept_termination(&mut self, fact: &CausalFact) -> Result<(), CausalError> {
        let CausalFactKind::Terminated {
            order,
            account,
            code,
            qty,
            reason,
        } = &fact.kind
        else {
            unreachable!("accept_termination 仅接收 Terminated fact");
        };

        let row = self
            .orders
            .get_mut(order)
            .ok_or(CausalError::OrderMismatch(*order))?;
        if row.origin.account != *account || row.origin.code != *code {
            return Err(CausalError::OrderMismatch(*order));
        }
        if row.open_qty != u64::from(*qty) || row.open_qty == 0 {
            return Err(CausalError::Conservation(*order));
        }
        match reason {
            Termination::Aborted => row.aborted_qty = u64::from(*qty),
            Termination::Voluntary
            | Termination::Reprice
            | Termination::Expired
            | Termination::DayEnd
            | Termination::MarketRemainder => row.canceled_qty = u64::from(*qty),
        }
        row.open_qty = 0;
        row.terminal_reason = Some(*reason);
        row.finish_lifecycle(fact)?;
        Ok(())
    }

    fn reconcile_execution(&mut self, fact: &CausalFact) -> Result<(), CausalError> {
        let CausalFactKind::Execution {
            qty,
            maker,
            taker,
            code,
            price_cents,
            side,
            ..
        } = &fact.kind
        else {
            unreachable!("reconcile_execution 仅接收 Execution fact");
        };

        if *qty == 0 || *price_cents <= 0 || maker == taker {
            return Err(CausalError::ExecutionMismatch);
        }
        let maker_row = self
            .orders
            .get(maker)
            .ok_or(CausalError::OrderMismatch(*maker))?;
        let taker_row = self
            .orders
            .get(taker)
            .ok_or(CausalError::OrderMismatch(*taker))?;
        if maker_row.origin.side == taker_row.origin.side
            || side.is_some_and(|side| side != taker_row.origin.side)
        {
            return Err(CausalError::ExecutionMismatch);
        }
        for id in [maker, taker] {
            if self
                .orders
                .get(id)
                .is_none_or(|row| row.origin.code != *code)
            {
                return Err(CausalError::OrderMismatch(*id));
            }
            let pending = self
                .unmatched_fills
                .get_mut(id)
                .ok_or(CausalError::ExecutionMismatch)?;
            pending.0 = pending
                .0
                .checked_sub(u64::from(*qty))
                .ok_or(CausalError::ExecutionMismatch)?;
            pending.1 -= i128::from(*price_cents) * i128::from(*qty);
        }
        self.market_volume = self
            .market_volume
            .checked_add(u64::from(*qty))
            .ok_or(CausalError::Overflow)?;
        Ok(())
    }

    fn build_report(mut self) -> Result<CausalReport, CausalError> {
        let facts = self.facts;
        for (index, fact) in facts.iter().enumerate() {
            self.consume_fact(index, fact)?;
        }
        let Self {
            seed,
            orders,
            information_delays,
            market_volume,
            unmatched_fills,
            ..
        } = self;
        if unmatched_fills.values().any(|pending| *pending != (0, 0)) {
            return Err(CausalError::ExecutionMismatch);
        }
        let sum = |project: fn(&OrderLifecycle) -> u64| {
            orders.values().try_fold(0_u64, |total, row| {
                total.checked_add(project(row)).ok_or(CausalError::Overflow)
            })
        };
        let submitted_qty = sum(|row| u64::from(row.origin.qty))?;
        let filled_qty = sum(|row| row.filled_qty)?;
        let canceled_qty = sum(|row| row.canceled_qty)?;
        let aborted_qty = sum(|row| row.aborted_qty)?;
        let open_qty = sum(|row| row.open_qty)?;
        let accounted = filled_qty
            .checked_add(canceled_qty)
            .and_then(|value| value.checked_add(aborted_qty))
            .and_then(|value| value.checked_add(open_qty))
            .ok_or(CausalError::Overflow)?;
        if accounted != submitted_qty {
            return Err(CausalError::ExecutionMismatch);
        }
        if market_volume.checked_mul(2).ok_or(CausalError::Overflow)? != filled_qty {
            return Err(CausalError::ExecutionMismatch);
        }
        let (direction_persistence, impacts, recoveries) = microstructure::analyze(facts)?;
        Ok(CausalReport {
            seed,
            submitted_qty,
            filled_qty,
            canceled_qty,
            aborted_qty,
            open_qty,
            filled_submitted_ratio: (submitted_qty > 0)
                .then(|| filled_qty as f64 / submitted_qty as f64),
            ratio_absent_reason: (submitted_qty == 0).then_some("no_submissions"),
            orders: orders.into_values().collect(),
            information_delays,
            direction_persistence,
            direction_absent_reason: direction_persistence
                .is_none()
                .then_some("fewer_than_two_active_fills_per_stock"),
            impacts,
            recoveries,
            observation_end: facts.last().map(|fact| fact.time),
        })
    }
}

impl OrderLifecycle {
    fn finish_lifecycle(&mut self, fact: &CausalFact) -> Result<(), CausalError> {
        let row = self;
        if row.open_qty == 0 {
            row.lifetime_market_minutes = Some(
                fact.time
                    .market_minute
                    .checked_sub(row.submitted_at.market_minute)
                    .ok_or(CausalError::Time(fact.sequence))?,
            );
            let seconds = seconds_between(fact.time.civil, row.submitted_at.civil);
            if seconds < 0 {
                return Err(CausalError::Time(fact.sequence));
            }
            row.lifetime_civil_seconds = Some(seconds);
            row.censored_reason = None;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn time() -> FactTime {
        FactTime {
            phase: crate::TradingPhase::Continuous,
            market_minute: 1,
            civil: CivilInstant::new(crate::CivilDate::from_ymd(2030, 1, 1).unwrap(), 1).unwrap(),
        }
    }

    fn origin(order: u64, side: Side) -> OrderOrigin {
        OrderOrigin {
            order: OrderId(order),
            account: AccountId(order),
            code: StockCode("600101".to_owned()),
            company: None,
            plan: None,
            decision: None,
            side,
            qty: 100,
        }
    }

    fn facts(kinds: Vec<CausalFactKind>) -> Vec<CausalFact> {
        kinds
            .into_iter()
            .enumerate()
            .map(|(sequence, kind)| CausalFact {
                sequence: sequence as u64,
                time: time(),
                kind,
            })
            .collect()
    }

    fn bilateral_fill_facts() -> Vec<CausalFact> {
        let code = StockCode("600101".to_owned());
        let quote = Quote {
            code: code.clone(),
            bid_cents: Some(990),
            ask_cents: Some(1_010),
            bid_depth: 100,
            ask_depth: 100,
        };
        facts(vec![
            CausalFactKind::Submitted(origin(1, Side::Sell)),
            CausalFactKind::Submitted(origin(2, Side::Buy)),
            CausalFactKind::Filled {
                order: OrderId(1),
                account: AccountId(1),
                code: code.clone(),
                qty: 100,
                value_before: 0,
                gross: 100_000,
            },
            CausalFactKind::Filled {
                order: OrderId(2),
                account: AccountId(2),
                code: code.clone(),
                qty: 100,
                value_before: 0,
                gross: 100_000,
            },
            CausalFactKind::Execution {
                code,
                maker: OrderId(1),
                taker: OrderId(2),
                side: Some(Side::Buy),
                qty: 100,
                price_cents: 1_000,
                before: quote,
            },
        ])
    }

    #[test]
    fn report_builder_reconciles_bilateral_gross_and_missing_quote_reason() {
        let mut facts = bilateral_fill_facts();
        let report = CausalReport::from_facts(7, &facts).unwrap();
        assert_eq!(
            (report.submitted_qty, report.filled_qty, report.open_qty),
            (200, 200, 0)
        );
        assert_eq!(
            report.impacts[0].absent_reason,
            Some("no_post_execution_valid_quote_before_observation_end")
        );
        if let CausalFactKind::Execution { price_cents, .. } = &mut facts[4].kind {
            *price_cents += 1;
        }
        assert_eq!(
            CausalReport::from_facts(7, &facts).unwrap_err(),
            CausalError::ExecutionMismatch
        );
    }

    #[test]
    fn report_builder_rejects_duplicate_fill_and_preserves_partial_overfill_projection() {
        let mut facts = bilateral_fill_facts();
        facts.insert(3, facts[2].clone());
        for (index, fact) in facts.iter_mut().enumerate() {
            fact.sequence = index as u64;
        }
        assert_eq!(
            CausalReport::from_facts(7, &facts).unwrap_err(),
            CausalError::FillMismatch(OrderId(1))
        );
        let mut facts = bilateral_fill_facts();
        if let CausalFactKind::Filled { qty, .. } = &mut facts[2].kind {
            *qty = 101;
        }
        let mut builder = CausalReportBuilder::new(7, &facts);
        builder.consume_fact(0, &facts[0]).unwrap();
        builder.consume_fact(1, &facts[1]).unwrap();
        assert_eq!(
            builder.consume_fact(2, &facts[2]),
            Err(CausalError::Conservation(OrderId(1)))
        );
        assert_eq!(builder.orders[&OrderId(1)].filled_qty, 101);
        assert_eq!(builder.orders[&OrderId(1)].filled_value, 100_000);
        assert_eq!(builder.orders[&OrderId(1)].open_qty, 100);
    }

    #[test]
    fn report_builder_rejects_terminal_market_or_civil_time_regression() {
        for regress_market in [true, false] {
            let mut facts = facts(vec![
                CausalFactKind::Submitted(origin(1, Side::Buy)),
                CausalFactKind::Terminated {
                    order: OrderId(1),
                    account: AccountId(1),
                    code: StockCode("600101".to_owned()),
                    qty: 100,
                    reason: Termination::DayEnd,
                },
            ]);
            if regress_market {
                facts[1].time.market_minute = 0;
            } else {
                facts[1].time.civil =
                    CivilInstant::new(crate::CivilDate::from_ymd(2030, 1, 1).unwrap(), 0).unwrap();
            }
            assert_eq!(
                CausalReport::from_facts(7, &facts).unwrap_err(),
                CausalError::Time(1)
            );
        }
    }

    #[test]
    fn report_builder_keeps_sequence_and_budget_error_precedence() {
        let mut fact = CausalFact {
            sequence: 1,
            time: time(),
            kind: CausalFactKind::Budget {
                account: AccountId(1),
                available_cents: 0,
                allocated_cents: vec![1],
            },
        };
        assert_eq!(
            CausalReport::from_facts(7, &[fact.clone()]).unwrap_err(),
            CausalError::Sequence(1)
        );
        fact.sequence = 0;
        assert_eq!(
            CausalReport::from_facts(7, &[fact]).unwrap_err(),
            CausalError::Budget(0)
        );
    }

    #[test]
    fn report_builder_absence_and_restore_are_explicit() {
        let report = CausalReport::from_facts(7, &[]).unwrap();
        assert_eq!(report.filled_submitted_ratio, None);
        assert_eq!(report.ratio_absent_reason, Some("no_submissions"));
        assert_eq!(report.observation_end, None);
        let fact = CausalFact {
            sequence: 0,
            time: time(),
            kind: CausalFactKind::ObservationRestart,
        };
        assert_eq!(
            CausalReport::from_facts(7, &[fact]).unwrap_err(),
            CausalError::RestoredObservation
        );
    }
}
