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

pub(super) fn analyze(facts: &[CausalFact]) -> Result<Analysis, CausalError> {
    let mut directions = BTreeMap::<StockCode, Side>::new();
    let mut pairs = 0_u64;
    let mut same = 0_u64;
    let mut impacts = Vec::new();
    let mut recoveries = Vec::new();
    let mut prior_quotes = BTreeMap::<StockCode, &Quote>::new();
    for (index, fact) in facts.iter().enumerate() {
        match &fact.kind {
            CausalFactKind::Execution {
                code, side, before, ..
            } => {
                if let Some(direction) = side {
                    if let Some(previous) = directions.insert(code.clone(), *direction) {
                        pairs += 1;
                        same += u64::from(previous == *direction);
                    }
                }
                let after = facts[index + 1..].iter().find_map(|next| match &next.kind {
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
                impacts.push(ImpactSample {
                    execution_sequence: fact.sequence,
                    quote_sequence: after.map(|(seq, _)| seq),
                    signed_observational_bp: value,
                    absent_reason: reason,
                });
            }
            CausalFactKind::Quote(quote) => {
                if let Some(before) = prior_quotes.insert(quote.code.clone(), quote) {
                    let prior = before
                        .bid_depth
                        .checked_add(before.ask_depth)
                        .ok_or(CausalError::Overflow)?;
                    let current = quote
                        .bid_depth
                        .checked_add(quote.ask_depth)
                        .ok_or(CausalError::Overflow)?;
                    if current < prior {
                        let recovered = facts[index..].iter().find(|next| match &next.kind {
                            CausalFactKind::Quote(candidate)
                                if candidate.code == quote.code
                                    && candidate.midpoint().is_some() =>
                            {
                                u128::from(candidate.bid_depth) + u128::from(candidate.ask_depth)
                                    >= u128::from(prior).div_ceil(2)
                            }
                            _ => false,
                        });
                        recoveries.push(RecoverySample {
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
    }
    Ok((
        (pairs > 0).then(|| same as f64 / pairs as f64),
        impacts,
        recoveries,
    ))
}
