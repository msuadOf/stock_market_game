use super::*;
use std::collections::BTreeMap;

type Analysis = (Option<f64>, Vec<ImpactSample>, Vec<RecoverySample>);

#[derive(Debug, Serialize)]
pub struct ImpactSample {
    pub execution_sequence: u64,
    pub quote_sequence: Option<u64>,
    pub signed_observational_bp: Option<f64>,
    pub absent_reason: Option<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct RecoverySample {
    pub loss_sequence: u64,
    pub recovered_sequence: Option<u64>,
    pub market_minutes: Option<u64>,
    pub censored_reason: Option<&'static str>,
}

struct DirectionPersistenceAccumulator<'a> {
    facts: &'a [CausalFact],
    directions: BTreeMap<StockCode, Side>,
    pairs: u64,
    same: u64,
}

impl<'a> DirectionPersistenceAccumulator<'a> {
    fn new(facts: &'a [CausalFact]) -> Self {
        Self {
            facts,
            directions: BTreeMap::new(),
            pairs: 0,
            same: 0,
        }
    }

    fn consume(&mut self, index: usize) {
        if let CausalFactKind::Execution {
            code,
            side: Some(direction),
            ..
        } = &self.facts[index].kind
        {
            if let Some(previous) = self.directions.insert(code.clone(), *direction) {
                self.pairs += 1;
                self.same += u64::from(previous == *direction);
            }
        }
    }

    fn finish(self) -> Option<f64> {
        (self.pairs > 0).then(|| self.same as f64 / self.pairs as f64)
    }
}

struct QuoteResponseAccumulator<'a> {
    facts: &'a [CausalFact],
    prior_quotes: BTreeMap<StockCode, &'a Quote>,
    impacts: Vec<ImpactSample>,
    recoveries: Vec<RecoverySample>,
}

impl<'a> QuoteResponseAccumulator<'a> {
    fn new(facts: &'a [CausalFact]) -> Self {
        Self {
            facts,
            prior_quotes: BTreeMap::new(),
            impacts: Vec::new(),
            recoveries: Vec::new(),
        }
    }

    fn consume(&mut self, index: usize) -> Result<(), CausalError> {
        let fact = &self.facts[index];
        match &fact.kind {
            CausalFactKind::Execution {
                code, side, before, ..
            } => {
                let after = self.facts[index + 1..]
                    .iter()
                    .find_map(|next| match &next.kind {
                        CausalFactKind::Quote(quote) if quote.code == *code => {
                            quote.midpoint().map(|mid| (next.sequence, mid))
                        }
                        _ => None,
                    });
                let (value, reason) = match (side, before.midpoint(), after) {
                    (Some(direction), Some(pre), Some((_, post))) => {
                        let sign = match direction {
                            Side::Buy => 1.0,
                            Side::Sell => -1.0,
                        };
                        (Some(sign * (post - pre) / pre * 10_000.0), None)
                    }
                    (None, _, _) => (None, Some("auction_has_no_aggressor")),
                    (_, None, _) => (None, Some("missing_pre_execution_two_sided_quote")),
                    (_, _, None) => (
                        None,
                        Some("no_post_execution_valid_quote_before_observation_end"),
                    ),
                };
                self.impacts.push(ImpactSample {
                    execution_sequence: fact.sequence,
                    quote_sequence: after.map(|(seq, _)| seq),
                    signed_observational_bp: value,
                    absent_reason: reason,
                });
            }
            CausalFactKind::Quote(quote) => {
                if let Some(before) = self.prior_quotes.insert(quote.code.clone(), quote) {
                    let prior = before
                        .bid_depth
                        .checked_add(before.ask_depth)
                        .ok_or(CausalError::Overflow)?;
                    let current = quote
                        .bid_depth
                        .checked_add(quote.ask_depth)
                        .ok_or(CausalError::Overflow)?;
                    if current < prior {
                        let recovered = self.facts[index..].iter().find(|next| match &next.kind {
                            CausalFactKind::Quote(candidate)
                                if candidate.code == quote.code
                                    && candidate.midpoint().is_some() =>
                            {
                                u128::from(candidate.bid_depth) + u128::from(candidate.ask_depth)
                                    >= u128::from(prior).div_ceil(2)
                            }
                            _ => false,
                        });
                        self.recoveries.push(RecoverySample {
                            loss_sequence: fact.sequence,
                            recovered_sequence: recovered.map(|next| next.sequence),
                            market_minutes: recovered
                                .map(|next| {
                                    next.time
                                        .market_minute
                                        .checked_sub(fact.time.market_minute)
                                        .ok_or(CausalError::Time(next.sequence))
                                })
                                .transpose()?,
                            censored_reason: recovered
                                .is_none()
                                .then_some("not_recovered_before_observation_end"),
                        });
                    }
                }
            }
            CausalFactKind::Submitted(_)
            | CausalFactKind::Filled { .. }
            | CausalFactKind::Terminated { .. }
            | CausalFactKind::Acquisition { .. }
            | CausalFactKind::Decision { .. }
            | CausalFactKind::Budget { .. }
            | CausalFactKind::ObservationRestart => {}
        }
        Ok(())
    }

    fn finish(self) -> (Vec<ImpactSample>, Vec<RecoverySample>) {
        (self.impacts, self.recoveries)
    }
}

pub(super) fn analyze(facts: &[CausalFact]) -> Result<Analysis, CausalError> {
    let mut directions = DirectionPersistenceAccumulator::new(facts);
    let mut quotes = QuoteResponseAccumulator::new(facts);
    for index in 0..facts.len() {
        directions.consume(index);
        quotes.consume(index)?;
    }
    let (impacts, recoveries) = quotes.finish();
    Ok((directions.finish(), impacts, recoveries))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quote(bid: Option<i64>, ask: Option<i64>, depth: u64) -> Quote {
        Quote {
            code: StockCode("600101".to_owned()),
            bid_cents: bid,
            ask_cents: ask,
            bid_depth: depth,
            ask_depth: depth,
        }
    }

    #[test]
    fn direction_persistence_keeps_each_stock_chain_and_ignores_auction_direction() {
        let kinds = [
            ("600101", Some(Side::Buy)),
            ("000001", Some(Side::Sell)),
            ("600101", None),
            ("600101", Some(Side::Buy)),
            ("600101", Some(Side::Sell)),
        ];
        let facts: Vec<_> = kinds
            .into_iter()
            .enumerate()
            .map(|(sequence, (code, side))| CausalFact {
                sequence: sequence as u64,
                time: FactTime {
                    phase: crate::TradingPhase::Continuous,
                    market_minute: sequence as u64,
                    civil: CivilInstant::new(
                        crate::CivilDate::from_ymd(2030, 1, 1).unwrap(),
                        sequence as u32,
                    )
                    .unwrap(),
                },
                kind: CausalFactKind::Execution {
                    code: StockCode(code.to_owned()),
                    maker: OrderId(1),
                    taker: OrderId(2),
                    side,
                    qty: 100,
                    price_cents: 1_000,
                    before: quote(Some(990), Some(1_010), 100),
                },
            })
            .collect();
        let (direction, impacts, recoveries) = analyze(&facts).unwrap();
        assert_eq!(direction, Some(0.5));
        assert_eq!(impacts[2].absent_reason, Some("auction_has_no_aggressor"));
        assert!(recoveries.is_empty());
        assert_eq!(analyze(&[]).unwrap().0, None);
    }

    #[test]
    fn quote_response_uses_first_later_valid_quote_and_includes_current_recovery() {
        let kinds = [
            CausalFactKind::Execution {
                code: StockCode("600101".to_owned()),
                maker: OrderId(1),
                taker: OrderId(2),
                side: Some(Side::Buy),
                qty: 100,
                price_cents: 1_000,
                before: quote(Some(990), Some(1_010), 100),
            },
            CausalFactKind::Quote(quote(None, Some(1_030), 100)),
            CausalFactKind::Quote(quote(Some(1_010), Some(1_030), 60)),
            CausalFactKind::Quote(quote(Some(1_020), Some(1_040), 100)),
        ];
        let facts: Vec<_> = kinds
            .into_iter()
            .enumerate()
            .map(|(sequence, kind)| CausalFact {
                sequence: sequence as u64,
                time: FactTime {
                    phase: crate::TradingPhase::Continuous,
                    market_minute: sequence as u64,
                    civil: CivilInstant::new(
                        crate::CivilDate::from_ymd(2030, 1, 1).unwrap(),
                        sequence as u32,
                    )
                    .unwrap(),
                },
                kind,
            })
            .collect();
        let (direction, impacts, recoveries) = analyze(&facts).unwrap();
        assert_eq!(direction, None);
        assert_eq!(impacts[0].quote_sequence, Some(2));
        assert_eq!(impacts[0].signed_observational_bp, Some(200.0));
        assert_eq!(recoveries[0].loss_sequence, 2);
        assert_eq!(recoveries[0].recovered_sequence, Some(2));
        assert_eq!(recoveries[0].market_minutes, Some(0));
    }
}
