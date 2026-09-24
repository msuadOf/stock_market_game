//! Production P2 input capture on the discardable tick shadow.
//!
//! This seam advances only the attention/retail-observation state needed
//! to seal strategy inputs. It never routes an intent or mutates an order book.

use super::{DecisionAccountInput, DecisionSnapshot, DecisionSnapshotError};
use crate::behavior::BehaviorMarketObservation;
use crate::observation::{
    build_account_risk_observation, build_equal_weight_market_observation, AccountRiskObservation,
    ObservationError, RiskPositionInput,
};
use crate::strategy::{PositionView, SelfView, StrategyStateError};
use crate::{
    AccountId, AccountKind, ExperienceError, Money, MoneyError, NpcAttentionState,
    RetailExperienceState, StockCode,
};
use rayon::prelude::*;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub(in crate::session) enum DecisionSnapshotCaptureError {
    #[error("P2 decision snapshot accepted missing account {0:?}")]
    MissingAccount(AccountId),
    #[error("P2 decision snapshot accepted player account {0:?} as an NPC")]
    PlayerAccount(AccountId),
    #[error("P2 decision snapshot account {0:?} has no attention state")]
    MissingAttention(AccountId),
    #[error(
        "P2 decision snapshot account {account:?} has invalid attention probability {probability}"
    )]
    InvalidAttentionProbability {
        account: AccountId,
        probability: f64,
    },
    #[error("P2 decision snapshot account {0:?} has no production strategy")]
    MissingStrategy(AccountId),
    #[error("P2 decision snapshot cannot export strategy for account {account:?}: {source}")]
    StrategyState {
        account: AccountId,
        #[source]
        source: StrategyStateError,
    },
    #[error("P2 retail experience observation failed for account {account:?}: {source}")]
    Experience {
        account: AccountId,
        #[source]
        source: ExperienceError,
    },
    #[error("P2 decision snapshot {location} is missing market data for {code:?}")]
    MissingMarketData {
        location: &'static str,
        code: StockCode,
    },
    #[error("P2 decision snapshot {location} money failure for account {account:?}: {source}")]
    Money {
        location: &'static str,
        account: AccountId,
        #[source]
        source: MoneyError,
    },
    #[error(
        "P2 decision snapshot {location} observation failure for account {account:?}: {source}"
    )]
    Observation {
        location: &'static str,
        account: Option<AccountId>,
        #[source]
        source: ObservationError,
    },
    #[error("P2 decision snapshot contract rejected captured input: {0}")]
    Snapshot(#[source] DecisionSnapshotError),
}

/// Advances due-attention and pre-decision retail observations on `shadow`, then
/// returns the immutable input consumed by the pure NPC P2 source.
///
/// Only due attention and accepted retail observations can change here. Record
/// those entries and the popped queue items, so a healthy tick never copies
/// every sleeping NPC or its accumulated experience.
pub(in crate::session) fn capture_decision_snapshot(
    shadow: &mut super::GameSession,
) -> Result<Arc<DecisionSnapshot>, DecisionSnapshotCaptureError> {
    let mut rollback = CaptureRollback::default();
    let result = capture_decision_snapshot_in_place(shadow, &mut rollback);
    if result.is_err() {
        rollback.restore(shadow);
    }
    result
}

#[derive(Default)]
struct CaptureRollback {
    popped: Vec<(u64, AccountId)>,
    scheduled: Vec<(u64, AccountId)>,
    attention: BTreeMap<AccountId, NpcAttentionState>,
}

impl CaptureRollback {
    fn restore(self, shadow: &mut super::GameSession) {
        shadow
            .attention_queue
            .extend(self.popped.into_iter().map(Reverse));
        for (account, state) in self.attention {
            shadow.npc_attention.insert(account, state);
        }
    }
}

fn capture_decision_snapshot_in_place(
    shadow: &mut super::GameSession,
    rollback: &mut CaptureRollback,
) -> Result<Arc<DecisionSnapshot>, DecisionSnapshotCaptureError> {
    let tick = shadow.tick;
    let phase = shadow.phase();
    validate_market_view_inputs(shadow)?;
    let market = shadow.build_market_view();

    let mut accepted_due_npc_ids = Vec::new();
    let (due_npc_ids, popped) = shadow.pop_due_npc_ids(tick);
    rollback.popped = popped;
    if let Some(account) = rollback
        .popped
        .iter()
        .map(|(_, account)| *account)
        .find(|account| !shadow.npc_attention.contains_key(account))
    {
        return Err(DecisionSnapshotCaptureError::MissingAttention(account));
    }
    for account in due_npc_ids {
        let kind = shadow
            .accounts
            .get(&account)
            .map(|entry| entry.kind)
            .ok_or(DecisionSnapshotCaptureError::MissingAccount(account))?;
        if kind == AccountKind::Player {
            return Err(DecisionSnapshotCaptureError::PlayerAccount(account));
        }
        let attention = shadow
            .npc_attention
            .get(&account)
            .ok_or(DecisionSnapshotCaptureError::MissingAttention(account))?;
        if !attention.base_probability.is_finite()
            || attention.base_probability <= 0.0
            || attention.base_probability > 1.0
        {
            return Err(DecisionSnapshotCaptureError::InvalidAttentionProbability {
                account,
                probability: attention.base_probability,
            });
        }
        rollback.attention.insert(account, attention.clone());
        let observes = shadow.evaluate_attention_candidate(account, &market);
        let scheduled = shadow.npc_attention[&account].next_attention_candidate_tick;
        rollback.scheduled.push((scheduled, account));
        if observes {
            accepted_due_npc_ids.push(account);
        }
    }

    let market_minute = shadow.current_market_minute();
    let (working_continuous, working_auction) = if accepted_due_npc_ids.is_empty() {
        (BTreeMap::new(), BTreeMap::new())
    } else {
        shadow.working_orders_by_account()
    };
    let has_retail_observer = accepted_due_npc_ids.iter().any(|account| {
        shadow
            .accounts
            .get(account)
            .is_some_and(|entry| entry.kind == AccountKind::Retail)
    });
    let behavior_market = has_retail_observer
        .then(|| build_behavior_market_checked(shadow))
        .transpose()?;
    let prepared = accepted_due_npc_ids
        .par_iter()
        .map(|account| {
            let entry = shadow
                .accounts
                .get(account)
                .ok_or(DecisionSnapshotCaptureError::MissingAccount(*account))?;
            let experience = observe_retail_experience_for(shadow, *account, market_minute)?;
            let self_view = build_self_view_for(
                shadow,
                *account,
                phase,
                &working_continuous,
                &working_auction,
            )?;
            let account_risk = if entry.kind == AccountKind::Retail {
                Some(build_account_risk_for(
                    shadow,
                    *account,
                    experience
                        .as_ref()
                        .ok_or(DecisionSnapshotCaptureError::MissingAccount(*account))?,
                )?)
            } else {
                None
            };
            let strategy_state = entry
                .strategy
                .as_ref()
                .ok_or(DecisionSnapshotCaptureError::MissingStrategy(*account))?
                .production_state()
                .map_err(|source| DecisionSnapshotCaptureError::StrategyState {
                    account: *account,
                    source,
                })?;
            let input = DecisionAccountInput::new(
                entry.kind,
                self_view,
                strategy_state,
                account_risk,
                experience.clone(),
            );
            Ok((*account, input, experience))
        })
        .collect::<Vec<Result<_, DecisionSnapshotCaptureError>>>()
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    let mut accounts = BTreeMap::new();
    let mut experience_updates = Vec::new();
    for (account, input, experience) in prepared {
        accounts.insert(account, input);
        if let Some(experience) = experience {
            experience_updates.push((account, experience));
        }
    }

    let snapshot = DecisionSnapshot::new(
        tick,
        shadow.seed,
        phase,
        market_minute,
        market,
        behavior_market,
        accepted_due_npc_ids,
        accounts,
    )
    .map_err(DecisionSnapshotCaptureError::Snapshot)?;

    for (account, experience) in experience_updates {
        shadow.retail_experience.insert(account, experience);
    }
    shadow
        .attention_queue
        .extend(rollback.scheduled.iter().copied().map(Reverse));
    Ok(Arc::new(snapshot))
}

fn validate_market_view_inputs(
    session: &super::GameSession,
) -> Result<(), DecisionSnapshotCaptureError> {
    for code in session.markets.keys() {
        for (location, present) in [
            (
                "market price history",
                session.price_history.contains_key(code),
            ),
            (
                "market-minute history",
                session.market_minute_closes.contains_key(code),
            ),
            ("daily history", session.daily_candles.contains_key(code)),
        ] {
            if !present {
                return Err(DecisionSnapshotCaptureError::MissingMarketData {
                    location,
                    code: code.clone(),
                });
            }
        }
    }
    Ok(())
}

fn observe_retail_experience_for(
    session: &super::GameSession,
    account: AccountId,
    market_minute: u64,
) -> Result<Option<RetailExperienceState>, DecisionSnapshotCaptureError> {
    let Some(mut experience) = session.retail_experience.get(&account).cloned() else {
        return Ok(None);
    };
    let entry = session
        .accounts
        .get(&account)
        .ok_or(DecisionSnapshotCaptureError::MissingAccount(account))?;
    let mut equity = entry.cash;
    let mut positions = Vec::new();
    for (code, position) in &entry.positions {
        let price = session
            .markets
            .get(code)
            .ok_or_else(|| DecisionSnapshotCaptureError::MissingMarketData {
                location: "retail experience position",
                code: code.clone(),
            })?
            .last_price();
        equity = equity
            .add(price.mul_shares(position.qty).map_err(|source| {
                DecisionSnapshotCaptureError::Money {
                    location: "retail experience equity",
                    account,
                    source,
                }
            })?)
            .map_err(|source| DecisionSnapshotCaptureError::Money {
                location: "retail experience equity",
                account,
                source,
            })?;
        positions.push((code.clone(), price));
    }
    if equity.cents() > 0 {
        experience
            .observe_equity(equity)
            .map_err(|source| DecisionSnapshotCaptureError::Experience { account, source })?;
    }
    let held: BTreeSet<_> = positions.iter().map(|(code, _)| code.clone()).collect();
    for (code, price) in positions {
        experience
            .observe_position(&code, price, market_minute)
            .map_err(|source| DecisionSnapshotCaptureError::Experience { account, source })?;
    }
    experience.prune_watchlist(&held);
    Ok(Some(experience))
}

fn build_self_view_for(
    session: &super::GameSession,
    account: AccountId,
    phase: crate::TradingPhase,
    continuous: &super::super::ContinuousOrdersByAccount,
    auction: &super::super::AuctionOrdersByAccount,
) -> Result<SelfView, DecisionSnapshotCaptureError> {
    let auction_cancelable = phase == crate::TradingPhase::CallAuction
        && session.tick % session.setup.ticks_per_day < session.setup.auction_ticks / 3;
    let entry = session
        .accounts
        .get(&account)
        .ok_or(DecisionSnapshotCaptureError::MissingAccount(account))?;
    let mut reserved = Money::ZERO;
    let mut replaceable = Money::ZERO;
    let mut record = |required: Money, may_replace: bool| -> Result<(), MoneyError> {
        reserved = reserved.add(required)?;
        if may_replace {
            replaceable = replaceable.add(required)?;
        }
        Ok(())
    };
    for (_, order) in continuous.get(&account).into_iter().flatten() {
        record(
            super::super::live_cash_reservation(
                &session.setup.config,
                order.side,
                order.price,
                order.qty,
                order.filled_value,
            )
            .map_err(|source| DecisionSnapshotCaptureError::Money {
                location: "continuous self-view reservation",
                account,
                source,
            })?,
            phase == crate::TradingPhase::Continuous,
        )
        .map_err(|source| DecisionSnapshotCaptureError::Money {
            location: "continuous self-view reservation sum",
            account,
            source,
        })?;
    }
    for (_, order) in auction.get(&account).into_iter().flatten() {
        record(
            super::super::live_cash_reservation(
                &session.setup.config,
                order.side,
                order.limit,
                order.qty,
                Money::ZERO,
            )
            .map_err(|source| DecisionSnapshotCaptureError::Money {
                location: "auction self-view reservation",
                account,
                source,
            })?,
            auction_cancelable,
        )
        .map_err(|source| DecisionSnapshotCaptureError::Money {
            location: "auction self-view reservation sum",
            account,
            source,
        })?;
    }
    let cash = entry
        .cash
        .sub(reserved)
        .and_then(|cash| cash.add(replaceable))
        .map_err(|source| DecisionSnapshotCaptureError::Money {
            location: "available self-view cash",
            account,
            source,
        })?;
    let positions = entry
        .positions
        .iter()
        .map(|(code, position)| {
            (
                code.clone(),
                PositionView {
                    qty: position.qty,
                    sellable_qty: position.sellable(),
                    cost_price: position.cost_price(),
                },
            )
        })
        .collect();
    Ok(SelfView { cash, positions })
}

fn build_behavior_market_checked(
    session: &super::GameSession,
) -> Result<BehaviorMarketObservation, DecisionSnapshotCaptureError> {
    let price_paths = session.market_price_path_observations().map_err(|source| {
        DecisionSnapshotCaptureError::Observation {
            location: "behavior price paths",
            account: None,
            source,
        }
    })?;
    let returns = price_paths
        .iter()
        .map(|(code, path)| (code.clone(), path.thirty_minute.return_ratio))
        .collect();
    let thirty_minute_market =
        build_equal_weight_market_observation(&returns).map_err(|source| {
            DecisionSnapshotCaptureError::Observation {
                location: "behavior equal-weight market",
                account: None,
                source,
            }
        })?;
    Ok(BehaviorMarketObservation {
        price_paths,
        thirty_minute_market,
    })
}

fn build_account_risk_for(
    session: &super::GameSession,
    account: AccountId,
    experience: &RetailExperienceState,
) -> Result<AccountRiskObservation, DecisionSnapshotCaptureError> {
    let entry = session
        .accounts
        .get(&account)
        .ok_or(DecisionSnapshotCaptureError::MissingAccount(account))?;
    let mut positions = BTreeMap::new();
    for (code, position) in &entry.positions {
        let last_price = session
            .markets
            .get(code)
            .ok_or_else(|| DecisionSnapshotCaptureError::MissingMarketData {
                location: "account risk position",
                code: code.clone(),
            })?
            .last_price();
        positions.insert(
            code.clone(),
            RiskPositionInput {
                qty: position.qty,
                cost_price: position.cost_price().filter(|price| price.cents() > 0),
                last_price,
                peak_price_since_entry: experience
                    .stocks
                    .get(code)
                    .and_then(|stock| stock.peak_price_since_entry),
            },
        );
    }
    build_account_risk_observation(
        entry.cash,
        &positions,
        experience.reference_equity,
        experience.peak_equity,
    )
    .map_err(|source| DecisionSnapshotCaptureError::Observation {
        location: "account risk",
        account: Some(account),
        source,
    })
}
